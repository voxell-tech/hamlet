//! A stand-in for the realm: it sets agents' reach on the workspace
//! and keeps the workspace's acknowledgements, in its table `acks`.
//!
//! ```text
//! demo realm reach <n> <version> [project=<path>] [mode=manual|auto]
//!     [net] [worker | worker=<starter>]
//!     [trust=<conversation>:edits,commands] [secrets=<v>] [ended]
//!                    send agent <n> its reach; manual mode by default
//! demo realm acks    wait up to 10 s for each reach sent to be
//!                    acknowledged, then print the acknowledgements
//! ```
//!
//! `reach` prints `reach <n> <version> sent`, and `acks` prints
//! `ack <n> <sent> <in_force>` for each acknowledgement.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{
    ActorConfig, ActorFactory, BindParam, ColumnValue, SqliteDb,
    SqliteTransaction,
};
use hamlet_actors::workspace::reach::{
    Footing, Mode, Project, Reach, SecretsVersion, Trust,
};
use hamlet_actors::workspace::{
    self, AgentNumber, ConversationId, ReachApplied, ReachVersion,
    SetReach,
};
use hamlet_host::send::{self, Message};
use hamlet_host::{Address, Cause, Host};
use serde::{Deserialize, Serialize};

pub const KIND: &str = "realm";
pub const KEY: &str = "local";

/// How long `acks` waits for the workspace's acknowledgements.
const PATIENCE: Duration = Duration::from_secs(10);

/// The stub realm's kind, under `kind`.
pub fn factory(kind: &str) -> ActorFactory {
    let mut registry = Registry::new();
    registry.register_actor_with::<Realm>(
        kind,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    let layer = registry
        .into_factories()
        .remove(kind)
        .expect("the registry holds the kind it registered");
    send::receiving(layer, receive)
}

/// Sends the workspace `set`, from the realm at `at`.
pub async fn put(
    host: &Host,
    at: &Address,
    set: SetReach,
) -> Result<()> {
    call(host, at, &PutReach { set }).await
}

/// The reach the realm sent, the sends not yet answered, and the
/// acknowledgements it holds.
pub async fn standing(host: &Host, at: &Address) -> Result<Standing> {
    call(host, at, &Look).await
}

/// Runs `demo realm <words>`.
#[cfg_attr(test, expect(dead_code, reason = "the demo's command"))]
pub async fn run(host: &Host, words: &[String]) -> Result<()> {
    let at = Address::new(KIND, KEY);
    let mut out = std::io::stdout();
    match words {
        [verb, agent, version, rest @ ..] if verb == "reach" => {
            let set = SetReach {
                agent: AgentNumber(agent.parse()?),
                version: ReachVersion(version.parse()?),
                reach: parse_reach(rest)?,
            };
            put(host, &at, set).await?;
            writeln!(out, "reach {agent} {version} sent")?;
        }
        [verb] if verb == "acks" => {
            let begun = tokio::time::Instant::now();
            let mut now = standing(host, &at).await?;
            while now.acks.len() < now.sent as usize
                && begun.elapsed() < PATIENCE
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
                now = standing(host, &at).await?;
            }
            for ack in now.acks {
                writeln!(
                    out,
                    "ack {} {} {}",
                    ack.agent.0, ack.sent.0, ack.in_force.0
                )?;
            }
        }
        _ => bail!("usage: demo realm reach <n> <version> ...|acks"),
    }
    Ok(())
}

#[cfg_attr(test, expect(dead_code, reason = "the demo's command"))]
fn parse_reach(words: &[String]) -> Result<Reach> {
    let mut footing = Footing {
        project: None,
        network: false,
        trust: Vec::new(),
        secrets: SecretsVersion(1),
    };
    let mut mode = Mode::Manual;
    let mut worker = None;
    for word in words {
        match word.split_once('=') {
            None if word == "ended" => return Ok(Reach::Ended),
            None if word == "net" => footing.network = true,
            None if word == "worker" => worker = Some(None),
            Some(("project", folder)) => {
                footing.project = Some(Project {
                    folder: folder.into(),
                    mode,
                });
            }
            Some(("mode", "manual")) => mode = Mode::Manual,
            Some(("mode", "auto")) => mode = Mode::Auto,
            Some(("worker", starter)) => {
                worker = Some(Some(AgentNumber(starter.parse()?)));
            }
            Some(("secrets", version)) => {
                footing.secrets = SecretsVersion(version.parse()?);
            }
            Some(("trust", trust)) => {
                let (conversation, allows) =
                    trust.split_once(':').context(
                        "trust is <conversation>:edits,commands",
                    )?;
                footing.trust.push(Trust {
                    conversation: ConversationId(conversation.into()),
                    edits: allows.split(',').any(|it| it == "edits"),
                    commands: allows
                        .split(',')
                        .any(|it| it == "commands"),
                });
            }
            _ => bail!("`{word}` is not a part of a reach"),
        }
    }
    if let Some(project) = &mut footing.project {
        project.mode = mode;
    }
    Ok(match worker {
        Some(starter) => Reach::Worker { starter, footing },
        None => Reach::Agent(footing),
    })
}

