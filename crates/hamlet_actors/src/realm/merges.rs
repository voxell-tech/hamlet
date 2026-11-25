//! A chief's `/merge` and a lead's `merge_hires`, a chief's
//! `/restore` of an ended hire or of one merged away, and the
//! `merge` and `split` rows that carry a merge's files.

use anyhow::{Context as _, Result, bail};
use hamlet_actor::BindParam;
use hamlet_messages::address::{Agent, Profile};
use hamlet_messages::agent::{AgentKind, Fill};
use hamlet_messages::profile::{
    Bundle, BundleAnswer, CommitId, MergeAnswer, MergeBranch, Sha1,
    Source, Unmerge, UnmergeAnswer,
};
use hamlet_messages::realm::hires::{
    EndedRefusal, Merge, MergeRefusal, MergedTo, Restore, Restored,
};
use hamlet_messages::realm::{
    AgentName, Done, HamletId, HowEnded, LeadRefusal, Named,
    Refusable,
};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;
use serde::{Deserialize, Serialize};

use super::act::{Act, Decided, Decision};
use super::callers::{Decider as _, Lead};
use super::calls::{Calls, Made};
use super::change::{self, Affected, Rows, Writer, integer, number};
use super::peer::{self, When};
use super::steps::{self, Cx, Kind, StepId, Steps};
use super::tree::{self, Life, Lineage};
use super::{
    drop_parts, erase, hire, laid, profiles, record, retire, workers,
};

/// A chief's reason for a merge when it gives none.
const CHIEFS_REASON: &str = "merged by the chief";

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// The merge `merge` of `absorbed` into `kept`, whose files its
    /// row brings over in a commit with `message`.
    Begin {
        merge: u64,
        absorbed: AgentNumber,
        kept: AgentNumber,
        message: String,
    },
    /// The merge's commit, once its profile made it.
    Tip { merge: u64, tip: CommitId },
    /// A restore took the merge out again.
    Split { merge: u64 },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Begin {
            merge,
            absorbed,
            kept,
            message,
        } => {
            writer
                .execute(
                    "INSERT INTO merge (id, absorbed, kept, \
                     request, split_request, message, at) VALUES \
                     (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    vec![
                        integer(merge)?,
                        number(absorbed),
                        number(kept),
                        BindParam::Text(RequestId::new().to_string()),
                        BindParam::Text(RequestId::new().to_string()),
                        BindParam::Text(message),
                        BindParam::Integer(writer.at().0),
                    ],
                )
                .await?;
        }
        Change::Tip { merge, tip } => {
            writer
                .execute(
                    "UPDATE merge SET tip = ?2 WHERE id = ?1",
                    vec![
                        integer(merge)?,
                        BindParam::Text(tip.0.to_string()),
                    ],
                )
                .await?;
        }
        Change::Split { merge } => {
            writer
                .execute(
                    "UPDATE merge SET split = 1 WHERE id = ?1",
                    vec![integer(merge)?],
                )
                .await?;
        }
    }
    Ok(Affected::default())
}

/// A merge as its rows read it.
struct Kept {
    absorbed: AgentNumber,
    kept: AgentNumber,
    request: RequestId,
    split_request: RequestId,
    message: String,
    tip: Option<CommitId>,
}

impl Kept {
    async fn read(rows: &Rows<'_>, merge: u64) -> Result<Self> {
        let (absorbed, kept, request, split_request, message, tip) =
            rows.row::<(u32, u32, String, String, String, Option<String>)>(
                "SELECT absorbed, kept, request, split_request, \
                 message, tip FROM merge WHERE id = ?1",
                vec![integer(merge)?],
            )
            .await?
            .with_context(|| format!("no merge {merge}"))?;
        let tip = tip
            .map(|tip| {
                Sha1::from_hex(&tip)
                    .map(CommitId)
                    .with_context(|| format!("merge {merge}'s tip"))
            })
            .transpose()?;
        Ok(Self {
            absorbed: AgentNumber(absorbed),
            kept: AgentNumber(kept),
            request: RequestId::try_from(request)?,
            split_request: RequestId::try_from(split_request)?,
            message,
            tip,
        })
    }
}

/// The row that brings the merge's files over.
fn row(merge: u64) -> StepId {
    StepId {
        kind: Kind::Merge,
        n: merge,
    }
}

