//! A stand-in for the realm that the real agent of demo/agent_host.rs
//! calls: the actor `realm/local`, which keeps in its tables what the
//! agents ask of it.
//!
//! - `may_spend` is answered `done`, or `refused` while spending is
//!   switched off.
//! - A `charge` is kept once by its sender and call id.
//! - A society act, `Tool<T>{call, act}`, is carried out once by its
//!   sender and call id: the stand-in keeps it with the answer it
//!   made, and answers a repeat with that answer. A repeat whose act
//!   differs fails with `message.id_reused`.
//! - A `reply` is kept once by its question.
//! - Every call of each of these is counted, repeats included.
//!
//! For the workspace it answers `secrets` with one made-up secret,
//! and keeps each call that waits for a chief. For the program it
//! sets an agent's reach (`grant`) and answers a call that waits for
//! a chief (`decide`), under the names and shapes of demo/realm.rs,
//! so `demo realm reach` and `demo realm answer` reach it too; it
//! calls an agent's action as itself (`send`); it sets a switch; and
//! it reports what it keeps (`recorded`).
//!
//! With the switch `die_in_tell` on, the next new `tell` is kept,
//! then the stand-in kills the Node host and answers only once that
//! host has ended, so the agent never reads the answer. The switch is
//! then off.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hamlet_actor::action::encode_positional;
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{
    ActorConfig, ActorFactory, BindParam, ColumnValue,
};
use hamlet_host::{Caller, HostRef};
use hamlet_messages::Message;
use hamlet_messages::address::{self, Party};
use hamlet_messages::realm::budget::{
    Budget, Charge, MaySpend, Payer, SpendRefusal,
};
use hamlet_messages::realm::{
    Ask, Done, End, Found, FoundAnswer, HamletId, Hire, HireAnswer,
    LayRoad, NeedsChief, Reply, RequestRoad, Seat, SecretList,
    Secrets, SetTools, TakeUpRoad, Tell, Tool, ToolCall,
};
use hamlet_messages::sender::{id_reused, sender, wrong_caller};
use hamlet_messages::workspace::reach::SecretsVersion;
use hamlet_messages::workspace::{
    AgentNumber, AnswerTaken, ChiefAnswer, InForce, SetReach,
};
use sha2::{Digest, Sha256};

use crate::agent_host;

pub fn factory() -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<StandIn>(
        address::Realm::KIND,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    registry
        .into_factories()
        .remove(address::Realm::KIND)
        .expect("the registry holds the kind it registered")
}

struct StandIn;

const SCHEMA: &str =
    "
