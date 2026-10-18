//! Record refusals and their chief-facing and agent-facing words.

use serde::{Deserialize, Serialize};

/// The reader of a refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Audience {
    Chief,
    Agent,
}

/// The operation a refusal answers.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Found,
    Admit,
    Depart,
    Merge,
    Restore,
    Move,
    LayRoad,
    TakeUpRoad,
    CarryRoad,
    Seat,
    Mayor,
    FinishMerge,
    Character,
    Limits,
    Guards,
}

/// A refused record operation, kept in its marker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub operation: Action,
    pub hamlet: String,
    pub reason: Reason,
}

/// The reason a record cannot change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Reason {
    AgentMissing {
        name: String,
        local: bool,
    },
    AgentDeparted {
        farewell: String,
    },
    NameFormat,
    NameReserved {
        name: String,
    },
    NameTaken {
        name: String,
        archive: Option<String>,
    },
    HamletFull {
        most: usize,
    },
    HamletMissing,
    HamletUsed,
    NotMayor,
    Nonmember,
    RecordChief,
    MayorChief,
    ProposedMayorMissing,
    AlreadyMayor {
        name: String,
    },
    MayorUntouchable,
    PurposeEmpty,
    ReasonEmpty,
    ArchiveMissing {
        archive: String,
    },
    RoadSelf,
    OfficeRoad,
    RoadExists {
        a: String,
        b: String,
    },
    RoadMissing {
        a: String,
        b: String,
    },
    RoadsFull {
        most: usize,
    },
    RoadBudget {
        text: String,
    },
    VoteOpen {
        vote: u64,
        motion: String,
    },
    Protected {
        name: String,
        chief: String,
        text: String,
        done: Option<i64>,
    },
    MoveMayor,
    MoveAlreadyThere {
        name: String,
    },
    UnsplitMerge {
        name: String,
        absorbed: String,
        archive: String,
    },
    MergeSelf,
    MergedPurposeEmpty,
    ProposedKeptMissing,
    MergeUnfinished,
    KeptDeparted {
        kept: String,
        absorbed: String,
    },
    LaterMerge {
        later: String,
        kept: String,
        absorbed: String,
    },
}

impl Refusal {
    pub(super) fn new(
        operation: Action,
        hamlet: &str,
        reason: Reason,
    ) -> Self {
        Self {
            operation,
            hamlet: hamlet.into(),
            reason,
        }
    }

