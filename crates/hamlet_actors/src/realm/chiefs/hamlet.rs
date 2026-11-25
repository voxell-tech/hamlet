//! A chief's acts on a hamlet: its name, its site, its mode, its
//! folder with the `hamlet_folder` table of the folders it had, and
//! its removal, return and erasure.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use hamlet_actor::BindParam;
use hamlet_messages::address::Agent;
use hamlet_messages::realm::hamlets::{
    EraseHamlet, MoveHamlet, NameHamlet, RemoveHamlet, RestoreHamlet,
    SetFolder, SetMode,
};
use hamlet_messages::realm::{
    AgentName, AgentRefusal, Done, HamletId, HamletRefusal, HowEnded,
    ProfileId, Refusable,
};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::AgentNumber;
use hamlet_messages::workspace::reach::Mode;
use serde::{Deserialize, Serialize};

use super::super::act::{Act, Decided, Decision};
use super::super::calls::Made;
use super::super::change::{self, Affected, Rows, Writer};
use super::super::record::{self, By};
use super::super::world::Part;
use super::super::{erase, profiles, retire, steps, tree};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(in crate::realm) enum Status {
    Living,
    Removed,
    Erased,
}

#[derive(Debug, PartialEq)]
pub(in crate::realm) enum Change {
    Name {
        hamlet: HamletId,
        name: String,
    },
    Site {
        hamlet: HamletId,
        x: f64,
        z: f64,
    },
    Mode {
        hamlet: HamletId,
        mode: Mode,
    },
    Folder {
        hamlet: HamletId,
        folder: Option<PathBuf>,
    },
    Status {
        hamlet: HamletId,
        status: Status,
    },
}

pub(in crate::realm) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Name { hamlet, name } => {
            set(writer, &hamlet, "name = ?2", BindParam::Text(name))
                .await?;
            Ok(Affected::seen([Part::Hamlets]))
        }
        Change::Site { hamlet, x, z } => {
            writer
                .execute(
                    "UPDATE hamlet SET x = ?2, z = ?3 WHERE id = ?1",
                    vec![
                        BindParam::Text(hamlet.0),
                        BindParam::Float(x),
                        BindParam::Float(z),
                    ],
                )
                .await?;
            Ok(Affected::seen([Part::Hamlets]))
        }
        Change::Mode { hamlet, mode } => {
            let mode = BindParam::Text(change::case_name(&mode)?);
            set(writer, &hamlet, "mode = ?2", mode).await?;
            reached(&writer.rows(), &hamlet).await
        }
        Change::Folder { hamlet, folder } => {
            let folder = folder.as_deref().map(text).transpose()?;
            let bound = folder
                .clone()
                .map_or(BindParam::Null, BindParam::Text);
            set(writer, &hamlet, "folder = ?2", bound).await?;
            if let Some(folder) = folder {
                remember(writer, &hamlet, folder).await?;
            }
            reached(&writer.rows(), &hamlet).await
        }
        Change::Status { hamlet, status } => {
            let status = BindParam::Text(change::case_name(&status)?);
            set(writer, &hamlet, "status = ?2", status).await?;
            Ok(Affected::seen([Part::Hamlets]))
        }
    }
}

async fn set(
    writer: &Writer<'_>,
    hamlet: &HamletId,
    column: &str,
    value: BindParam,
) -> Result<()> {
    writer
        .execute(
            &format!("UPDATE hamlet SET {column} WHERE id = ?1"),
            vec![BindParam::Text(hamlet.0.clone()), value],
        )
        .await?;
    Ok(())
}

fn text(folder: &Path) -> Result<String> {
    folder.to_str().map(str::to_owned).with_context(|| {
        format!("the folder {} is not UTF-8", folder.display())
    })
}

/// Keeps `folder` among the folders `hamlet` had, once.
async fn remember(
    writer: &Writer<'_>,
    hamlet: &HamletId,
    folder: String,
) -> Result<()> {
    let params = vec![
        BindParam::Text(hamlet.0.clone()),
        BindParam::Text(folder),
        BindParam::Integer(writer.at().0),
    ];
    let kept = writer
        .execute(
            "UPDATE hamlet_folder SET at = ?3 WHERE hamlet = ?1 AND \
             folder = ?2",
            params.clone(),
        )
        .await?;
    if kept == 0 {
        writer
            .execute(
                "INSERT INTO hamlet_folder (hamlet, folder, at) \
                 VALUES (?1, ?2, ?3)",
                params,
            )
            .await?;
    }
    Ok(())
}