CREATE TABLE IF NOT EXISTS calls (action TEXT PRIMARY KEY, count \
     INTEGER NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS charges (agent INTEGER NOT NULL, call \
     TEXT NOT NULL, task TEXT NOT NULL, micros INTEGER NOT NULL, \
     took_ms INTEGER NOT NULL, PRIMARY KEY (agent, call)) STRICT;
CREATE TABLE IF NOT EXISTS acts (agent INTEGER NOT NULL, call TEXT \
     NOT NULL, action TEXT NOT NULL, digest BLOB NOT NULL, act TEXT \
     NOT NULL, answer BLOB NOT NULL, PRIMARY KEY (agent, call)) \
     STRICT;
CREATE TABLE IF NOT EXISTS replies (question INTEGER PRIMARY KEY, \
     agent INTEGER NOT NULL, text TEXT NOT NULL) STRICT;
CREATE TABLE IF NOT EXISTS waits (agent INTEGER NOT NULL, call TEXT \
     NOT NULL, needs INTEGER NOT NULL, subject TEXT NOT NULL) \
     STRICT;
CREATE TABLE IF NOT EXISTS switches (name TEXT PRIMARY KEY) STRICT;
";

#[derive(
    Clone, Copy, Debug, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Switch {
    RefuseSpending,
    /// The next new `tell` kills the Node host before it is answered.
    DieInTell,
}

impl Switch {
    fn name(self) -> &'static str {
        match self {
            Self::RefuseSpending => "refuse_spending",
            Self::DieInTell => "die_in_tell",
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Set {
    pub switch: Switch,
    pub on: bool,
}

impl Action for Set {
    type Output = ();
    const NAME: &'static str = "set";
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Grant(pub SetReach);

impl Action for Grant {
    type Output = InForce;
    const NAME: &'static str = "grant";
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Decide(ChiefAnswer);

impl Action for Decide {
    type Output = AnswerTaken;
    const NAME: &'static str = "decide";
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Send {
    pub to: AgentNumber,
    pub name: String,
    pub args: Vec<u8>,
}

impl Action for Send {
    type Output = ();
    const NAME: &'static str = "send";
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Recorded;

impl Action for Recorded {
    type Output = Records;
    const NAME: &'static str = "recorded";
}

#[derive(
    Debug,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct Records {
    /// How many calls of each action came, repeats included.
    pub calls: BTreeMap<String, u64>,
    /// How many society acts of each action it keeps, one per call
    /// id.
    pub acts: BTreeMap<String, u64>,
    /// How many charges it keeps of each agent, by its number.
    pub charges: BTreeMap<String, u64>,
    pub replies: u64,
    pub waits: u64,
}

const CALL_WITHIN: Duration = Duration::from_secs(10);

const DEATH_WITHIN: Duration = Duration::from_secs(10);

#[async_trait::async_trait]
impl Actor for StandIn {
    type Input = ();
    type Actions = (
        MaySpend,
        Charge,
        Reply,
        Tool<Found>,
        Tool<Hire>,
        Tool<End>,
        Tool<Seat>,
        Tool<Tell>,
        Tool<Ask>,
        Tool<LayRoad>,
        Tool<TakeUpRoad>,
        Tool<RequestRoad>,
        Tool<SetTools>,
        Secrets,
        NeedsChief,
        Grant,
        Decide,
        Send,
        Set,
        Recorded,
    );
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        ctx.sql().exec(SCHEMA).await?;
        Ok(Self)
    }
}

/// A society act the stand-in carries out: the answer it makes for
/// it, the `n`th act it keeps.
trait Granted: ToolCall {
    fn granted(&self, n: u32) -> Self::Answer;
}

macro_rules! done {
    ($($act:ty),+) => {$(
        impl Granted for $act {
            fn granted(&self, _: u32) -> Self::Answer {
                Done::Done
            }
        }
    )+};
}

done!(
    End,
    Seat,
    Tell,
    Ask,
    LayRoad,
    TakeUpRoad,
    RequestRoad,
    SetTools
);

fn hired(n: u32) -> AgentNumber {
    AgentNumber(100 + n)
}

impl Granted for Found {
    fn granted(&self, n: u32) -> Self::Answer {
        FoundAnswer::Founded {
            hamlet: HamletId(format!("hamlet-{n}")),
            mayor: hired(n),
        }
    }
}

impl Granted for Hire {
    fn granted(&self, n: u32) -> Self::Answer {
        HireAnswer::Hired { agent: hired(n) }
    }
}

impl<T: Granted + std::fmt::Debug> Handler<Tool<T>> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        tool: Tool<T>,
    ) -> Result<T::Answer> {
        let address::Agent(agent) = sender(&ctx, &tool)?;
        let action = <Tool<T> as Action>::NAME;
        count(&ctx, action).await?;
        let act = cbor(&tool.act)?;
        let digest = Sha256::new()
            .chain_update(action)
            .chain_update(&act)
            .finalize()
            .to_vec();
        let n = integer(
            &ctx.sql()
                .query("SELECT COUNT(*) FROM acts", None)
                .await?,
        )?;
        let kept = ctx
            .sql()
            .run(
                "INSERT INTO acts (agent, call, action, digest, \
                 act, answer) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON \
                 CONFLICT DO NOTHING",
                Some(vec![
                    BindParam::Integer(i64::from(agent.0)),
                    BindParam::Text(tool.call.0.clone()),
                    BindParam::Text(action.to_owned()),
                    BindParam::Blob(digest.clone()),
                    BindParam::Text(format!("{:?}", tool.act)),
                    BindParam::Blob(cbor(
                        &tool.act.granted(u32::try_from(n + 1)?),
                    )?),
                ]),
            )
            .await?
            .changes
            == 1;
        let row = ctx
            .sql()
            .query(
                "SELECT digest, answer FROM acts WHERE agent = ?1 \
                 AND call = ?2",
                Some(vec![
                    BindParam::Integer(i64::from(agent.0)),
                    BindParam::Text(tool.call.0.clone()),
                ]),
            )
            .await?;
        let [ColumnValue::Blob(first), ColumnValue::Blob(answer)] =
            row.rows.first().map(Vec::as_slice).unwrap_or_default()
        else {
            bail!(
                "the act {} of agent {} was not kept",
                tool.call.0,
                agent.0
            );
        };
        if *first != digest {
            return Err(id_reused(action, &tool.call));
        }
        if kept {
            tracing::info!(
                act = ?tool.act,
                "kept agent {}'s {action} {}",
                agent.0,
                tool.call.0
            );
            if action == Tell::TOOL.name()
                && take(&ctx, Switch::DieInTell).await?
            {
                let host = HostRef::of(ctx.inner())?;
                agent_host::kill_node_host(
                    host.folder(),
                    DEATH_WITHIN,
                )
                .await?;
            }
        }
        Ok(ciborium::from_reader(answer.as_slice())?)
    }
}

impl Handler<MaySpend> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: MaySpend,
    ) -> Result<Done<SpendRefusal>> {
        sender(&ctx, &asked)?;
        count(&ctx, MaySpend::NAME).await?;
        if !is_on(&ctx, Switch::RefuseSpending).await? {
            return Ok(Done::Done);
        }
        Ok(Done::Refused {
            why: SpendRefusal::Budget(Budget::OverCost {
                payer: Payer::Realm,
                spent_micros: SPENT_MICROS,
                budget_micros: SPENT_MICROS,
            }),
        })
    }
}

/// What a refused `may_spend` says the realm spent of its budget.
const SPENT_MICROS: u64 = 1_000_000;

impl Handler<Charge> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        charge: Charge,
    ) -> Result<()> {
        let address::Agent(agent) = sender(&ctx, &charge)?;
        count(&ctx, Charge::NAME).await?;
        ctx.sql()
            .run(
                "INSERT INTO charges (call, agent, task, micros, \
                 took_ms) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT \
                 DO NOTHING",
                Some(vec![
                    BindParam::Text(charge.call.0),
                    BindParam::Integer(i64::from(agent.0)),
                    BindParam::Text(charge.task.0),
                    BindParam::Integer(i64::from(charge.micros)),
                    BindParam::Integer(i64::from(charge.took_ms)),
                ]),
            )
            .await?;
        Ok(())
    }
}