/// The row that takes the merge out again.
fn split_row(merge: u64) -> StepId {
    StepId {
        kind: Kind::Split,
        n: merge,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Retire,
    Merge,
    Fill,
    Inbox,
}

impl Steps for Step {
    const ALL: &'static [Self] =
        &[Self::Retire, Self::Merge, Self::Fill, Self::Inbox];

    fn name(self) -> &'static str {
        match self {
            Self::Retire => "retire",
            Self::Merge => "merge",
            Self::Fill => "fill",
            Self::Inbox => "inbox",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SplitStep {
    Unmerge,
    Fill,
}

impl Steps for SplitStep {
    const ALL: &'static [Self] = &[Self::Unmerge, Self::Fill];

    fn name(self) -> &'static str {
        match self {
            Self::Unmerge => "unmerge",
            Self::Fill => "fill",
        }
    }
}

/// What a merge decides over.
pub(super) struct Merging {
    absorbed: Option<Lineage>,
    into: Option<Lineage>,
    merge: u64,
    /// The other ends of the absorbed hire's open roads.
    roads: Vec<(AgentNumber, laid::Kind)>,
    /// The other ends of the kept hire's open roads.
    kept_roads: Vec<AgentNumber>,
    /// The changes that stop the absorbed hire's workers.
    workers: Vec<change::Change>,
    address: String,
    /// The agent whose `merge_hires` it is; `None` for a chief.
    by: Option<AgentName>,
}

/// What a merge made.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Merged {
    merge: u64,
    absorbed: AgentName,
    into: AgentName,
}

async fn load_merge(
    merge: &Merge,
    by: &impl Lead,
    rows: &Rows<'_>,
) -> Result<Merging> {
    let absorbed = tree::named(rows, by, &merge.absorbed).await?;
    let into = tree::named(rows, by, &merge.into).await?;
    let (roads, workers, address) = match &absorbed {
        Some(lineage) => (
            laid::of(rows, lineage.agent.agent).await?,
            workers::end_with(rows, lineage.agent.agent).await?,
            record::address(rows, lineage.agent.agent).await?,
        ),
        None => (Vec::new(), Vec::new(), String::new()),
    };
    let kept_roads = match &into {
        Some(lineage) => laid::of(rows, lineage.agent.agent)
            .await?
            .into_iter()
            .map(|(other, _)| other)
            .collect(),
        None => Vec::new(),
    };
    let by = match by.by() {
        record::By::Chief => None,
        record::By::Agent(agent) => {
            Some(record::agent_name(rows, agent).await?)
        }
    };
    Ok(Merging {
        absorbed,
        into,
        merge: rows.next_id("merge").await?,
        roads,
        kept_roads,
        workers,
        address,
        by,
    })
}

fn decide_merge<By: Lead>(
    merge: &Merge,
    by: &By,
    merging: Merging,
) -> Result<Decided<Merged>, By::Why<MergeRefusal>> {
    let no_one = |named: &Named| {
        By::refused(MergeRefusal::NoSuchAgent {
            address: named.to_string(),
        })
    };
    // A chief's two names are found first, as today's door finds
    // them.
    if merging.by.is_none()
        && merging.absorbed.is_some()
        && merging.into.is_none()
    {
        return Err(no_one(&merge.into));
    }
    let led = |named: &Named, found: Option<Lineage>| {
        let found = found.ok_or_else(|| no_one(named))?;
        if found.agent.kind != AgentKind::Hire {
            return Err(By::refused(MergeRefusal::NotAHire {
                name: found.agent.name.name.clone(),
            }));
        }
        by.leads(&found)?;
        Ok(found)
    };
    let absorbed = led(&merge.absorbed, merging.absorbed)?;
    let into = led(&merge.into, merging.into)?;
    let (one, kept) = (absorbed.agent.agent, into.agent.agent);
    if one == kept {
        return Err(By::refused(MergeRefusal::Same));
    }
    let hamlet = |lineage: &Lineage| {
        lineage.hamlet.as_ref().map(|hamlet| hamlet.id.clone())
    };
    let Some(at) = hamlet(&absorbed)
        .filter(|at| hamlet(&into).as_ref() == Some(at))
    else {
        return Err(By::refused(MergeRefusal::OtherHamlet {
            name: absorbed.agent.name,
        }));
    };
    if absorbed.is_mayor() {
        return Err(By::refused(MergeRefusal::TheMayor));
    }
    let given = merge
        .why
        .as_deref()
        .map(str::trim)
        .filter(|why| !why.is_empty());
    let reason = match (given, &merging.by) {
        (Some(given), _) => given,
        (None, None) => CHIEFS_REASON,
        (None, Some(_)) => "",
    };
    let why = Some(reason.to_owned()).filter(|why| !why.is_empty());
    let how = HowEnded::Merged {
        into: into.agent.name.clone(),
        by: merging.by,
    };
    let mut decided = Decided::new(Merged {
        merge: merging.merge,
        absorbed: absorbed.agent.name,
        into: into.agent.name,
    })
    .change(tree::Change::Absorb {
        absorbed: one,
        kept,
    });
    decided.changes.extend(moved_roads(
        one,
        kept,
        &at,
        &merging.roads,
        &merging.kept_roads,
    ));
    decided.changes.extend(tree::hire_ending(
        one,
        how,
        why.clone(),
        merging.workers,
    ));
    Ok(decided
        .change(Change::Begin {
            merge: merging.merge,
            absorbed: one,
            kept,
            message: format!(
                "Took in what {} learned: {reason}",
                merging.address
            ),
        })
        .change(steps::Change::Begin {
            row: row(merging.merge),
        })
        .record(record::Entry::Merged {
            absorbed: one,
            kept,
            by: by.by(),
            why,
        }))
}

/// The changes that take up each road laid to `absorbed` and lay it
/// again from `kept`, unless its other end is `kept` or `kept` has a
/// road there.
fn moved_roads(
    absorbed: AgentNumber,
    kept: AgentNumber,
    hamlet: &HamletId,
    roads: &[(AgentNumber, laid::Kind)],
    kept_roads: &[AgentNumber],
) -> Vec<change::Change> {
    let mut changes = Vec::new();
    for &(other, kind) in roads {
        if kind != laid::Kind::Tree {
            continue;
        }
        changes.push(
            laid::Change::TakeUp {
                a: absorbed,
                b: other,
            }
            .into(),
        );
        if other != kept && !kept_roads.contains(&other) {
            changes.push(
                laid::Change::Lay {
                    a: kept,
                    b: other,
                    hamlet: hamlet.clone(),
                }
                .into(),
            );
        }
    }
    changes
}

impl Act<Client> for Merge {
    type Output = Done<MergeRefusal>;
    type Loaded = Merging;
    type Done = Merged;

    async fn load(
        &self,
        by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Merging> {
        load_merge(self, by, rows).await
    }

    fn decide(
        &self,
        by: &Client,
        merging: Merging,
    ) -> Decision<Merged, Self::Output> {
        match decide_merge(self, by, merging) {
            Ok(decided) => Decision::Change(decided),
            Err(why) => Decision::Answer(Done::refused(why)),
        }
    }

    fn answers_after(&self, merged: &Merged) -> Vec<StepId> {
        vec![row(merged.merge)]
    }

    fn answer(&self, _: Merged, _made: &Made) -> Self::Output {
        Done::Done
    }
}

impl Act<Agent> for Merge {
    type Output = MergedTo<LeadRefusal<MergeRefusal>>;
    type Loaded = Merging;
    type Done = Merged;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Merging> {
        load_merge(self, by, rows).await
    }

    fn decide(
        &self,
        by: &Agent,
        merging: Merging,
    ) -> Decision<Merged, Self::Output> {
        match decide_merge(self, by, merging) {
            Ok(decided) => Decision::Change(decided),
            Err(why) => Decision::Answer(MergedTo::refused(why)),
        }
    }

    fn answers_after(&self, merged: &Merged) -> Vec<StepId> {
        vec![row(merged.merge)]
    }

    fn answer(&self, merged: Merged, _made: &Made) -> Self::Output {
        MergedTo::Done {
            absorbed: merged.absorbed,
            into: merged.into,
        }
    }
}

/// What a restore decides over, once the hire it names has ended.
pub(super) struct Coming {
    agent: AgentNumber,
    hamlet: HamletId,
    removed: bool,
    /// The label of its profile, when the profile has left the city.
    gone: Option<String>,
    living: u32,
    most: u32,
    /// The lead it comes back under.
    lead: AgentNumber,
    /// The merge that absorbed it, while it stands, and the hire
    /// that kept it.
    merge: Option<(u64, AgentNumber)>,
}

/// What a restore did.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(super) enum Restoring {
    Back,
    Split { merge: u64 },
}

impl Act<Client> for Restore {
    type Output = Restored;
    type Loaded = Result<Coming, EndedRefusal>;
    type Done = Restoring;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Self::Loaded> {
        let found = tree::kept_named(rows, &self.hire).await?;
        let hire = match erase::ended_hire(&self.hire, found) {
            Ok(hire) => hire,
            Err(why) => return Ok(Err(why)),
        };
        let agent = hire.agent.agent;
        let hamlet = hire.hamlet.context("an ended hire's hamlet")?;
        let removed = status(
            rows,
            "hamlet",
            BindParam::Text(hamlet.id.0.clone()),
        )
        .await?
        .as_deref()
            != Some("living");
        let profile = tree::profile_of(rows, agent).await?;
        let gone = if status(
            rows,
            "profile",
            BindParam::Integer(profile.0.into()),
        )
        .await?
        .as_deref()
            == Some("living")
        {
            None
        } else {
            Some(profiles::label_of(rows, profile).await?)
        };
        let lead = match hire.agent.lead {
            Some(lead)
                if lead != agent
                    && tree::life(rows, lead).await?
                        == Life::Living =>
            {
                lead
            }
            _ => hamlet.mayor,
        };
        let merge = rows
            .row::<(u64, u32)>(
                "SELECT id, kept FROM merge WHERE absorbed = ?1 AND \
                 split = 0 ORDER BY id DESC LIMIT 1",
                vec![number(agent)],
            )
            .await?
            .map(|(merge, kept)| (merge, AgentNumber(kept)));
        Ok(Ok(Coming {
            agent,
            living: tree::living(rows, &hamlet.id).await?,
            most: hamlet.most(),
            hamlet: hamlet.id,
            removed,
            gone,
            lead,
            merge,
        }))
    }

    fn decide(
        &self,
        by: &Client,
        loaded: Self::Loaded,
    ) -> Decision<Restoring, Restored> {
        let refuse =
            |why| Decision::Answer(Restored::Refused { why });
        let coming = match loaded {
            Ok(coming) => coming,
            Err(why) => return refuse(why),
        };
        if coming.removed {
            return refuse(EndedRefusal::HamletRemoved {
                hamlet: coming.hamlet,
            });
        }
        if let Some(label) = coming.gone {
            return refuse(EndedRefusal::ProfileGone { label });
        }
        if coming.living >= coming.most {
            return refuse(EndedRefusal::Full {
                hamlet: coming.hamlet,
                most: coming.most,
            });
        }
        let agent = coming.agent;
        let done = match coming.merge {
            Some((merge, _)) => Restoring::Split { merge },
            None => Restoring::Back,
        };
        let mut decided = Decided::new(done)
            .change(tree::Change::Back {
                agent,
                lead: Some(coming.lead),
            })
            .record(record::Entry::Restored {
                hire: agent,
                by: by.by(),
                why: self.why.clone(),
            });
        if let Some((merge, kept)) = coming.merge {
            decided = decided
                .change(Change::Split { merge })
                .change(steps::Change::Begin {
                    row: split_row(merge),
                })
                .record(record::Entry::Split {
                    absorbed: agent,
                    kept,
                    by: by.by(),
                    why: self.why.clone(),
                });
        }
        Decision::Change(decided)
    }

    fn answers_after(&self, done: &Restoring) -> Vec<StepId> {
        match done {
            Restoring::Back => Vec::new(),
            Restoring::Split { merge } => vec![split_row(*merge)],
        }
    }

    fn answer(&self, done: Restoring, _made: &Made) -> Restored {
        match done {
            Restoring::Back => Restored::Back,
            Restoring::Split { .. } => Restored::Split,
        }
    }
}

async fn status(
    rows: &Rows<'_>,
    table: &str,
    id: BindParam,
) -> Result<Option<String>> {
    rows.value(
        &format!("SELECT status FROM {table} WHERE id = ?1"),
        vec![id],
    )
    .await
}

pub(super) async fn run(
    cx: &Cx<'_>,
    n: u64,
    step: Step,
) -> Result<Vec<change::Change>> {
    let merge = Kept::read(&cx.rows()?, n).await?;
    match step {
        Step::Retire => retire::retire(cx, merge.absorbed).await,
        Step::Merge => merged(cx, n, &merge).await,
        Step::Fill => {
            fill(cx, merge.kept).await?;
            Ok(Vec::new())
        }
        // A parcel goes with a letter, which would give the kept hire
        // a turn a merge never gives: the absorbed hire's inbox
        // stays.
        Step::Inbox => Ok(Vec::new()),
    }
}

/// Merges the absorbed hire's branch into the kept hire's, and keeps
/// the merge's commit.
async fn merged(
    cx: &Cx<'_>,
    n: u64,
    merge: &Kept,
) -> Result<Vec<change::Change>> {
    let rows = cx.rows()?;
    if !retire::reached(&rows, merge.absorbed, hire::Step::Branch)
        .await?
    {
        return Ok(Vec::new());
    }
    let into = tree::profile_of(&rows, merge.kept).await?;
    let from = tree::profile_of(&rows, merge.absorbed).await?;
    let branch = tree::branch(merge.absorbed);
    let merging = |from| MergeBranch {
        id: merge.request.clone(),
        into: tree::branch(merge.kept),
        absorbed: merge.absorbed,
        message: merge.message.clone(),
        from,
    };
    let call = async |source| {
        cx.peers()
            .call(&Profile(into), &merging(source), When::AfterAnswer)
            .await
    };
    let answer = if into == from {
        call(Source::Branch { name: branch }).await?
    } else {
        let mut after = None;
        let mut part = 0;
        loop {
            let read = Bundle {
                branch: branch.clone(),
                after,
            };
            let (files, removed, next) = match cx
                .peers()
                .call(&Profile(from), &read, When::AfterAnswer)
                .await?
            {
                BundleAnswer::Part {
                    files,
                    removed,
                    next,
                } => (files, removed, next),
                BundleAnswer::Refused { why } => {
                    return Err(peer::refusal(why));
                }
            };
            let last = next.is_none();
            let answer = call(Source::Part {
                part,
                last,
                files,
                removed,
            })
            .await?;
            if last {
                break answer;
            }
            after = next;
            part += 1;
        }
    };
    match answer {
        MergeAnswer::Merged { tip, .. } => {
            Ok(vec![Change::Tip { merge: n, tip }.into()])
        }
        MergeAnswer::Kept => bail!("merge {n}'s last part was kept"),
        MergeAnswer::Refused { why } => Err(peer::refusal(why)),
    }
}

/// Has the agent of the hire `agent`, if it was made, pull its
/// branch's tip.
async fn fill(cx: &Cx<'_>, agent: AgentNumber) -> Result<()> {
    let rows = cx.rows()?;
    if !retire::reached(&rows, agent, hire::Step::Create).await? {
        return Ok(());
    }
    let fill = Fill {
        profile: tree::profile_of(&rows, agent).await?,
    };
    let to = Agent(agent);
    let filled = cx.peers().call(&to, &fill, When::AfterAnswer);
    retire::unless_gone(filled.await)
}

pub(super) async fn failed(
    rows: &Rows<'_>,
    n: u64,
) -> Result<(Vec<change::Change>, Calls)> {
    let merge = Kept::read(rows, n).await?;
    let changes = vec![
        steps::Change::Close { row: row(n) }.into(),
        steps::Change::Begin {
            row: drop_parts::row(merge.absorbed),
        }
        .into(),
    ];
    Ok((changes, Calls::none()))
}

pub(super) async fn split(
    cx: &Cx<'_>,
    n: u64,
    step: SplitStep,
) -> Result<Vec<change::Change>> {
    match step {
        SplitStep::Unmerge => unmerge(cx, n).await?,
        SplitStep::Fill => {
            fill(cx, Kept::read(&cx.rows()?, n).await?.kept).await?;
        }
    }
    Ok(Vec::new())
}

/// Takes the merge's commit out of the kept hire's branch, once the
/// merge's own row has ended: before that its commit may yet come.
async fn unmerge(cx: &Cx<'_>, n: u64) -> Result<()> {
    cx.realm.drives.ended(row(n), cx.realm.answer_within).await;
    let rows = cx.rows()?;
    if steps::row(&rows, row(n))
        .await?
        .is_some_and(|row| !row.closed)
    {
        bail!("merge {n} has not ended");
    }
    let merge = Kept::read(&rows, n).await?;
    let Some(tip) = merge.tip else {
        return Ok(());
    };
    let unmerged = Unmerge {
        id: merge.split_request,
        branch: tree::branch(merge.kept),
        merge: tip,
    };
    let profile = tree::profile_of(&rows, merge.kept).await?;
    match cx
        .peers()
        .call(&Profile(profile), &unmerged, When::AfterAnswer)
        .await?
    {
        UnmergeAnswer::Unmerged { .. } => Ok(()),
        UnmergeAnswer::Refused { why } => Err(peer::refusal(why)),
    }
}

pub(super) async fn split_failed(
    _rows: &Rows<'_>,
    n: u64,
) -> Result<(Vec<change::Change>, Calls)> {
    let close = steps::Change::Close { row: split_row(n) };
    Ok((vec![close.into()], Calls::none()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restore_into_a_full_hamlet_is_refused() {
        let restore = Restore {
            hire: Named::Agent {
                agent: AgentNumber(3),
            },
            why: None,
        };
        let hamlet = HamletId("mill".to_owned());
        let coming = Coming {
            agent: AgentNumber(3),
            hamlet: hamlet.clone(),
            removed: false,
            gone: None,
            living: 50,
            most: 50,
            lead: AgentNumber(2),
            merge: None,
        };

        assert_eq!(
            restore.decide(&Client, Ok(coming)),
            Decision::Answer(Restored::Refused {
                why: EndedRefusal::Full { hamlet, most: 50 }
            })
        );
    }
}
