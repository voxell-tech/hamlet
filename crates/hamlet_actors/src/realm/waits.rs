//! The waits for a chief: the `wait` table, `NeedsChief` and the
//! chief's `Answer` (H11, H34).

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use hamlet_actor::{BindParam, Ctx};
use hamlet_messages::address::Workspace;
use hamlet_messages::realm::client::{Answer, AnswerRefusal};
use hamlet_messages::realm::{Done, NeedsChief};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::{
    self, AgentNumber, CallId, ChiefAnswer,
};
use hamlet_protocol::client as protocol;
use serde::{Deserialize, Serialize};

use super::act::{Act, Decided, Decision};
use super::calls::{Calls, Delivery, Made};
use super::change::{self, Affected, Rows, Writer, number};
use super::peer::When;
use super::world::Part;
use super::{Realm, tree};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub(super) struct WaitId(pub u64);

impl WaitId {
    /// The wait a client names: its number, as the client reads it,
    /// with or without a leading `#`.
    fn named(wait: &str) -> Option<Self> {
        wait.trim_start_matches('#').parse().ok().map(Self)
    }

    fn param(self) -> Result<BindParam> {
        Ok(BindParam::Integer(i64::try_from(self.0)?))
    }
}

/// How long an allow holds, as the row records it. The workspace
/// hears only allow or refuse, so every allow acts once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Scope {
    Once,
    Always,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Allow(Scope),
    Refuse { reason: Option<String> },
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// A wait opens, once per (agent, call).
    Open {
        asked: NeedsChief,
    },
    Close {
        wait: WaitId,
        choice: Choice,
    },
    /// Every open wait of an agent that ended closes, as lapsed.
    Lapse {
        agent: AgentNumber,
    },
}

/// A client sees each, as `Part::Waits`.
pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    let changed = match change {
        Change::Open { asked } => open(writer, asked).await?,
        Change::Close { wait, choice } => {
            close(writer, wait, choice).await?
        }
        Change::Lapse { agent } => lapse(writer, agent).await?,
    };
    Ok(if changed == 0 {
        Affected::default()
    } else {
        Affected::seen([Part::Waits])
    })
}

async fn open(writer: &Writer<'_>, asked: NeedsChief) -> Result<u64> {
    writer
        .execute(
            "INSERT INTO wait (agent, call, subject, needs, \
             reach_version, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON \
             CONFLICT (agent, call) DO NOTHING",
            vec![
                number(asked.agent),
                BindParam::Text(asked.call.0),
                BindParam::Text(asked.subject),
                BindParam::Text(serde_json::to_string(&asked.needs)?),
                BindParam::Integer(i64::try_from(
                    asked.reach_version.0,
                )?),
                BindParam::Integer(writer.at().0),
            ],
        )
        .await
}

async fn close(
    writer: &Writer<'_>,
    wait: WaitId,
    choice: Choice,
) -> Result<u64> {
    let (choice, scope, reason) = match choice {
        Choice::Allow(scope) => ("allow", Some(scope), None),
        Choice::Refuse { reason } => ("refuse", None, reason),
    };
    writer
        .execute(
            "UPDATE wait SET choice = ?1, scope = ?2, reason = ?3, \
             closed_at = ?4 WHERE id = ?5 AND closed_at IS NULL",
            vec![
                BindParam::Text(choice.to_owned()),
                match scope {
                    Some(scope) => {
                        BindParam::Text(change::case_name(&scope)?)
                    }
                    None => BindParam::Null,
                },
                reason.map_or(BindParam::Null, BindParam::Text),
                BindParam::Integer(writer.at().0),
                wait.param()?,
            ],
        )
        .await
}

async fn lapse(
    writer: &Writer<'_>,
    agent: AgentNumber,
) -> Result<u64> {
    writer
        .execute(
            "UPDATE wait SET choice = 'lapsed', closed_at = ?1 \
             WHERE agent = ?2 AND closed_at IS NULL",
            vec![BindParam::Integer(writer.at().0), number(agent)],
        )
        .await
}

/// Calls the workspace with the chief's answer to `wait`, from its
/// row: allow or refuse.
pub(super) async fn deliver(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    wait: WaitId,
) -> Result<()> {
    let (agent, call, choice, reason) = Rows::outside(ctx)?
        .row::<(u32, String, Option<String>, Option<String>)>(
            "SELECT agent, call, choice, reason FROM wait WHERE id \
             = ?1",
            vec![wait.param()?],
        )
        .await?
        .with_context(|| format!("wait {} has no row", wait.0))?;
    let answer = match choice.as_deref() {
        Some("allow") => workspace::Answer::Allow,
        Some("refuse") => workspace::Answer::Refuse { reason },
        other => bail!(
            "wait {} holds no chief's answer: {other:?}",
            wait.0
        ),
    };
    realm
        .peers
        .call(
            &Workspace,
            &ChiefAnswer {
                agent: AgentNumber(agent),
                call: CallId(call),
                answer,
            },
            When::AfterAnswer,
        )
        .await?;
    Ok(())
}

impl Act<Workspace> for NeedsChief {
    type Output = ();
    /// Whether the agent has ended.
    type Loaded = bool;
    type Done = ();

    async fn load(
        &self,
        _by: &Workspace,
        rows: &Rows<'_>,
    ) -> Result<bool> {
        Ok(tree::left(rows, self.agent).await?.is_some())
    }

    fn decide(
        &self,
        _by: &Workspace,
        ended: bool,
    ) -> Decision<(), ()> {
        // The end lapsed this agent's waits; the workspace closes its
        // own when it hears of the end.
        if ended {
            return Decision::Answer(());
        }
        Decision::Change(Decided::new(()).change(Change::Open {
            asked: self.clone(),
        }))
    }

    fn answer(&self, (): (), _made: &Made) {}
}

impl Act<Client> for Answer {
    type Output = Done<AnswerRefusal>;
    /// The open wait it names.
    type Loaded = Option<WaitId>;
    type Done = WaitId;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<WaitId>> {
        let Some(wait) = WaitId::named(&self.0.wait) else {
            return Ok(None);
        };
        Ok(rows
            .value::<u64>(
                "SELECT id FROM wait WHERE id = ?1 AND closed_at IS \
                 NULL",
                vec![wait.param()?],
            )
            .await?
            .map(WaitId))
    }

    fn decide(
        &self,
        _by: &Client,
        open: Option<WaitId>,
    ) -> Decision<WaitId, Self::Output> {
        let refuse = |why| Decision::Answer(Done::Refused { why });
        let Some(wait) = open else {
            return refuse(AnswerRefusal::NothingWaits {
                wait: self.0.wait.clone(),
            });
        };
        let choice = match (&self.0.choice, &self.0.picks) {
            (protocol::Choice::Allow, None) => {
                Choice::Allow(Scope::Once)
            }
            (protocol::Choice::Always, None) => {
                Choice::Allow(Scope::Always)
            }
            (protocol::Choice::Deny, None) => Choice::Refuse {
                reason: self
                    .0
                    .text
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned),
            },
            _ => return refuse(AnswerRefusal::NotOffered),
        };
        Decision::Change(
            Decided::new(wait).change(Change::Close { wait, choice }),
        )
    }

    fn deliveries(&self, _by: &Client, wait: &WaitId) -> Calls {
        Calls::after_answer(Delivery::ChiefAnswer(*wait))
    }

    fn answer(&self, _wait: WaitId, _made: &Made) -> Self::Output {
        Done::Done
    }
}