impl Handler<Reply> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        reply: Reply,
    ) -> Result<<Reply as Action>::Output> {
        let address::Agent(agent) = sender(&ctx, &reply)?;
        count(&ctx, Reply::NAME).await?;
        tracing::info!(
            "agent {} replies to question {}: {}",
            agent.0,
            reply.question.0,
            reply.text
        );
        ctx.sql()
            .run(
                "INSERT INTO replies (question, agent, text) VALUES \
                 (?1, ?2, ?3) ON CONFLICT DO NOTHING",
                Some(vec![
                    BindParam::Integer(i64::from(reply.question.0)),
                    BindParam::Integer(i64::from(agent.0)),
                    BindParam::Text(reply.text),
                ]),
            )
            .await?;
        Ok(Done::Done)
    }
}

impl Handler<Secrets> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Secrets,
    ) -> Result<SecretList> {
        sender(&ctx, &asked)?;
        Ok(SecretList {
            version: SecretsVersion(1),
            values: vec!["demo-secret-value".to_owned()],
        })
    }
}

impl Handler<NeedsChief> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: NeedsChief,
    ) -> Result<()> {
        sender(&ctx, &asked)?;
        tracing::info!(
            subject = asked.subject,
            needs = ?asked.needs,
            "agent {}'s call {} waits for a chief",
            asked.agent.0,
            asked.call.0
        );
        ctx.sql()
            .run(
                "INSERT INTO waits (agent, call, needs, subject) \
                 VALUES (?1, ?2, ?3, ?4)",
                Some(vec![
                    BindParam::Integer(i64::from(asked.agent.0)),
                    BindParam::Text(asked.call.0),
                    BindParam::Integer(i64::try_from(
                        asked.needs.len(),
                    )?),
                    BindParam::Text(asked.subject),
                ]),
            )
            .await?;
        Ok(())
    }
}

impl Handler<Grant> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Grant(set): Grant,
    ) -> Result<InForce> {
        from_program(&ctx, Grant::NAME)?;
        call(&ctx, &address::Workspace, &set).await
    }
}

impl Handler<Decide> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Decide(answer): Decide,
    ) -> Result<AnswerTaken> {
        from_program(&ctx, Decide::NAME)?;
        call(&ctx, &address::Workspace, &answer).await
    }
}

impl Handler<Send> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        send: Send,
    ) -> Result<()> {
        from_program(&ctx, Send::NAME)?;
        HostRef::of(ctx.inner())?
            .call(
                &address::Agent(send.to).address()?,
                &send.name,
                send.args,
                CALL_WITHIN,
            )
            .await?;
        Ok(())
    }
}

impl Handler<Set> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        set: Set,
    ) -> Result<()> {
        from_program(&ctx, Set::NAME)?;
        let sql = if set.on {
            "INSERT INTO switches (name) VALUES (?1) ON CONFLICT DO \
             NOTHING"
        } else {
            "DELETE FROM switches WHERE name = ?1"
        };
        ctx.sql()
            .run(
                sql,
                Some(vec![BindParam::Text(
                    set.switch.name().to_owned(),
                )]),
            )
            .await?;
        Ok(())
    }
}

