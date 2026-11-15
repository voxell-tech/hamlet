//! The `decision` table: the record of what was decided, by whom
//! and why (H17).

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::address::Agent;
use hamlet_messages::realm::{HamletId, Tier};
use hamlet_messages::workspace::AgentNumber;

use super::callers::Witness;
use super::change::{Writer, number};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum By {
    Chief,
    Agent(AgentNumber),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Entry {
    Founded {
        hamlet: HamletId,
        by: By,
    },
    #[expect(dead_code, reason = "U4 records it")]
    Hired {
        hire: AgentNumber,
        by: By,
        why: Option<String>,
    },
    #[expect(dead_code, reason = "U4 records it")]
    Ended {
        hire: AgentNumber,
        by: By,
        why: Option<String>,
    },
    #[expect(dead_code, reason = "U4 records it")]
    Seated {
        hire: AgentNumber,
        by: By,
    },
    #[expect(dead_code, reason = "U11 records it")]
    Road {
        a: AgentNumber,
        b: AgentNumber,
        by: By,
    },
    #[expect(dead_code, reason = "U11 records it")]
    Unroad {
        a: AgentNumber,
        b: AgentNumber,
        by: By,
    },
    #[expect(dead_code, reason = "U11 records it")]
    Tools {
        hire: AgentNumber,
        by: By,
    },
    #[expect(dead_code, reason = "U11 records it")]
    Tier {
        hamlet: HamletId,
        tier: Tier,
        by: By,
    },
    /// The limits of a hamlet, or of the realm when `hamlet` is
    /// `None`.
    Limits {
        hamlet: Option<HamletId>,
        by: By,
    },
}

pub(super) async fn write(
    writer: &Writer<'_>,
    entries: Vec<Entry>,
) -> Result<()> {
    for entry in entries {
        let (kind, hamlet, hire, by, why) = match entry {
            Entry::Founded { hamlet, by } => {
                ("founded", Some(hamlet), None, by, None)
            }
            Entry::Hired { hire, by, why } => {
                ("hired", None, Some(hire), by, why)
            }
            Entry::Ended { hire, by, why } => {
                ("ended", None, Some(hire), by, why)
            }
            Entry::Seated { hire, by } => {
                ("seated", None, Some(hire), by, None)
            }
            Entry::Road { .. } | Entry::Unroad { .. } => {
                todo!(
                    "the decision table has no column for a road's \
                     other end"
                )
            }
            Entry::Tools { hire, by } => {
                ("tools", None, Some(hire), by, None)
            }
            Entry::Tier { .. } => {
                todo!("the decision table has no column for a tier")
            }
            Entry::Limits { hamlet, by } => {
                ("limits", hamlet, None, by, None)
            }
        };
        writer
            .execute(
                "INSERT INTO decision (kind, hamlet, agent, by, \
                 why, at) VALUES (?1, COALESCE(?2, (SELECT hamlet \
                 FROM hire WHERE agent = ?3)), ?3, ?4, ?5, ?6)",
                vec![
                    BindParam::Text(kind.to_owned()),
                    hamlet.map_or(BindParam::Null, |hamlet| {
                        BindParam::Text(hamlet.0)
                    }),
                    hire.map_or(BindParam::Null, number),
                    BindParam::Text(by.name()),
                    why.map_or(BindParam::Null, BindParam::Text),
                    BindParam::Integer(writer.at().0),
                ],
            )
            .await?;
    }
    Ok(())
}

impl By {
    fn name(&self) -> String {
        match self {
            Self::Chief => "chief".to_owned(),
            Self::Agent(agent) => Agent(*agent).name(),
        }
    }
}