/// The reaches and places a change of the hamlet's footing moves:
/// those of its agents.
async fn reached(
    rows: &Rows<'_>,
    hamlet: &HamletId,
) -> Result<Affected> {
    let agents = agents_of(rows, hamlet).await?;
    Ok(Affected {
        places: agents.clone(),
        reaches: agents,
        ..Affected::seen([Part::Hamlets, Part::Reaches])
    })
}

/// The hamlet's living hires and their living workers.
pub(in crate::realm) async fn agents_of(
    rows: &Rows<'_>,
    hamlet: &HamletId,
) -> Result<BTreeSet<AgentNumber>> {
    Ok(rows
        .values::<u32>(
            "SELECT agent.id FROM agent LEFT JOIN hire ON \
             hire.agent = agent.id LEFT JOIN worker ON worker.agent \
             = agent.id WHERE agent.status IN ('hiring', 'live') \
             AND (hire.hamlet = ?1 OR worker.hamlet = ?1 OR \
             worker.starter IN (SELECT agent FROM hire WHERE hamlet \
             = ?1))",
            vec![BindParam::Text(hamlet.0.clone())],
        )
        .await?
        .into_iter()
        .map(AgentNumber)
        .collect())
}

/// The hamlet `agent` works for: a hire's, or a worker's own or its
/// starter's. `None` for the Hand, its workers and an errand.
pub(in crate::realm) async fn of_agent(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<HamletId>> {
    Ok(rows
        .value::<String>(
            "SELECT hamlet FROM (SELECT hamlet FROM hire WHERE \
             agent = ?1 UNION ALL SELECT COALESCE(worker.hamlet, \
             (SELECT hamlet FROM hire WHERE agent = \
             worker.starter)) FROM worker WHERE agent = ?1) WHERE \
             hamlet IS NOT NULL",
            vec![change::number(agent)],
        )
        .await?
        .map(HamletId))
}

async fn missing(
    rows: &Rows<'_>,
    hamlet: &HamletId,
) -> Result<Option<HamletRefusal>> {
    Ok(super::hamlet(rows, hamlet)
        .await?
        .is_none()
        .then(|| HamletRefusal::NoSuchHamlet { id: hamlet.clone() }))
}

/// The most characters a hamlet's name has.
const NAME_MOST: usize = 40;

/// `name` trimmed, when it is one line of 1 to [`NAME_MOST`]
/// characters with no colon.
pub(in crate::realm) fn checked_name(
    name: &str,
) -> Result<&str, HamletRefusal> {
    let name = name.trim();
    let shaped = (1..=NAME_MOST).contains(&name.chars().count())
        && !name.contains(char::is_control)
        && !name.contains(':');
    if shaped {
        Ok(name)
    } else {
        Err(HamletRefusal::BadName {
            name: name.to_owned(),
        })
    }
}

fn decided(
    refusal: Option<HamletRefusal>,
    change: Change,
    entry: record::Entry,
) -> Decision<(), Done<HamletRefusal>> {
    match refusal {
        Some(why) => Decision::Answer(Done::refused(why)),
        None => Decision::Change(
            Decided::new(()).change(change).record(entry),
        ),
    }
}

impl Act<Client> for NameHamlet {
    type Output = Done<HamletRefusal>;
    type Loaded = Option<HamletRefusal>;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<HamletRefusal>> {
        missing(rows, &self.hamlet).await
    }

    fn decide(
        &self,
        _by: &Client,
        refusal: Option<HamletRefusal>,
    ) -> Decision<(), Self::Output> {
        let name = checked_name(&self.name);
        let refusal = refusal.or_else(|| name.clone().err());
        let hamlet = self.hamlet.clone();
        decided(
            refusal,
            Change::Name {
                hamlet: hamlet.clone(),
                name: name.unwrap_or_default().to_owned(),
            },
            record::Entry::NamedHamlet {
                hamlet,
                by: By::Chief,
            },
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

impl Act<Client> for MoveHamlet {
    type Output = Done<HamletRefusal>;
    type Loaded = Option<HamletRefusal>;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<HamletRefusal>> {
        missing(rows, &self.hamlet).await
    }

    fn decide(
        &self,
        _by: &Client,
        refusal: Option<HamletRefusal>,
    ) -> Decision<(), Self::Output> {
        let is_site = self.x.is_finite() && self.z.is_finite();
        let refusal = refusal.or_else(|| {
            (!is_site).then(|| HamletRefusal::BadSite {
                site: format!("{}, {}", self.x, self.z),
            })
        });
        let hamlet = self.hamlet.clone();
        decided(
            refusal,
            Change::Site {
                hamlet: hamlet.clone(),
                x: self.x,
                z: self.z,
            },
            record::Entry::PlacedHamlet {
                hamlet,
                by: By::Chief,
            },
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

impl Act<Client> for SetMode {
    type Output = Done<HamletRefusal>;
    type Loaded = Option<HamletRefusal>;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<HamletRefusal>> {
        missing(rows, &self.hamlet).await
    }

    fn decide(
        &self,
        _by: &Client,
        refusal: Option<HamletRefusal>,
    ) -> Decision<(), Self::Output> {
        let hamlet = self.hamlet.clone();
        decided(
            refusal,
            Change::Mode {
                hamlet: hamlet.clone(),
                mode: self.mode,
            },
            record::Entry::Mode {
                hamlet,
                by: By::Chief,
            },
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

impl Act<Client> for SetFolder {
    type Output = Done<HamletRefusal>;
    type Loaded = Option<HamletRefusal>;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<HamletRefusal>> {
        if let Some(refusal) = missing(rows, &self.hamlet).await? {
            return Ok(Some(refusal));
        }
        let Some(folder) = &self.folder else {
            return Ok(None);
        };
        let is_folder = tokio::fs::metadata(folder)
            .await
            .is_ok_and(|found| found.is_dir());
        Ok((!is_folder).then(|| HamletRefusal::NoSuchFolder {
            path: folder.clone(),
        }))
    }

    fn decide(
        &self,
        _by: &Client,
        refusal: Option<HamletRefusal>,
    ) -> Decision<(), Self::Output> {
        let hamlet = self.hamlet.clone();
        decided(
            refusal,
            Change::Folder {
                hamlet: hamlet.clone(),
                folder: self.folder.clone(),
            },
            record::Entry::Folder {
                hamlet,
                by: By::Chief,
            },
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

pub(in crate::realm) async fn status(
    rows: &Rows<'_>,
    hamlet: &HamletId,
) -> Result<Option<Status>> {
    rows.value::<String>(
        "SELECT status FROM hamlet WHERE id = ?1",
        vec![BindParam::Text(hamlet.0.clone())],
    )
    .await?
    .map(change::from_case_name)
    .transpose()
}

/// The refusal of an act on `hamlet`, which is `status`, unless the
/// act is on a hamlet that is `wanted`.
fn unless(
    hamlet: &HamletId,
    status: Option<Status>,
    wanted: Status,
) -> Option<HamletRefusal> {
    let id = hamlet.clone();
    match status {
        None => Some(HamletRefusal::NoSuchHamlet { id }),
        Some(status) if status == wanted => None,
        Some(Status::Erased) => Some(HamletRefusal::Erased { id }),
        Some(Status::Removed) => Some(HamletRefusal::Removed { id }),
        Some(Status::Living) => Some(HamletRefusal::Living { id }),
    }
}

/// Whose hires to pick.
pub(in crate::realm) enum HiresOf<'a> {
    Hamlet(&'a HamletId),
    Profile(ProfileId),
}

/// Which hires to pick.
pub(in crate::realm) enum Picked {
    Living,
    /// Ended and not erased.
    Ended,
}

/// The hires of `of` that `picked` names, by number.
pub(in crate::realm) async fn hires_where(
    rows: &Rows<'_>,
    of: HiresOf<'_>,
    picked: Picked,
) -> Result<Vec<AgentNumber>> {
    let (column, key) = match of {
        HiresOf::Hamlet(hamlet) => {
            ("hamlet", BindParam::Text(hamlet.0.clone()))
        }
        HiresOf::Profile(profile) => {
            ("profile", BindParam::Integer(i64::from(profile.0)))
        }
    };
    let picked = match picked {
        Picked::Living => "agent.status IN ('hiring', 'live')",
        Picked::Ended => {
            "agent.status = 'ended' AND agent.erased = 0"
        }
    };
    Ok(rows
        .values::<u32>(
            &format!(
                "SELECT hire.agent FROM hire JOIN agent ON agent.id \
                 = hire.agent WHERE hire.{column} = ?1 AND {picked} \
                 ORDER BY hire.agent"
            ),
            vec![key],
        )
        .await?
        .into_iter()
        .map(AgentNumber)
        .collect())
}

pub(in crate::realm) struct Removing {
    refusal: Option<HamletRefusal>,
    /// Each living hire, with the changes that end it.
    hires: Vec<(AgentNumber, Vec<change::Change>)>,
}

/// The reason a removal's hires end with, a chief's by default.
fn removal_why(act: &RemoveHamlet, by: &By) -> Option<String> {
    let given = act
        .why
        .as_deref()
        .map(str::trim)
        .filter(|why| !why.is_empty());
    match (given, by) {
        (Some(why), _) => Some(why.to_owned()),
        (None, By::Chief) => Some("closed by the chief".to_owned()),
        (None, By::Agent(_)) => None,
    }
}

async fn removing(
    act: &RemoveHamlet,
    rows: &Rows<'_>,
    by: &By,
    named: Option<AgentName>,
) -> Result<Removing> {
    let found = status(rows, &act.hamlet).await?;
    let refusal = unless(&act.hamlet, found, Status::Living);
    let mut hires = Vec::new();
    if refusal.is_none() {
        let how = HowEnded::HamletRemoved { by: named };
        let why = removal_why(act, by);
        for hire in hires_where(
            rows,
            HiresOf::Hamlet(&act.hamlet),
            Picked::Living,
        )
        .await?
        {
            let changes =
                tree::ending(rows, hire, how.clone(), why.clone())
                    .await?;
            hires.push((hire, changes));
        }
    }
    Ok(Removing { refusal, hires })
}

/// Its hires ended, with no entry of each, and the hamlet removed.
fn removed(
    act: &RemoveHamlet,
    by: By,
    removing: Removing,
) -> Result<Decided<Vec<AgentNumber>>, HamletRefusal> {
    if let Some(why) = removing.refusal {
        return Err(why);
    }
    let mut decided = Decided::new(Vec::new());
    for (hire, changes) in removing.hires {
        decided.done.push(hire);
        decided.changes.extend(changes);
    }
    Ok(decided
        .change(Change::Status {
            hamlet: act.hamlet.clone(),
            status: Status::Removed,
        })
        .record(record::Entry::RemovedHamlet {
            hamlet: act.hamlet.clone(),
            why: removal_why(act, &by),
            by,
        }))
}

fn retired(hires: &[AgentNumber]) -> Vec<steps::StepId> {
    hires.iter().copied().map(retire::row).collect()
}

impl Act<Client> for RemoveHamlet {
    type Output = Done<HamletRefusal>;
    type Loaded = Removing;
    /// The hires it ended.
    type Done = Vec<AgentNumber>;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Removing> {
        removing(self, rows, &By::Chief, None).await
    }

    fn decide(
        &self,
        _by: &Client,
        removing: Removing,
    ) -> Decision<Vec<AgentNumber>, Self::Output> {
        match removed(self, By::Chief, removing) {
            Ok(decided) => Decision::Change(decided),
            Err(why) => Decision::Answer(Done::refused(why)),
        }
    }

    fn answers_after(
        &self,
        hires: &Vec<AgentNumber>,
    ) -> Vec<steps::StepId> {
        retired(hires)
    }

    fn answer(
        &self,
        _hires: Vec<AgentNumber>,
        _made: &Made,
    ) -> Self::Output {
        Done::Done
    }
}

/// The Hand's `remove_hamlet`, once a chief allows it.
impl Act<Agent> for RemoveHamlet {
    type Output = Done<AgentRefusal<HamletRefusal>>;
    type Loaded = Removing;
    type Done = Vec<AgentNumber>;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Removing> {
        let named = record::agent_name(rows, by.0).await?;
        removing(self, rows, &By::Agent(by.0), Some(named)).await
    }

    fn decide(
        &self,
        by: &Agent,
        removing: Removing,
    ) -> Decision<Vec<AgentNumber>, Self::Output> {
        match removed(self, By::Agent(by.0), removing) {
            Ok(decided) => Decision::Change(decided),
            Err(why) => Decision::Answer(Done::refused(
                AgentRefusal::Act(why),
            )),
        }
    }

    fn answers_after(
        &self,
        hires: &Vec<AgentNumber>,
    ) -> Vec<steps::StepId> {
        retired(hires)
    }

    fn answer(
        &self,
        _hires: Vec<AgentNumber>,
        _made: &Made,
    ) -> Self::Output {
        Done::Done
    }
}

pub(in crate::realm) struct Restoring {
    status: Option<Status>,
    /// The hires its removal ended.
    hires: Vec<AgentNumber>,
    /// The first of their profiles that is not living, by its label.
    gone: Option<String>,
}

impl Act<Client> for RestoreHamlet {
    type Output = Done<HamletRefusal>;
    type Loaded = Restoring;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Restoring> {
        let mut hires = Vec::new();
        let mut gone = None;
        for hire in hires_where(
            rows,
            HiresOf::Hamlet(&self.hamlet),
            Picked::Ended,
        )
        .await?
        {
            let Some(left) = tree::left(rows, hire).await? else {
                continue;
            };
            if !matches!(left.how, HowEnded::HamletRemoved { .. }) {
                continue;
            }
            let profile = tree::profile_of(rows, hire).await?;
            if gone.is_none()
                && !profiles::is_living(rows, profile).await?
            {
                gone = Some(profiles::label_of(rows, profile).await?);
            }
            hires.push(hire);
        }
        Ok(Restoring {
            status: status(rows, &self.hamlet).await?,
            hires,
            gone,
        })
    }

    fn decide(
        &self,
        _by: &Client,
        restoring: Restoring,
    ) -> Decision<(), Self::Output> {
        let refusal =
            unless(&self.hamlet, restoring.status, Status::Removed)
                .or_else(|| {
                    restoring.gone.map(|label| {
                        HamletRefusal::ProfileGone { label }
                    })
                });
        if let Some(why) = refusal {
            return Decision::Answer(Done::refused(why));
        }
        let mut decided = Decided::new(()).change(Change::Status {
            hamlet: self.hamlet.clone(),
            status: Status::Living,
        });
        for agent in restoring.hires {
            decided = decided
                .change(tree::Change::Back { agent, lead: None });
        }
        Decision::Change(decided.record(
            record::Entry::RestoredHamlet {
                hamlet: self.hamlet.clone(),
                by: By::Chief,
                why: self.why.clone(),
            },
        ))
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

pub(in crate::realm) struct Erasing {
    status: Option<Status>,
    /// Its hires not erased.
    hires: Vec<AgentNumber>,
}

impl Act<Client> for EraseHamlet {
    type Output = Done<HamletRefusal>;
    type Loaded = Erasing;
    /// The hires it erased.
    type Done = Vec<AgentNumber>;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Erasing> {
        Ok(Erasing {
            status: status(rows, &self.hamlet).await?,
            hires: hires_where(
                rows,
                HiresOf::Hamlet(&self.hamlet),
                Picked::Ended,
            )
            .await?,
        })
    }

    fn decide(
        &self,
        _by: &Client,
        erasing: Erasing,
    ) -> Decision<Vec<AgentNumber>, Self::Output> {
        if let Some(why) =
            unless(&self.hamlet, erasing.status, Status::Removed)
        {
            return Decision::Answer(Done::refused(why));
        }
        let mut decided = Decided::new(erasing.hires.clone());
        for agent in erasing.hires {
            decided = decided.change(tree::Change::Erased { agent });
        }
        Decision::Change(
            decided
                .change(Change::Status {
                    hamlet: self.hamlet.clone(),
                    status: Status::Erased,
                })
                .record(record::Entry::ErasedHamlet {
                    hamlet: self.hamlet.clone(),
                    by: By::Chief,
                }),
        )
    }

    fn answers_after(
        &self,
        hires: &Vec<AgentNumber>,
    ) -> Vec<steps::StepId> {
        hires.iter().copied().map(erase::row).collect()
    }

    fn answer(
        &self,
        _hires: Vec<AgentNumber>,
        _made: &Made,
    ) -> Self::Output {
        Done::Done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_that_is_no_number_is_refused() {
        let moved = MoveHamlet {
            hamlet: HamletId("h".to_owned()),
            x: f64::NAN,
            z: 4.0,
        };
        let Decision::Answer(Done::Refused { why }) =
            moved.decide(&Client, None)
        else {
            panic!("a site that is no number is refused");
        };
        assert_eq!(why.to_string(), "`NaN, 4` is not an address");
    }
}