impl Handler<Recorded> for StandIn {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Recorded,
    ) -> Result<Records> {
        from_program(&ctx, Recorded::NAME)?;
        let sql = ctx.sql();
        let total = async |table: &str| -> Result<u64> {
            let counted = sql
                .query(format!("SELECT COUNT(*) FROM {table}"), None)
                .await?;
            Ok(u64::try_from(integer(&counted)?)?)
        };
        Ok(Records {
            calls: by_name(
                &sql.query("SELECT action, count FROM calls", None)
                    .await?,
            )?,
            acts: by_name(
                &sql.query(
                    "SELECT action, COUNT(*) FROM acts GROUP BY \
                     action",
                    None,
                )
                .await?,
            )?,
            charges: by_name(
                &sql.query(
                    "SELECT CAST(agent AS TEXT), COUNT(*) FROM \
                     charges GROUP BY agent",
                    None,
                )
                .await?,
            )?,
            replies: total("replies").await?,
            waits: total("waits").await?,
        })
    }
}

async fn count(ctx: &Ctx<StandIn>, action: &str) -> Result<()> {
    ctx.sql()
        .run(
            "INSERT INTO calls (action, count) VALUES (?1, 1) ON \
             CONFLICT (action) DO UPDATE SET count = count + 1",
            Some(vec![BindParam::Text(action.to_owned())]),
        )
        .await?;
    Ok(())
}

async fn is_on(ctx: &Ctx<StandIn>, switch: Switch) -> Result<bool> {
    let found = ctx
        .sql()
        .query(
            "SELECT COUNT(*) FROM switches WHERE name = ?1",
            Some(vec![BindParam::Text(switch.name().to_owned())]),
        )
        .await?;
    Ok(integer(&found)? > 0)
}

/// Turns `switch` off. Answers whether it was on.
async fn take(ctx: &Ctx<StandIn>, switch: Switch) -> Result<bool> {
    let taken = ctx
        .sql()
        .run(
            "DELETE FROM switches WHERE name = ?1",
            Some(vec![BindParam::Text(switch.name().to_owned())]),
        )
        .await?;
    Ok(taken.changes > 0)
}

fn integer(found: &hamlet_actor_core::QueryResult) -> Result<i64> {
    match found.rows.first().and_then(|row| row.first()) {
        Some(ColumnValue::Integer(n)) => Ok(*n),
        other => bail!("a count read {other:?}"),
    }
}

fn by_name(
    found: &hamlet_actor_core::QueryResult,
) -> Result<BTreeMap<String, u64>> {
    found
        .rows
        .iter()
        .map(|row| match row.as_slice() {
            [ColumnValue::Text(name), ColumnValue::Integer(n)] => {
                Ok((name.clone(), u64::try_from(*n)?))
            }
            other => bail!("a count by name read {other:?}"),
        })
        .collect()
}

fn cbor<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes)?;
    Ok(bytes)
}

/// Fails with [`wrong_caller`] unless the program, which the host
/// stamps with no caller, made the call `ctx` handles.
fn from_program(
    ctx: &Ctx<StandIn>,
    action: &'static str,
) -> Result<()> {
    match ctx
        .conn()
        .context("the call has no connection")?
        .params()?
    {
        None => Ok(()),
        caller => Err(wrong_caller(action, caller)),
    }
}

