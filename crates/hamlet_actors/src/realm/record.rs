//! The `decision` table: the record of what was decided, by whom
//! and why (H17), and its reader.

use anyhow::{Context as _, Result, bail};
use hamlet_actor::BindParam;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::hamlets::{
    ReadRecord, Record, RecordLine,
};
use hamlet_messages::realm::{
    AgentName, HamletId, HamletRefusal, Refusable, Tier,
};
use hamlet_messages::workspace::AgentNumber;
use hamlet_sandbox::scrub::scrub_text;

use super::act::{Act, Decision, Reading, Unchanged};
use super::callers::{RealmWide, Witness};
use super::calls::Made;
use super::change::{self, Rows, Writer, number};
use super::chiefs;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum By {
    Chief,
    Agent(AgentNumber),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Entry {
    Founded {
        hamlet: HamletId,
        mayor: AgentNumber,
        by: By,
        why: Option<String>,
    },
    /// A hire made, kept with no reason, as today
    /// (`crates/hamlet_server/src/society/hires.rs:144`).
    Hired {
        hire: AgentNumber,
        by: By,
    },
    Ended {
        hire: AgentNumber,
        by: By,
        why: Option<String>,
    },
    Seated {
        hire: AgentNumber,
        by: By,
        why: Option<String>,
    },
    Road {
        a: AgentNumber,
        b: AgentNumber,
        by: By,
        why: Option<String>,
    },
    Unroad {
        a: AgentNumber,
        b: AgentNumber,
        by: By,
        why: Option<String>,
    },
    /// A road between hamlets that both mayors agreed, as the second
    /// agreed it, kept in `hamlet`.
    RoadAgreed {
        hamlet: HamletId,
        a: AgentNumber,
        b: AgentNumber,
        by: By,
        why: Option<String>,
    },
    Tools {
        hire: AgentNumber,
        by: By,
        why: Option<String>,
    },
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
    NamedHamlet {
        hamlet: HamletId,
        by: By,
    },
    PlacedHamlet {
        hamlet: HamletId,
        by: By,
    },
    Folder {
        hamlet: HamletId,
        by: By,
    },
    Mode {
        hamlet: HamletId,
        by: By,
    },
    Nicknamed {
        agent: AgentNumber,
        by: By,
        why: Option<String>,
    },
    Model {
        agent: AgentNumber,
        by: By,
        why: Option<String>,
    },
    Allowance {
        hire: AgentNumber,
        by: By,
        why: Option<String>,
    },
}

pub(super) async fn write(
    writer: &Writer<'_>,
    entries: Vec<Entry>,
) -> Result<()> {
    for entry in entries {
        let (kind, by, fields) = match entry {
            Entry::Founded {
                hamlet,
                mayor,
                by,
                why,
            } => (
                "founded",
                by,
                Fields {
                    hamlet: Some(hamlet),
                    ..Fields::of(mayor, why)
                },
            ),
            Entry::Hired { hire, by } => {
                ("hired", by, Fields::of(hire, None))
            }
            Entry::Ended { hire, by, why } => {
                ("ended", by, Fields::of(hire, why))
            }
            Entry::Seated { hire, by, why } => {
                ("seated", by, Fields::of(hire, why))
            }
            Entry::Road { a, b, by, why } => {
                ("road", by, Fields::road(a, b, why))
            }
            Entry::Unroad { a, b, by, why } => {
                ("unroad", by, Fields::road(a, b, why))
            }
            Entry::RoadAgreed {
                hamlet,
                a,
                b,
                by,
                why,
            } => (
                "road-agreed",
                by,
                Fields {
                    hamlet: Some(hamlet),
                    ..Fields::road(a, b, why)
                },
            ),
            Entry::Tools { hire, by, why } => {
                ("tools", by, Fields::of(hire, why))
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
            Entry::NamedHamlet { hamlet, by } => {
                ("named-hamlet", by, Fields::hamlet(hamlet))
            }
            Entry::PlacedHamlet { hamlet, by } => {
                ("placed-hamlet", by, Fields::hamlet(hamlet))
            }
            Entry::Folder { hamlet, by } => {
                ("folder", by, Fields::hamlet(hamlet))
            }
            Entry::Mode { hamlet, by } => {
                ("mode", by, Fields::hamlet(hamlet))
            }
            Entry::Nicknamed { agent, by, why } => {
                ("nicknamed", by, Fields::of(agent, why))
            }
            Entry::Model { agent, by, why } => {
                ("model", by, Fields::of(agent, why))
            }
            Entry::Allowance { hire, by, why } => {
                ("allowance", by, Fields::of(hire, why))
            }
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
                    fields.why.map_or(BindParam::Null, |why| {
                        BindParam::Text(writer.scrubbed(&why))
                    }),
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

    fn hamlet(hamlet: HamletId) -> Self {
        Self {
            hamlet: Some(hamlet),
            ..Self::default()
        }
    }

    fn road(
        a: AgentNumber,
        b: AgentNumber,
        why: Option<String>,
    ) -> Self {
        Self {
            other: Some(b),
            ..Self::of(a, why)
        }
    }
}

impl By {
    /// The name the `decision` and `model_choice` tables keep.
    pub fn name(&self) -> String {
        match self {
            Self::Chief => CHIEF.to_owned(),
            Self::Agent(agent) => Agent(*agent).name(),
        }
    }
}

const CHIEF: &str = "chief";

impl<By: RealmWide> Act<By> for Reading<ReadRecord> {
    type Output = Record;
    type Loaded = Record;
    type Done = Unchanged;

    async fn load(
        &self,
        _by: &By,
        rows: &Rows<'_>,
    ) -> Result<Record> {
        let hamlet = self.read.hamlet.as_ref();
        if let Some(hamlet) = hamlet
            && chiefs::hamlet(rows, hamlet).await?.is_none()
        {
            return Ok(Record::refused(
                HamletRefusal::NoSuchHamlet { id: hamlet.clone() },
            ));
        }
        Ok(Record::Read {
            lines: lines(rows, hamlet, &self.secrets).await?,
        })
    }

    fn decide(
        &self,
        _by: &By,
        record: Record,
    ) -> Decision<Unchanged, Record> {
        Decision::Answer(record)
    }

    fn answer(&self, done: Unchanged, _made: &Made) -> Record {
        match done {}
    }
}

/// The decisions of `hamlet`, or the realm's own, oldest first, in
/// words with `secrets` removed, or "Nothing is decided yet." when
/// there are none.
async fn lines(
    rows: &Rows<'_>,
    hamlet: Option<&HamletId>,
    secrets: &[String],
) -> Result<Vec<RecordLine>> {
    let kept = rows
        .rows::<(String, Option<u32>, String, Option<String>)>(
            "SELECT kind, agent, by, why FROM decision WHERE hamlet \
             IS ?1 ORDER BY id",
            vec![hamlet.map_or(BindParam::Null, |hamlet| {
                BindParam::Text(hamlet.0.clone())
            })],
        )
        .await?;
    let mut lines = Vec::new();
    for (kind, agent, by, why) in kept {
        let who = match by.as_str() {
            CHIEF => CHIEF.to_owned(),
            agent => agent_name(rows, numbered(agent)?).await?.name,
        };
        let subject = match agent {
            Some(agent) => {
                agent_name(rows, AgentNumber(agent)).await?.name
            }
            None => String::new(),
        };
        let why = why
            .filter(|why| !why.is_empty())
            .map_or_else(String::new, |why| {
                format!(": {}", scrub_text(secrets, &why))
            });
        lines.push(RecordLine {
            text: format!("- {who} {} {subject}{why}", done(&kind)),
        });
    }
    if lines.is_empty() {
        lines.push(RecordLine {
            text: "Nothing is decided yet.".to_owned(),
        });
    }
    Ok(lines)
}

fn numbered(by: &str) -> Result<AgentNumber> {
    match by.strip_prefix("agent/").map(str::parse) {
        Some(Ok(agent)) => Ok(AgentNumber(agent)),
        _ => bail!("a decision by `{by}`"),
    }
}

/// `agent` as chiefs and agents are shown it: its address, and beside
/// it a hire's nickname or else its profile's name, or a worker's
/// nickname, as in `hire#3 (Mira)`.
pub(super) async fn agent_name(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<AgentName> {
    let (kind, nickname, profile) = rows
        .row::<(String, Option<String>, Option<String>)>(
            "SELECT agent.kind, nickname.name, profile.name FROM \
             agent LEFT JOIN nickname ON nickname.agent = agent.id \
             LEFT JOIN hire ON hire.agent = agent.id LEFT JOIN \
             profile ON profile.id = hire.profile WHERE agent.id = \
             ?1",
            vec![number(agent)],
        )
        .await?
        .with_context(|| format!("agent {} has no row", agent.0))?;
    let kind = change::from_case_name(kind)?;
    let address = address_of(agent, kind)?;
    let beside = match kind {
        AgentKind::Hire => nickname.or(profile),
        AgentKind::Worker => nickname,
        AgentKind::Hand | AgentKind::Errand => None,
    };
    let name = match beside {
        Some(beside) => format!("{address} ({beside})"),
        None => address,
    };
    Ok(AgentName { agent, name })
}

/// `agent`'s address, as in `hire#3`.
pub(super) async fn address(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<String> {
    address_of(agent, super::tree::kind_of(rows, agent).await?)
}

fn address_of(agent: AgentNumber, kind: AgentKind) -> Result<String> {
    let n = agent.0;
    Ok(match kind {
        AgentKind::Hand => "hand".to_owned(),
        AgentKind::Hire => format!("hire#{n}"),
        AgentKind::Worker => new_worker(agent).name,
        AgentKind::Errand => bail!("errand {n} has no address"),
    })
}

/// A hire just made of a profile named `profile`.
pub(super) fn new_hire(
    hire: AgentNumber,
    profile: &str,
) -> AgentName {
    AgentName {
        agent: hire,
        name: format!("hire#{} ({profile})", hire.0),
    }
}

/// `agent` shown by its address alone, as `hand` or `hire#3`.
#[cfg(test)]
pub(super) fn bare(agent: AgentNumber) -> AgentName {
    let name = if agent == hamlet_messages::sender::Hand::NUMBER {
        "hand".to_owned()
    } else {
        format!("hire#{}", agent.0)
    };
    AgentName { agent, name }
}

/// A worker just started, which no one has nicknamed yet.
pub(super) fn new_worker(worker: AgentNumber) -> AgentName {
    AgentName {
        agent: worker,
        name: format!("worker#{}", worker.0),
    }
}

/// What a decision of `kind` did, after who made it.
fn done(kind: &str) -> &'static str {
    match kind {
        "founded" => "founded the hamlet under",
        "hired" => "hired",
        "ended" => "ended",
        "seated" => "gave the office to",
        "model" => "chose the model of",
        "tools" => "changed the tools of",
        "allowance" => "set the allowance of",
        "road" => "laid a road from",
        "unroad" => "took up a road from",
        "road-agreed" => "agreed a road from",
        "nicknamed" => "nicknamed",
        _ => "changed",
    }
}

/// The decisions of `hamlet` made at or after `since`, in ms since
/// the Unix epoch, oldest first, in words with `secrets` removed: its
/// latest decisions, as many as were made since.
pub(super) async fn since(
    rows: &Rows<'_>,
    hamlet: &HamletId,
    since: i64,
    secrets: &[String],
) -> Result<Vec<String>> {
    let made = rows
        .value::<u64>(
            "SELECT COUNT(*) FROM decision WHERE hamlet = ?1 AND at \
             >= ?2",
            vec![
                BindParam::Text(hamlet.0.clone()),
                BindParam::Integer(since),
            ],
        )
        .await?
        .unwrap_or(0);
    let mut lines = lines(rows, Some(hamlet), secrets).await?;
    let made = usize::try_from(made)?.min(lines.len());
    Ok(lines
        .split_off(lines.len() - made)
        .into_iter()
        .map(|line| line.text)
        .collect())
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
                why: None,
            },
            Entry::Unroad {
                a,
                b,
                by: By::Agent(a),
                why: None,
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
