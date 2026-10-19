//! Hamlet records and marker-backed changes. Callers authenticate
//! actors before constructing requests; the record checks their
//! current authority in the change's transaction.

mod document;
mod membership;
mod names;
mod refusals;
mod roads;

use anyhow::{Context, Result, ensure};
pub use document::{
    Agent, AgentRef, Amount, Archived, BroughtFile, Decision,
    DecisionKind, End, Hamlet, InheritedRoad, Limits, Merge,
    MergeFiles, MergeNotes, OpenVote, Protection, Road, RoadBudget,
    RoadCharge,
};
pub(crate) use names::trim;
pub use names::{NameFallback, check_name, tidy_name};
pub use refusals::{Action, Audience, Reason, Refusal};
pub use roads::level;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{Operation, Outcome, Store};

/// The authenticated author of a record change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Actor {
    Chief(String),
    Mayor(AgentRef),
    Member(AgentRef),
    /// The realm owner supplies both recorded agreements, not a
    /// single tool caller's claim.
    Mayors {
        leaving: AgentRef,
        joining: AgentRef,
    },
}

impl Actor {
    fn by(&self) -> String {
        match self {
            Self::Chief(name) => format!("chief:{name}"),
            Self::Mayor(_) => "mayor".into(),
            Self::Member(place) => {
                format!("{}:{}", place.hamlet, place.number)
            }
            Self::Mayors { leaving, .. } => {
                format!("mayor@{}", leaving.hamlet)
            }
        }
    }
}

/// A membership, road or office change, run with [`Store::run`].
#[derive(Clone, Debug, Serialize)]
pub struct RecordOperation {
    pub hamlet: String,
    pub actor: Actor,
    /// Server time at this attempt, excluded from the request
    /// identity.
    #[serde(skip)]
    pub at: i64,
    pub reason: String,
    pub task: Option<String>,
    pub change: Change,
}

/// An agent's proposed admission.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Admit {
    pub name: String,
    pub purpose: String,
    pub model: Option<String>,
    pub character: String,
    pub roads: Vec<String>,
}

/// The departure requested by the lifecycle owner.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Departure {
    Removed,
    Merged {
        into: u64,
        purpose: Option<String>,
        notes: MergeNotes,
        files: MergeFiles,
    },
}

/// The record's transactional operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Change {
    Admit(Admit),
    Depart {
        name: String,
        how: Departure,
    },
    Restore {
        archive: String,
        name: String,
    },
    MoveMember {
        number: u64,
        to: String,
    },
    LayRoad {
        a: String,
        b: String,
    },
    TakeUpRoad {
        a: String,
        b: String,
    },
    CarryRoad {
        to: u64,
    },
    Seat {
        number: u64,
    },
    /// The completed file work of a merge or split.
    FinishMerge {
        archive: String,
    },
    SetCharacter {
        number: u64,
        text: String,
    },
    SetLimits(Limits),
    /// Integration with votes and instructions; these are not agent
    /// tools.
    SetGuards {
        open_votes: Vec<OpenVote>,
        protected: Vec<Protection>,
    },
}

impl Change {
    fn name(&self) -> Action {
        match self {
            Self::Admit(_) => Action::Admit,
            Self::Depart {
                how: Departure::Removed,
                ..
            } => Action::Depart,
            Self::Depart {
                how: Departure::Merged { .. },
                ..
            } => Action::Merge,
            Self::Restore { .. } => Action::Restore,
            Self::MoveMember { .. } => Action::Move,
            Self::LayRoad { .. } => Action::LayRoad,
            Self::TakeUpRoad { .. } => Action::TakeUpRoad,
            Self::CarryRoad { .. } => Action::CarryRoad,
            Self::Seat { .. } => Action::Seat,
            Self::FinishMerge { .. } => Action::FinishMerge,
            Self::SetCharacter { .. } => Action::Character,
            Self::SetLimits(_) => Action::Limits,
            Self::SetGuards { .. } => Action::Guards,
        }
    }
}

/// An optional admission road that was refused without undoing
/// admission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoadRefusal {
    pub peer: String,
    pub refusal: Refusal,
}

