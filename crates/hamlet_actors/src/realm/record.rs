//! The `decision` table: the record of what was decided, by whom
//! and why (H17).

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::address::Agent;
use hamlet_messages::realm::{HamletId, Tier};
use hamlet_messages::workspace::AgentNumber;

use super::callers::Witness;
use super::change::{self, Writer, number};

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
    Hired {
        hire: AgentNumber,
        by: By,
        why: Option<String>,
    },
    Ended {
        hire: AgentNumber,
        by: By,
        why: Option<String>,
    },
    Seated {
        hire: AgentNumber,
        by: By,
    },
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "U11 records it")
    )]
    Road {
        a: AgentNumber,
        b: AgentNumber,
        by: By,
    },
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "U11 records it")
    )]
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "U11 records it")
    )]
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
        let (kind, by, fields) = match entry {
            Entry::Founded { hamlet, by } => (
                "founded",
                by,
                Fields {
                    hamlet: Some(hamlet),
                    ..Fields::default()
                },
            ),
            Entry::Hired { hire, by, why } => {
                ("hired", by, Fields::of(hire, why))
            }
            Entry::Ended { hire, by, why } => {
                ("ended", by, Fields::of(hire, why))
            }
            Entry::Seated { hire, by } => {
                ("seated", by, Fields::of(hire, None))
            }
            Entry::Road { a, b, by } => {
                ("road", by, Fields::road(a, b))
            }
            Entry::Unroad { a, b, by } => {
                ("unroad", by, Fields::road(a, b))
            }
            Entry::Tools { hire, by } => {
                ("tools", by, Fields::of(hire, None))
            }
            Entry::Tier { hamlet, tier, by } => (
                "tier",
                by,
                Fields {
                    hamlet: Some(hamlet),
                    tier: Some(tier),
                    ..Fields::default()
                },
            ),
            Entry::Limits { hamlet, by } => (
                "limits",
                by,
                Fields {
                    hamlet,
                    ..Fields::default()
                },
            ),
        };
        writer
            .execute(
                "INSERT INTO decision (kind, hamlet, agent, other, \
                 tier, by, why, at) VALUES (?1, COALESCE(?2, \
                 (SELECT hamlet FROM hire WHERE agent = ?3)), ?3, \
                 ?4, ?5, ?6, ?7, ?8)",
                vec![
                    BindParam::Text(kind.to_owned()),
                    fields.hamlet.map_or(BindParam::Null, |hamlet| {
                        BindParam::Text(hamlet.0)
                    }),
                    fields.agent.map_or(BindParam::Null, number),
                    fields.other.map_or(BindParam::Null, number),
                    fields
                        .tier
                        .map(|tier| change::case_name(&tier))
                        .transpose()?
                        .map_or(BindParam::Null, BindParam::Text),
                    BindParam::Text(by.name()),
                    fields
                        .why
                        .map_or(BindParam::Null, BindParam::Text),
                    BindParam::Integer(writer.at().0),
                ],
            )
            .await?;
    }
    Ok(())
}

#[derive(Default)]
struct Fields {
    hamlet: Option<HamletId>,
    agent: Option<AgentNumber>,
    other: Option<AgentNumber>,
    tier: Option<Tier>,
    why: Option<String>,
}

impl Fields {
    fn of(agent: AgentNumber, why: Option<String>) -> Self {
        Self {
            agent: Some(agent),
            why,
            ..Self::default()
        }
    }

    fn road(a: AgentNumber, b: AgentNumber) -> Self {
        Self {
            agent: Some(a),
            other: Some(b),
            ..Self::default()
        }
    }
}

impl By {
    fn name(&self) -> String {
        match self {
            Self::Chief => "chief".to_owned(),
            Self::Agent(agent) => Agent(*agent).name(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::change::{Transaction, Write};
    use super::super::testing::Started;
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn a_road_keeps_both_ends_and_a_tier_its_name() {
        let started = Started::new().await;
        let hamlet = HamletId("h".to_owned());
        started.found(&hamlet, AgentNumber(2)).await;
        let (a, b) = (AgentNumber(2), AgentNumber(3));
        let entries = vec![
            Entry::Road {
                a,
                b,
                by: By::Chief,
            },
            Entry::Unroad {
                a,
                b,
                by: By::Agent(a),
            },
            Entry::Tier {
                hamlet: hamlet.clone(),
                tier: Tier::Hundred,
                by: By::Chief,
            },
        ];
        Transaction::begin(&started.ctx)
            .await
            .unwrap()
            .commit(
                &started.realm,
                Write::<()> {
                    changes: Vec::new(),
                    entries,
                    marker: None,
                },
            )
            .await
            .unwrap();

        let rows = started.rows();
        let read = rows
            .rows::<(String, Option<u32>, Option<u32>, Option<String>)>(
                "SELECT kind, agent, other, tier FROM decision ORDER BY id",
                Vec::new(),
            )
            .await
            .unwrap();
        let road =
            |kind: &str| (kind.to_owned(), Some(2), Some(3), None);
        assert_eq!(
            read,
            [
                road("road"),
                road("unroad"),
                (
                    "tier".to_owned(),
                    None,
                    None,
                    Some("hundred".to_owned())
                ),
            ]
        );
        let hamlets = rows
            .values::<String>(
                "SELECT hamlet FROM decision",
                Vec::new(),
            )
            .await
            .unwrap();
        assert_eq!(hamlets, ["h", "h", "h"]);
        started.stop().await;
    }
}