async fn call<M: Message>(
    ctx: &Ctx<StandIn>,
    to: &M::To,
    message: &M,
) -> Result<M::Output> {
    let answer = HostRef::of(ctx.inner())?
        .call(
            &to.address()?,
            M::NAME,
            encode_positional(message)?,
            CALL_WITHIN,
        )
        .await?;
    Ok(ciborium::from_reader(answer.as_slice())?)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use hamlet_actor_error::ActorError;
    use hamlet_host::{Address, Host, HostOptions};
    use hamlet_messages::bytes::Base64;
    use hamlet_messages::realm::Named;
    use hamlet_messages::workspace::{CallId, RealmTask};
    use hamlet_testing::Scratch;
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::realm::ask;

    /// Stands in for an agent: calls the realm as itself.
    struct Sender;

    #[derive(Serialize, Deserialize)]
    struct Forward {
        name: String,
        args: Base64,
    }

    impl Action for Forward {
        type Output = Vec<u8>;
        const NAME: &'static str = "forward";
    }

    #[async_trait::async_trait]
    impl Actor for Sender {
        type Input = ();
        type Actions = (Forward,);
        type Events = ();
        type ConnParams = Option<Caller>;
        type Action = ();

        async fn create(_: &Ctx<Self>) -> Result<Self> {
            Ok(Self)
        }
    }

    impl Handler<Forward> for Sender {
        async fn handle(
            self: Arc<Self>,
            ctx: Ctx<Self>,
            forward: Forward,
        ) -> Result<Vec<u8>> {
            HostRef::of(ctx.inner())?
                .call(
                    &address::Realm.address()?,
                    &forward.name,
                    forward.args.0,
                    CALL_WITHIN,
                )
                .await
        }
    }

    async fn open(dir: &Scratch) -> Host {
        let mut registry = Registry::new();
        registry.register_actor_with::<Sender>(
            address::Agent::KIND,
            ActorConfig::default(),
        );
        let kinds = HashMap::from([
            (address::Realm::KIND.to_owned(), factory()),
            (
                address::Agent::KIND.to_owned(),
                registry
                    .into_factories()
                    .remove(address::Agent::KIND)
                    .unwrap(),
            ),
        ]);
        let host =
            Host::open_with(dir, kinds, HostOptions::default())
                .unwrap();
        host.get_or_start(&address::Realm.address().unwrap())
            .await
            .unwrap();
        host
    }

    async fn from_agent<A: Action + Serialize>(
        host: &Host,
        n: u32,
        action: &A,
    ) -> Result<Vec<u8>> {
        let agent: Address =
            address::Agent(AgentNumber(n)).address()?;
        host.get_or_start(&agent).await?;
        let forward = Forward {
            name: A::NAME.to_owned(),
            args: Base64(encode_positional(action)?),
        };
        let reply = host
            .call(
                &agent,
                Forward::NAME,
                encode_positional(&forward)?,
                CALL_WITHIN,
            )
            .await?;
        Ok(ciborium::from_reader(reply.as_slice())?)
    }

    fn tell(call: &str, text: &str) -> Tool<Tell> {
        Tool {
            call: CallId(call.to_owned()),
            act: Tell {
                to: Named::Agent {
                    agent: AgentNumber(2),
                },
                text: text.to_owned(),
                task: None,
            },
        }
    }

    fn charge(call: &str) -> Charge {
        Charge {
            call: CallId(call.to_owned()),
            task: RealmTask("line-1".to_owned()),
            micros: 0,
            took_ms: 1,
        }
    }

    #[tokio::test]
    async fn an_act_is_kept_once_and_a_repeat_gets_its_answer() {
        let dir = Scratch::new("stand-in-realm-acts");
        let host = open(&dir).await;
        let first = from_agent(&host, 1, &tell("t12", "hello"))
            .await
            .unwrap();
        let again = from_agent(&host, 1, &tell("t12", "hello"))
            .await
            .unwrap();
        assert_eq!(again, first);
        let reused = from_agent(&host, 1, &tell("t12", "bye"))
            .await
            .unwrap_err();
        let reused = ActorError::extract(&reused);
        assert_eq!(
            (reused.group(), reused.code()),
            ("message", "id_reused")
        );
        // Each agent's call ids are its own.
        from_agent(&host, 3, &tell("t12", "hello")).await.unwrap();

        let records = ask(&host, &Recorded).await.unwrap();
        assert_eq!(records.calls.get("tell"), Some(&4));
        assert_eq!(records.acts.get("tell"), Some(&2));
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn a_charge_is_kept_once_and_spending_is_refused_when_set()
    {
        let dir = Scratch::new("stand-in-realm-charges");
        let host = open(&dir).await;
        for call in ["c1", "c1", "c2"] {
            from_agent(&host, 1, &charge(call)).await.unwrap();
        }
        from_agent(&host, 4, &charge("c1")).await.unwrap();
        let records = ask(&host, &Recorded).await.unwrap();
        assert_eq!(
            records.charges,
            BTreeMap::from([
                ("1".to_owned(), 2),
                ("4".to_owned(), 1)
            ])
        );

        let may_spend = MaySpend {
            task: RealmTask("line-1".to_owned()),
        };
        let answer = |bytes: Vec<u8>| -> Done<SpendRefusal> {
            ciborium::from_reader(bytes.as_slice()).unwrap()
        };
        let allowed = from_agent(&host, 1, &may_spend).await.unwrap();
        assert!(matches!(answer(allowed), Done::Done));
        ask(
            &host,
            &Set {
                switch: Switch::RefuseSpending,
                on: true,
            },
        )
        .await
        .unwrap();
        let refused = from_agent(&host, 1, &may_spend).await.unwrap();
        assert!(matches!(answer(refused), Done::Refused { .. }));
        host.shutdown().await.unwrap();
    }
}