/// The result kept in a record operation's marker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordResult {
    Admitted {
        agent: AgentRef,
        road_refusals: Vec<RoadRefusal>,
    },
    Departed {
        archive: String,
    },
    Restored {
        agent: Agent,
        text: String,
    },
    Moved(AgentRef),
    Road(Road),
    Changed,
}

impl Store {
    /// A hamlet's complete record, detached from the database.
    pub fn record(&self, hamlet: &str) -> Result<Option<Hamlet>> {
        load(&self.db, hamlet)
    }

    /// The current location of a living realm-wide name.
    pub fn agent_named(
        &self,
        name: &str,
    ) -> Result<Option<AgentRef>> {
        Ok(self
            .db
            .query_row(
                "SELECT hamlet, number FROM record_name WHERE name \
                 = ?1",
                [name],
                |row| {
                    Ok(AgentRef {
                        hamlet: row.get(0)?,
                        number: unsigned(row, 1)?,
                    })
                },
            )
            .optional()?)
    }

    /// All committed road charges for this hamlet, in charge order.
    pub fn road_charges(
        &self,
        hamlet: &str,
    ) -> Result<Vec<RoadCharge>> {
        let mut query = self.db.prepare(
            "SELECT agent, task, at, tokens, micros, ms FROM \
             record_road_charge WHERE hamlet = ?1 ORDER BY id",
        )?;
        Ok(query
            .query_map([hamlet], |row| {
                Ok(RoadCharge {
                    agent: unsigned(row, 0)?,
                    task: row.get(1)?,
                    at: row.get(2)?,
                    amount: Amount {
                        tokens: unsigned(row, 3)?,
                        micros: unsigned(row, 4)?,
                        ms: unsigned(row, 5)?,
                    },
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn load(db: &Connection, hamlet: &str) -> Result<Option<Hamlet>> {
    db.query_row(
        "SELECT document FROM record_hamlet WHERE id = ?1",
        [hamlet],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(|value| {
        serde_json::from_str(&value)
            .context("reading a hamlet record")
    })
    .transpose()
}

pub(crate) fn save(db: &Connection, record: &Hamlet) -> Result<()> {
    db.execute(
        "INSERT INTO record_hamlet(id, document) VALUES (?1, ?2) ON \
         CONFLICT(id) DO UPDATE SET document = excluded.document",
        params![record.id, serde_json::to_string(record)?],
    )?;
    db.execute(
        "DELETE FROM record_name WHERE hamlet = ?1",
        [&record.id],
    )?;
    for agent in &record.agents {
        db.execute(
            "INSERT INTO record_name(name, hamlet, number) VALUES \
             (?1, ?2, ?3)",
            params![
                agent.name,
                record.id,
                i64::try_from(agent.number)?
            ],
        )?;
    }
    Ok(())
}

fn unsigned(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<u64> {
    let value = row.get::<_, i64>(index)?;
    u64::try_from(value).map_err(|_| {
        rusqlite::Error::IntegralValueOutOfRange(index, value)
    })
}

impl Hamlet {
    /// The current office holder must still be a living member.
    pub fn require_mayor(&self, place: &AgentRef) -> Option<Refusal> {
        (!(place.hamlet == self.id
            && place.number == self.mayor
            && self.agents.iter().any(|a| a.number == place.number)))
        .then(|| {
            Refusal::new(Action::Mayor, &self.id, Reason::NotMayor)
        })
    }

    /// A current open vote holds mayor decisions until its current
    /// time limit expires.
    pub fn may_decide(
        &self,
        now: i64,
        operation: Action,
    ) -> Option<Refusal> {
        self.open_votes
            .iter()
            .find(|vote| {
                (now.saturating_sub(vote.called).max(0) as u64)
                    < self.limits.vote_open_ms
            })
            .map(|vote| {
                Refusal::new(
                    operation,
                    &self.id,
                    Reason::VoteOpen {
                        vote: vote.id,
                        motion: vote.motion.clone(),
                    },
                )
            })
    }

    fn chief(&self, chief: &str) -> bool {
        self.chiefs.is_empty()
            || self.chiefs.iter().any(|c| c == chief)
    }
}

impl RecordOperation {
    fn refuse(&self, reason: Reason) -> Refusal {
        Refusal::new(self.change.name(), &self.hamlet, reason)
    }

    fn decision(
        &self,
        record: &mut Hamlet,
        kind: DecisionKind,
        trim: bool,
    ) {
        record.decisions.push(Decision {
            at: self.at,
            by: self.actor.by(),
            reason: if trim {
                names::trim(&self.reason).into()
            } else {
                self.reason.clone()
            },
            kind,
        });
    }
}

impl Operation for RecordOperation {
    const KIND: &'static str = "record";
    type Done = RecordResult;
    type Refusal = Refusal;

    fn authorize(&self, db: &Connection) -> Result<Option<Refusal>> {
        let Some(record) = load(db, &self.hamlet)? else {
            return Ok(Some(self.refuse(Reason::HamletMissing)));
        };
        if matches!(self.change, Change::CarryRoad { .. }) {
            return Ok(match &self.actor {
                Actor::Member(place)
                    if place.hamlet == self.hamlet
                        && record
                            .agents
                            .iter()
                            .any(|a| a.number == place.number) =>
                {
                    None
                }
                _ => Some(self.refuse(Reason::Nonmember)),
            });
        }
        let chief_only = matches!(
            self.change,
            Change::Restore { .. }
                | Change::Seat { .. }
                | Change::TakeUpRoad { .. }
                | Change::FinishMerge { .. }
                | Change::SetCharacter { .. }
                | Change::SetLimits(_)
                | Change::SetGuards { .. }
        );
        let refused = match &self.actor {
            Actor::Member(_) => Some(self.refuse(Reason::NotMayor)),
            Actor::Chief(chief) => {
                (!record.chief(chief)).then(|| {
                    self.refuse(
                        if matches!(self.change, Change::Seat { .. })
                        {
                            Reason::MayorChief
                        } else {
                            Reason::RecordChief
                        },
                    )
                })
            }
            Actor::Mayor(place) => {
                if chief_only {
                    Some(self.refuse(
                        if matches!(self.change, Change::Seat { .. })
                        {
                            Reason::MayorChief
                        } else {
                            Reason::RecordChief
                        },
                    ))
                } else if matches!(
                    self.change,
                    Change::MoveMember { .. }
                ) {
                    Some(self.refuse(Reason::NotMayor))
                } else {
                    record
                        .require_mayor(place)
                        .map(|_| self.refuse(Reason::NotMayor))
                        .or_else(|| {
                            record.may_decide(
                                self.at,
                                self.change.name(),
                            )
                        })
                }
            }
            Actor::Mayors { leaving, .. } => {
                if matches!(self.change, Change::MoveMember { .. }) {
                    record.require_mayor(leaving).or_else(|| {
                        record.may_decide(self.at, Action::Move)
                    })
                } else {
                    Some(self.refuse(Reason::NotMayor))
                }
            }
        };
        if refused.is_some() {
            return Ok(refused);
        }
        if let Change::MoveMember { to, .. } = &self.change {
            let Some(target) = load(db, to)? else {
                return Ok(Some(Refusal::new(
                    Action::Move,
                    to,
                    Reason::HamletMissing,
                )));
            };
            return Ok(match &self.actor {
                Actor::Chief(chief) => {
                    (!target.chief(chief)).then(|| {
                        Refusal::new(
                            Action::Move,
                            to,
                            Reason::RecordChief,
                        )
                    })
                }
                Actor::Mayors { joining, .. } => {
                    target.require_mayor(joining).or_else(|| {
                        target.may_decide(self.at, Action::Move)
                    })
                }
                Actor::Mayor(_) | Actor::Member(_) => {
                    Some(self.refuse(Reason::NotMayor))
                }
            });
        }
        Ok(None)
    }

    fn apply(
        &self,
        db: &Connection,
    ) -> Result<Outcome<RecordResult, Refusal>> {
        let mut record = load(db, &self.hamlet)?
            .context("authorized hamlet missing")?;
        let result = match &self.change {
            Change::Admit(given) => {
                self.admit(db, &mut record, given)?
            }
            Change::Depart { name, how } => {
                self.depart(&mut record, name, how)
            }
            Change::Restore { archive, name } => {
                self.restore(db, &mut record, archive, name)?
            }
            Change::MoveMember { number, to } => {
                self.move_member(db, &mut record, *number, to)?
            }
            Change::LayRoad { a, b } => self
                .lay_road(db, &mut record, a, b)?
                .map(RecordResult::Road),
            Change::TakeUpRoad { a, b } => self
                .take_up_road(&mut record, a, b)
                .map(RecordResult::Road),
            Change::CarryRoad { to } => {
                let Actor::Member(place) = &self.actor else {
                    anyhow::bail!("authorized road sender missing");
                };
                self.carry_road(&mut record, place.number, *to)
                    .map(RecordResult::Road)
            }
            Change::Seat { number } => {
                self.seat(&mut record, *number)
            }
            Change::FinishMerge { archive } => match record
                .archive
                .iter_mut()
                .find(|a| a.id == *archive)
                .and_then(|a| a.merge.as_mut())
            {
                Some(merge) => {
                    merge.applied = true;
                    Ok(RecordResult::Changed)
                }
                None => Err(self.refuse(Reason::ArchiveMissing {
                    archive: archive.clone(),
                })),
            },
            Change::SetCharacter { number, text } => {
                match record
                    .agents
                    .iter_mut()
                    .find(|a| a.number == *number)
                {
                    Some(agent) => {
                        agent.character = text.clone();
                        Ok(RecordResult::Changed)
                    }
                    None => Err(self.refuse(Reason::AgentMissing {
                        name: format!("#{number}"),
                        local: true,
                    })),
                }
            }
            Change::SetLimits(limits) => {
                if let Some(budget) = &limits.road_budget {
                    ensure!(
                        budget.end > budget.start,
                        "a road budget needs a nonempty time window"
                    );
                }
                record.limits = limits.clone();
                Ok(RecordResult::Changed)
            }
            Change::SetGuards {
                open_votes,
                protected,
            } => {
                record.open_votes = open_votes.clone();
                record.protected = protected.clone();
                Ok(RecordResult::Changed)
            }
        };
        match result {
            Ok(done) => {
                save(db, &record)?;
                Ok(Outcome::Done(done))
            }
            Err(refusal) => Ok(Outcome::Refused(refusal)),
        }
    }
}

/// A founding request from the authenticated home chief or realm
/// owner. Realm capacity, votes and house creation belong to that
/// owner.
#[derive(Clone, Debug, Serialize)]
pub struct FoundRecord {
    pub hamlet: String,
    pub name: String,
    pub purpose: String,
    pub chief: String,
    /// Server time at this attempt, excluded from the request
    /// identity.
    #[serde(skip)]
    pub at: i64,
}

impl Operation for FoundRecord {
    const KIND: &'static str = "found-record";
    type Done = AgentRef;
    type Refusal = Refusal;

    fn authorize(&self, db: &Connection) -> Result<Option<Refusal>> {
        // The first request is made by home setup after
        // authenticating its chief.
        Ok(load(db, "home")?
            .filter(|home| !home.chief(&self.chief))
            .map(|_| {
                Refusal::new(
                    Action::Found,
                    &self.hamlet,
                    Reason::RecordChief,
                )
            }))
    }

    fn apply(
        &self,
        db: &Connection,
    ) -> Result<Outcome<AgentRef, Refusal>> {
        let refuse = |reason| {
            Ok(Outcome::Refused(Refusal::new(
                Action::Found,
                &self.hamlet,
                reason,
            )))
        };
        if load(db, &self.hamlet)?.is_some() {
            return refuse(Reason::HamletUsed);
        }
        if let Some(reason) = check_name(&self.hamlet) {
            return refuse(reason);
        }
        let first = self.hamlet == "home" && self.name == "founder";
        if !first
            && let Some(reason) =
                names::available(db, &self.name, None)?
        {
            return refuse(reason);
        }
        let record = Hamlet {
            id: self.hamlet.clone(),
            chiefs: vec![self.chief.clone()],
            founded: self.at,
            agents: vec![Agent {
                number: 0,
                name: self.name.clone(),
                purpose: names::trim(&self.purpose).into(),
                since: self.at,
                model: None,
                character: String::new(),
            }],
            mayor: 0,
            next: 1,
            roads: Vec::new(),
            archive: Vec::new(),
            decisions: Vec::new(),
            limits: Limits::default(),
            open_votes: Vec::new(),
            protected: Vec::new(),
        };
        save(db, &record)?;
        Ok(Outcome::Done(AgentRef {
            hamlet: self.hamlet.clone(),
            number: 0,
        }))
    }
}