    /// The TypeScript record's audience-specific wording.
    pub fn text(&self, audience: Audience) -> String {
        match &self.reason {
            Reason::AgentMissing { name, local } => format!(
                "no agent named `{name}`{}",
                if *local { " in this hamlet" } else { "" }
            ),
            Reason::AgentDeparted { farewell } => farewell.clone(),
            Reason::Nonmember => {
                "You do not live in this hamlet.".into()
            }
            Reason::NameFormat => "a name is 1 to 24 lowercase \
                                   letters and hyphens, such as \
                                   `librarian`"
                .into(),
            Reason::NameReserved { name } => {
                format!("`{name}` is reserved")
            }
            Reason::NameTaken { name, archive } => match archive {
                Some(archive) => format!(
                    "an agent named `{name}` lives in the hamlet \
                     now. Restore this one under another name: \
                     /restore {archive} as <new-name>"
                ),
                None => {
                    format!("an agent named `{name}` already exists")
                }
            },
            Reason::HamletFull { most } => match self.operation {
                Action::Move => {
                    format!("the hamlet `{}` is full", self.hamlet)
                }
                Action::Restore => format!(
                    "the hamlet is full ({most} agents); remove one \
                     first, or raise mostAgents in the limits"
                ),
                _ => format!(
                    "the hamlet is full ({most} agents). Only the \
                     chief can raise mostAgents on the hamlet's \
                     Bylaws page"
                ),
            },
            Reason::HamletMissing => {
                format!("no hamlet named `{}`", self.hamlet)
            }
            Reason::HamletUsed => format!(
                "a hamlet named `{}` already exists or was used \
                 before",
                self.hamlet
            ),
            Reason::NotMayor => if self.operation == Action::Merge {
                "Only the mayor can merge agents."
            } else {
                "Only the mayor may do that."
            }
            .into(),
            Reason::RecordChief => match self.operation {
                Action::Merge => {
                    "Only a chief of this hamlet can merge agents."
                }
                Action::Restore => {
                    "Only a chief of this hamlet can restore agents."
                }
                _ => "Only a chief of this hamlet can do this.",
            }
            .into(),
            Reason::MayorChief => "Only a chief of this hamlet can \
                                   name its mayor."
                .into(),
            Reason::ProposedMayorMissing => {
                "The proposed mayor no longer lives in this hamlet."
                    .into()
            }
            Reason::AlreadyMayor { name } => {
                format!("`{name}` is the mayor already")
            }
            Reason::MayorUntouchable => {
                "the mayor cannot be removed or merged; replacing a \
                 mayor takes a chief or a vote"
                    .into()
            }
            Reason::PurposeEmpty => {
                "say what the agent is for in `purpose`".into()
            }
            Reason::ReasonEmpty => {
                "give a `reason`; it is kept in the record".into()
            }
            Reason::ArchiveMissing { archive } => format!(
                "nothing is archived under `{archive}`; /record \
                 lists what is"
            ),
            Reason::RoadSelf => {
                "a road needs two different agents".into()
            }
            Reason::OfficeRoad => {
                if self.operation == Action::TakeUpRoad {
                    "an agent's road to the mayor stays for as long \
                     as the agent does"
                } else {
                    "every agent already has a road to the mayor"
                }
                .into()
            }
            Reason::RoadExists { a, b } => {
                format!("`{a}` and `{b}` already have a road")
            }
            Reason::RoadMissing { a, b } => {
                format!("`{a}` and `{b}` have no road")
            }
            Reason::RoadsFull { most } => format!(
                "the hamlet has all the roads {} ({most}). Only the \
                 chief can raise mostRoads on the hamlet's Bylaws \
                 page",
                if audience == Audience::Chief {
                    "your limits allow"
                } else {
                    "the chief allows"
                }
            ),
            Reason::RoadBudget { text } => text.clone(),
            Reason::VoteOpen { vote, motion } => format!(
                "vote {vote} is open ({motion}); nothing else is \
                 decided in this hamlet until it closes. Try again \
                 then"
            ),
            Reason::Protected {
                name,
                chief,
                text,
                done,
            } => format!(
                "`{name}` has a task from the chief {chief} \
                 (\"{}\") that {}. No agent may be removed or \
                 merged to avoid a chief's task. Wait until the \
                 chief has had the answer, or ask the chief to do \
                 this.",
                text.chars().take(80).collect::<String>(),
                if done.is_none() {
                    "is not finished"
                } else {
                    "has only just ended"
                }
            ),
            Reason::MoveMayor => {
                "a mayor never leaves its hamlet".into()
            }
            Reason::MoveAlreadyThere { name } => {
                format!("`{name}` already lives in `{}`", self.hamlet)
            }
            Reason::UnsplitMerge {
                name,
                absorbed,
                archive,
            } => format!(
                "Cannot move `{name}`: `{absorbed}` is still merged \
                 into it. Ask a chief to split or erase that \
                 archive first: /restore {archive} or /erase \
                 {archive}."
            ),
            Reason::MergeSelf => {
                "an agent cannot be merged with itself".into()
            }
            Reason::MergedPurposeEmpty => {
                "say what the merged agent is for in `purpose`".into()
            }
            Reason::ProposedKeptMissing => {
                "The kept agent has left the hamlet.".into()
            }
            Reason::MergeUnfinished => "The merge has not finished \
                                        yet; try again after it \
                                        finishes."
                .into(),
            Reason::KeptDeparted { kept, absorbed } => format!(
                "`{kept}`, which `{absorbed}` was merged into, has \
                 left the hamlet itself; restore that first"
            ),
            Reason::LaterMerge {
                later,
                kept,
                absorbed,
            } => format!(
                "`{later}` was merged into `{kept}` after \
                 `{absorbed}`; split that one first"
            ),
        }
    }
}