async fn call<A: Action>(
    host: &Host,
    at: &Address,
    action: &A,
) -> Result<A::Output> {
    let reply = host
        .call(
            at,
            A::NAME,
            hamlet_actor::action::encode_positional(action)?,
            Duration::from_secs(10),
            Cause::root("demo"),
        )
        .await?;
    Ok(ciborium::from_reader(reply.as_slice())?)
}

/// What the realm sent and was told.
#[derive(Debug, Serialize, Deserialize)]
pub struct Standing {
    pub sent: u64,
    pub unanswered: u64,
    pub acks: Vec<ReachApplied>,
}

struct Realm;

#[derive(Serialize, Deserialize)]
struct PutReach {
    set: SetReach,
}

impl Action for PutReach {
    type Output = ();
    const NAME: &'static str = "put_reach";
}

#[derive(Serialize, Deserialize)]
struct Look;

impl Action for Look {
    type Output = Standing;
    const NAME: &'static str = "look";
}

#[async_trait::async_trait]
impl Actor for Realm {
    type Input = ();
    type Actions = (PutReach, Look);
    type Events = ();
    type ConnParams = ();
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> Result<Self> {
        ctx.sql()
            .exec(
                "CREATE TABLE IF NOT EXISTS sent (id INTEGER \
                 PRIMARY KEY, agent INTEGER NOT NULL, version \
                 INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS acks \
                 (id INTEGER PRIMARY KEY, agent INTEGER NOT NULL, \
                 sent INTEGER NOT NULL, in_force INTEGER NOT NULL)"
                    .to_owned(),
            )
            .await?;
        Ok(Self)
    }
}

impl Handler<PutReach> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        PutReach { set }: PutReach,
    ) -> Result<()> {
        let tx = ctx.sql().begin_transaction(None).await?;
        tx.execute(
            "INSERT INTO sent (agent, version) VALUES (?, ?)",
            Some(vec![
                integer(set.agent.0)?,
                integer(set.version.0)?,
            ]),
        )
        .await?;
        let mut payload = Vec::new();
        ciborium::into_writer(&set, &mut payload)?;
        send::send(
            &tx,
            &Address::new(workspace::KIND, workspace::MACHINE),
            SetReach::NAME,
            &payload,
            &Cause::root("demo"),
        )
        .await?;
        tx.commit().await
    }
}

impl Handler<Look> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Look,
    ) -> Result<Standing> {
        let db = ctx.sql();
        let acks = rows(
            db,
            "SELECT agent, sent, in_force FROM acks ORDER BY id",
        )
        .await?
        .into_iter()
        .map(|row| ReachApplied {
            agent: AgentNumber(row[0]),
            sent: ReachVersion(row[1]),
            in_force: ReachVersion(row[2]),
        })
        .collect();
        let count = async |sql: &str| -> Result<u64> {
            Ok(rows(db, sql).await?.first().map_or(0, |row| row[0]))
        };
        Ok(Standing {
            sent: count("SELECT COUNT(*) FROM sent").await?,
            unanswered: count("SELECT COUNT(*) FROM _hamlet_outbox")
                .await?,
            acks,
        })
    }
}

/// Keeps a [`ReachApplied`] the workspace sent.
async fn receive(
    tx: SqliteTransaction,
    message: Message,
) -> Result<Vec<u8>> {
    anyhow::ensure!(
        message.name == ReachApplied::NAME,
        "the realm takes no send `{}`",
        message.name
    );
    let ack: ReachApplied =
        ciborium::from_reader(message.payload.as_slice())?;
    tx.execute(
        "INSERT INTO acks (agent, sent, in_force) VALUES (?, ?, ?)",
        Some(vec![
            integer(ack.agent.0)?,
            integer(ack.sent.0)?,
            integer(ack.in_force.0)?,
        ]),
    )
    .await?;
    Ok(Vec::new())
}

fn integer(n: u64) -> Result<BindParam> {
    Ok(BindParam::Integer(i64::try_from(n)?))
}

/// The rows of `sql`, each a list of whole numbers.
async fn rows(db: &SqliteDb, sql: &str) -> Result<Vec<Vec<u64>>> {
    let read = db.execute(sql.to_owned(), None).await?;
    read.rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|value| match value {
                    ColumnValue::Integer(n) => Ok(u64::try_from(*n)?),
                    other => bail!("not a whole number: {other:?}"),
                })
                .collect()
        })
        .collect()
}
