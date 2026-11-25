//! A chief's `/merge` and a lead's `merge_hires`, a chief's
//! `/restore` of an ended hire or of one merged away, and the
//! `merge` and `split` rows that carry a merge's files.

use anyhow::{Context as _, Result, bail};
use hamlet_actor::BindParam;
use hamlet_messages::address::{Agent, Profile};
use hamlet_messages::agent::{
    AgentKind, DropInbox, Fill, Input, InputId, ReadInbox, Submit,
    TakeInbox,
};
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
use super::calls::{Calls, Delivery, Made};
use super::change::{self, Affected, Rows, Writer, integer, number};
use super::chiefs::hamlet::Status as HamletStatus;
use super::peer::{self, When};
use super::steps::{self, Cx, Kind, StepId, Steps};
use super::tree::{self, Life, Lineage};
use super::{
    chiefs, erase, hire, laid, on_task, profiles, record, retire,
    workers,
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
        moved: Moved,
    },
    /// The merge's commit, once its profile made it.
    Tip { merge: u64, tip: CommitId },
    /// A restore took the merge out again, or it was taken back.
    Split { merge: u64 },
    /// A split taken back: the merge stands again.
    Unsplit { merge: u64 },
}

/// What a merge moved in the tree, kept with its row so that a merge
/// taken back gives it back.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Moved {
    /// The lead whose `merge_hires` it was.
    by: Option<AgentNumber>,
    /// Whether the kept hire was under the absorbed one.
    kept_was_under: bool,
    /// The absorbed hire's living hires, moved under the kept one.
    hires: Vec<AgentNumber>,
    /// The other end of each tree road taken up from the absorbed
    /// hire, with whether the kept hire was given it.
    roads: Vec<(AgentNumber, bool)>,
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
            moved,
        } => {
            writer
                .execute(
                    "INSERT INTO merge (id, absorbed, kept, \
                     request, split_request, message, moved, at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    vec![
                        integer(merge)?,
                        number(absorbed),
                        number(kept),
                        BindParam::Text(RequestId::new().to_string()),
                        BindParam::Text(RequestId::new().to_string()),
                        BindParam::Text(message),
                        BindParam::Text(serde_json::to_string(
                            &moved,
                        )?),
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
        Change::Unsplit { merge } => {
            writer
                .execute(
                    "UPDATE merge SET split = 0 WHERE id = ?1",
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
    moved: Moved,
    split: bool,
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
        let (moved, split) = rows
            .row::<(String, bool)>(
                "SELECT moved, split FROM merge WHERE id = ?1",
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
            moved: serde_json::from_str(&moved)?,
            split,
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

/// The merge and split rows of every merge `agent` was absorbed into
/// or kept in.
pub(super) async fn rows_of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<StepId>> {
    let merges = rows
        .values::<u64>(
            "SELECT id FROM merge WHERE kept = ?1 OR absorbed = ?1",
            vec![number(agent)],
        )
        .await?;
    Ok(merges
        .into_iter()
        .flat_map(|merge| [row(merge), split_row(merge)])
        .collect())
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
    /// The hires their agents said are on a chief's task.
    on_task: Vec<AgentNumber>,
}

impl on_task::Told for Merging {
    fn on_task(&mut self, on_task: Vec<AgentNumber>) {
        self.on_task = on_task;
    }
}

/// What a merge made.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Merged {
    merge: u64,
    absorbed: AgentName,
    into: AgentName,
}

/// Why a merge row that failed for good, its last step done `done`,
/// did not merge `absorbed` into `into`.
fn not_merged(
    absorbed: AgentName,
    into: AgentName,
    done: Option<&str>,
) -> MergeRefusal {
    if kept_inbox(done) {
        MergeRefusal::InboxKept { absorbed, into }
    } else {
        MergeRefusal::NotMerged { absorbed, into }
    }
}

/// Whether a merge row that failed for good, its last step done
/// `done`, failed at the inbox: the merge stands.
fn kept_inbox(done: Option<&str>) -> bool {
    done == Some(Step::Fill.name())
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
        on_task: Vec::new(),
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
    // A chief's two names are both found before either is checked.
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
    for hire in [&into, &absorbed] {
        if merging.on_task.contains(&hire.agent.agent) {
            return Err(By::refused(MergeRefusal::OnChiefsTask {
                name: hire.agent.name.clone(),
            }));
        }
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
    let roads =
        roads_moved(kept, &merging.roads, &merging.kept_roads);
    let moved = Moved {
        by: merging.by.as_ref().map(|by| by.agent),
        kept_was_under: absorbed
            .hires
            .iter()
            .any(|node| node.agent == kept),
        hires: absorbed
            .hires
            .iter()
            .filter(|node| {
                node.agent != kept
                    && !matches!(node.status, tree::Status::Ended(_))
            })
            .map(|node| node.agent)
            .collect(),
        roads,
    };
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
    decided
        .changes
        .extend(moved_roads(one, kept, &at, &moved.roads));
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
            moved,
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

/// The other end of each tree road in `roads`, laid to the absorbed
/// hire, with whether `kept` is given it: unless that end is `kept`
/// or `kept` has a road there.
fn roads_moved(
    kept: AgentNumber,
    roads: &[(AgentNumber, laid::Kind)],
    kept_roads: &[AgentNumber],
) -> Vec<(AgentNumber, bool)> {
    roads
        .iter()
        .filter(|(_, kind)| *kind == laid::Kind::Tree)
        .map(|&(other, _)| {
            (other, other != kept && !kept_roads.contains(&other))
        })
        .collect()
}

/// The changes that take up each of `roads` from `from` and lay each
/// one given again from `to`.
fn moved_roads(
    from: AgentNumber,
    to: AgentNumber,
    hamlet: &HamletId,
    roads: &[(AgentNumber, bool)],
) -> Vec<change::Change> {
    let mut changes = Vec::new();
    for &(other, given) in roads {
        changes
            .push(laid::Change::TakeUp { a: from, b: other }.into());
        if given {
            changes.push(
                laid::Change::Lay {
                    a: to,
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

    fn failed(
        &self,
        merged: Merged,
        _made: &Made,
        failed: &steps::Failed,
    ) -> Self::Output {
        Done::refused(not_merged(
            merged.absorbed,
            merged.into,
            failed.done.as_deref(),
        ))
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

    fn failed(
        &self,
        merged: Merged,
        _made: &Made,
        failed: &steps::Failed,
    ) -> Self::Output {
        MergedTo::refused(LeadRefusal::Act(not_merged(
            merged.absorbed,
            merged.into,
            failed.done.as_deref(),
        )))
    }
}

/// What a restore decides over, once the hire it names has ended.
pub(super) struct Coming {
    name: AgentName,
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
    merge: Option<(u64, AgentName)>,
}

/// What a restore did.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(super) enum Restoring {
    Back,
    Split {
        merge: u64,
        hire: AgentName,
        into: AgentName,
    },
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
        let removed = chiefs::hamlet::status(rows, &hamlet.id)
            .await?
            != Some(HamletStatus::Living);
        let profile = tree::profile_of(rows, agent).await?;
        let gone = if profiles::is_living(rows, profile).await? {
            None
        } else {
            Some(profiles::label_of(rows, profile).await?)
        };
        let lead =
            lead_back(rows, agent, hire.agent.lead, hamlet.mayor)
                .await?;
        let merge = match rows
            .row::<(u64, u32)>(
                "SELECT id, kept FROM merge WHERE absorbed = ?1 AND \
                 split = 0 ORDER BY id DESC LIMIT 1",
                vec![number(agent)],
            )
            .await?
        {
            Some((merge, kept)) => Some((
                merge,
                record::agent_name(rows, AgentNumber(kept)).await?,
            )),
            None => None,
        };
        Ok(Ok(Coming {
            name: hire.agent.name,
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
        let agent = coming.name.agent;
        let done = match &coming.merge {
            Some((merge, into)) => Restoring::Split {
                merge: *merge,
                hire: coming.name.clone(),
                into: into.clone(),
            },
            None => Restoring::Back,
        };
        let mut decided = Decided::new(done)
            .changes(tree::coming_back(agent, Some(coming.lead)))
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
                    kept: kept.agent,
                    by: by.by(),
                    why: self.why.clone(),
                });
        }
        Decision::Change(decided)
    }

    fn answers_after(&self, done: &Restoring) -> Vec<StepId> {
        match done {
            Restoring::Back => Vec::new(),
            Restoring::Split { merge, .. } => vec![split_row(*merge)],
        }
    }

    fn answer(&self, done: Restoring, _made: &Made) -> Restored {
        match done {
            Restoring::Back => Restored::Back,
            Restoring::Split { .. } => Restored::Split,
        }
    }

    fn failed(
        &self,
        done: Restoring,
        made: &Made,
        _failed: &steps::Failed,
    ) -> Restored {
        match done {
            Restoring::Split { hire, into, .. } => {
                Restored::Refused {
                    why: EndedRefusal::NotSplit { hire, into },
                }
            }
            Restoring::Back => self.answer(done, made),
        }
    }
}

/// The lead an ended hire comes back under: `lead`, its own, while it
/// lives, else the mayor.
async fn lead_back(
    rows: &Rows<'_>,
    agent: AgentNumber,
    lead: Option<AgentNumber>,
    mayor: AgentNumber,
) -> Result<AgentNumber> {
    Ok(match lead {
        Some(lead)
            if lead != agent
                && tree::life(rows, lead).await? == Life::Living =>
        {
            lead
        }
        _ => mayor,
    })
}

pub(super) async fn run(
    cx: &Cx<'_>,
    n: u64,
    step: Step,
) -> Result<Vec<change::Change>> {
    let merge = Kept::read(&cx.rows()?, n).await?;
    let ran = match step {
        Step::Retire => retire::retire(cx, merge.absorbed).await,
        Step::Merge => merged(cx, n, &merge).await,
        Step::Fill => fill(cx, merge.kept).await.map(|()| Vec::new()),
        Step::Inbox => inbox(cx, &merge).await.map(|()| Vec::new()),
    };
    // Once the merge's commit is made it is not taken back: the fill
    // runs until done, and an inbox the kept hire's agent refuses
    // stays with the absorbed hire.
    match step {
        Step::Retire | Step::Merge | Step::Inbox => ran,
        Step::Fill => steps::in_passing(ran),
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
    if !hire::reached(&rows, merge.absorbed, hire::Step::Branch)
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
    if !hire::reached(&rows, agent, hire::Step::Create).await? {
        return Ok(());
    }
    let fill = Fill {
        profile: tree::profile_of(&rows, agent).await?,
    };
    let to = Agent(agent);
    let filled = cx.peers().call(&to, &fill, When::AfterAnswer);
    retire::unless_gone(filled.await)?;
    Ok(())
}

/// Moves the absorbed hire's inbox to the kept hire's
/// `inbox/from-hire-<absorbed>/`, a part at a time, giving no turn.
/// Repeated from the start, it takes again what it took and has not
/// dropped.
async fn inbox(cx: &Cx<'_>, merge: &Kept) -> Result<()> {
    let rows = cx.rows()?;
    for agent in [merge.absorbed, merge.kept] {
        if !hire::reached(&rows, agent, hire::Step::Create).await? {
            return Ok(());
        }
    }
    let (from, into) = (Agent(merge.absorbed), Agent(merge.kept));
    let mut after = None;
    loop {
        let read = ReadInbox { after };
        let read = cx.peers().call(&from, &read, When::AfterAnswer);
        let Some(part) = retire::unless_gone(read.await)? else {
            return Ok(());
        };
        let taken: Vec<String> = part
            .files
            .iter()
            .filter(|piece| piece.last)
            .map(|piece| piece.path.clone())
            .collect();
        let take = TakeInbox {
            from: merge.absorbed,
            files: part.files,
        };
        let took = cx.peers().call(&into, &take, When::AfterAnswer);
        if retire::unless_gone(took.await)?.is_none() {
            return Ok(());
        }
        if !taken.is_empty() {
            let drop = DropInbox { paths: taken };
            let dropped =
                cx.peers().call(&from, &drop, When::AfterAnswer);
            retire::unless_gone(dropped.await)?;
        }
        match part.next {
            Some(next) => after = Some(next),
            None => return Ok(()),
        }
    }
}

/// Ends the merge `n`, which failed for good. At its inbox, the merge
/// stands and the inbox stays with the absorbed hire. Before its
/// commit, it is taken back: the absorbed hire lives again under its
/// lead, with what the merge moved given back, and the merge is
/// marked split. A hire restored or erased meanwhile stays as it is.
pub(super) async fn failed(
    rows: &Rows<'_>,
    n: u64,
) -> Result<(Vec<change::Change>, Calls)> {
    let merge = Kept::read(rows, n).await?;
    let done =
        steps::row(rows, row(n)).await?.and_then(|row| row.done);
    let why = not_merged(
        record::agent_name(rows, merge.absorbed).await?,
        record::agent_name(rows, merge.kept).await?,
        done.as_deref(),
    );
    if kept_inbox(done.as_deref()) {
        let changes =
            vec![steps::Change::Close { row: row(n) }.into()];
        return Ok((
            changes,
            told(rows, merge.moved.by, n, why).await?,
        ));
    }
    let mut changes = vec![
        steps::Change::Close { row: row(n) }.into(),
        Change::Split { merge: n }.into(),
    ];
    let absorbed = tree::lineage_of(rows, merge.absorbed).await?;
    let erased = rows
        .value::<bool>(
            "SELECT erased FROM agent WHERE id = ?1",
            vec![number(merge.absorbed)],
        )
        .await?
        .unwrap_or(true);
    if merge.split
        || erased
        || !matches!(absorbed.agent.status, tree::Status::Ended(_))
    {
        return Ok((changes, Calls::none()));
    }
    let hamlet =
        absorbed.hamlet.context("an absorbed hire's hamlet")?;
    let lead = lead_back(
        rows,
        merge.absorbed,
        absorbed.agent.lead,
        hamlet.mayor,
    )
    .await?;
    changes.extend(tree::coming_back(merge.absorbed, Some(lead)));
    changes.push(
        tree::Change::GiveBack {
            absorbed: merge.absorbed,
            kept: merge.kept,
            kept_was_under: merge.moved.kept_was_under,
            hires: merge.moved.hires,
        }
        .into(),
    );
    for (other, given) in merge.moved.roads {
        if given {
            changes.push(
                laid::Change::TakeUp {
                    a: merge.kept,
                    b: other,
                }
                .into(),
            );
        }
        changes.push(
            laid::Change::Lay {
                a: merge.absorbed,
                b: other,
                hamlet: hamlet.id.clone(),
            }
            .into(),
        );
    }
    changes.push(
        record::taken_back(
            rows,
            "merged",
            Some(merge.absorbed),
            row(n),
        )
        .await?,
    );
    Ok((changes, told(rows, merge.moved.by, n, why).await?))
}

/// The notice `why` of merge `n` to `lead`, whose `merge_hires` it
/// was, while it lives.
async fn told(
    rows: &Rows<'_>,
    lead: Option<AgentNumber>,
    n: u64,
    why: MergeRefusal,
) -> Result<Calls> {
    Ok(match lead {
        Some(lead)
            if tree::life(rows, lead).await? == Life::Living =>
        {
            Calls::after_answer(Delivery::Line {
                to: lead,
                submit: Submit {
                    id: InputId(format!("merge/{n}")),
                    task: None,
                    input: Input::Notice {
                        text: why.to_string(),
                        conversation: None,
                        held: None,
                    },
                },
            })
        }
        _ => Calls::none(),
    })
}

/// Whether `agent` was absorbed by a merge that stands and failed at
/// its inbox, into a hire not erased: that hire never got the inbox.
pub(super) async fn kept_its_inbox(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<bool> {
    let merges = rows
        .values::<u64>(
            "SELECT merge.id FROM merge JOIN agent ON agent.id = \
             merge.kept WHERE merge.absorbed = ?1 AND merge.split = \
             0 AND agent.erased = 0",
            vec![number(agent)],
        )
        .await?;
    for merge in merges {
        if let Some(found) = steps::row(rows, row(merge)).await?
            && found.closed
            && kept_inbox(found.done.as_deref())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) async fn split(
    cx: &Cx<'_>,
    n: u64,
    step: SplitStep,
) -> Result<Vec<change::Change>> {
    match step {
        SplitStep::Unmerge => unmerge(cx, n).await?,
        SplitStep::Fill => {
            let kept = Kept::read(&cx.rows()?, n).await?.kept;
            steps::in_passing(fill(cx, kept).await)?;
        }
    }
    Ok(Vec::new())
}

/// Takes the merge's commit out of the kept hire's branch, once the
/// merge's own row has ended: before that its commit may yet come.
async fn unmerge(cx: &Cx<'_>, n: u64) -> Result<()> {
    steps::after(cx, [row(n)]).await?;
    let rows = cx.rows()?;
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

/// Takes back the split of merge `n`, which failed for good before
/// its unmerge: the merge stands again, and the hire stays back.
pub(super) async fn split_failed(
    rows: &Rows<'_>,
    n: u64,
) -> Result<(Vec<change::Change>, Calls)> {
    let merge = Kept::read(rows, n).await?;
    let changes = vec![
        steps::Change::Close { row: split_row(n) }.into(),
        Change::Unsplit { merge: n }.into(),
        record::taken_back(
            rows,
            "split",
            Some(merge.absorbed),
            split_row(n),
        )
        .await?,
    ];
    Ok((changes, Calls::none()))
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
            name: AgentName {
                agent: AgentNumber(3),
                name: "hire#3".to_owned(),
            },
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
