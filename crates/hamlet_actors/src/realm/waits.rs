//! The waits for a chief: the `wait` table, `NeedsChief` and the
//! chief's `Answer` (H11, H34).

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use hamlet_actor::{BindParam, Ctx};
use hamlet_messages::address::{Agent, Workspace};
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::client::{Answer, AnswerRefusal};
use hamlet_messages::realm::models::TurnFailed;
use hamlet_messages::realm::oversight::Lapse;
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{Done, NeedsChief};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::{
    self, AgentNumber, CallId, ChiefAnswer,
};
use hamlet_protocol::client as protocol;
use serde::Serialize;

use super::act::{Act, Decided, Decision, Unbuilt};
use super::calls::{Calls, Delivery, Made};
use super::change::{self, Affected, Rows, Writer, number};
use super::peer::When;
use super::world::Part;
use super::{Realm, tree};

fn named(wait: &str) -> Option<WaitId> {
    let n = wait.trim_start_matches('#').parse::<i64>().ok()?;
    u64::try_from(n).ok().map(WaitId)
}

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
                change::integer(asked.reach_version.0)?,
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
                change::integer(wait.0)?,
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
            vec![change::integer(wait.0)?],
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

/// Delivers the notice that closed `wait` to its agent.
pub(super) async fn notice(
    _realm: &Arc<Realm>,
    _ctx: &Ctx<Realm>,
    wait: WaitId,
) -> Result<()> {
    bail!(
        "not built yet: U2, so {:?} is not made",
        Delivery::Notice(wait)
    )
}

/// The changes that close `payer`'s open budget faults when a chief
/// resets its budget.
pub(super) async fn budget_faults(
    _rows: &Rows<'_>,
    _payer: &Payer,
) -> Result<Vec<change::Change>> {
    Ok(Vec::new())
}

pub(super) enum Asker {
    Ended,
    NotEnded,
}

impl Act<Workspace> for NeedsChief {
    type Output = ();
    type Loaded = Asker;
    type Done = ();

    async fn load(
        &self,
        _by: &Workspace,
        rows: &Rows<'_>,
    ) -> Result<Asker> {
        Ok(match tree::left(rows, self.agent).await? {
            Some(_) => Asker::Ended,
            None => Asker::NotEnded,
        })
    }

    fn decide(
        &self,
        _by: &Workspace,
        asker: Asker,
    ) -> Decision<(), ()> {
        if let Asker::Ended = asker {
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
    type Loaded = Option<WaitId>;
    type Done = WaitId;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<WaitId>> {
        let Some(wait) = named(&self.0.wait) else {
            return Ok(None);
        };
        Ok(rows
            .value::<u64>(
                "SELECT id FROM wait WHERE id = ?1 AND closed_at IS \
                 NULL",
                vec![change::integer(wait.0)?],
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

impl Act<Agent> for TurnFailed {
    type Output = ();
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Agent,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U2")
    }

    fn decide(
        &self,
        _by: &Agent,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, ()> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) {
        match done {}
    }
}

impl Act<Agent> for Lapse {
    type Output = ();
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Agent,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U2")
    }

    fn decide(
        &self,
        _by: &Agent,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, ()> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) {
        match done {}
    }
}
