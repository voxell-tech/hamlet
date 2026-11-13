//! The workspace through a host, called by the actors that call it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory};
use hamlet_actors::workspace::reach::{
    Footing, Mode, Project, Reach, SecretsVersion,
};
use hamlet_actors::workspace::{
    self, AgentNumber, ReachAnswer, ReachRefusal, ReachVersion,
    SetReach,
};
use hamlet_host::{Address, Caller, Cause, Host, HostOptions};
use hamlet_sandbox::Sandbox;
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

const AGENT: AgentNumber = AgentNumber(7);

/// The kinds that call the workspace in these tests, each a
/// [`Relay`].
const CALLERS: [&str; 3] = ["realm", "agent", "other"];

/// Calls the workspace as itself, with the action and arguments it is
/// given.
struct Relay;

/// What a [`Relay`] calls the workspace with.
#[derive(Serialize, Deserialize)]
struct Forward {
    name: String,
    args: Vec<u8>,
}

impl Action for Forward {
    type Output = Vec<u8>;
    const NAME: &'static str = "forward";
}

#[async_trait::async_trait]
impl Actor for Relay {
    type Input = ();
    type Actions = (Forward,);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(_: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self)
    }
}

impl Handler<Forward> for Relay {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        forward: Forward,
    ) -> anyhow::Result<Vec<u8>> {
        Host::call_from(
            ctx.inner(),
            &workspace_address(),
            &forward.name,
            forward.args,
            Duration::from_secs(10),
            Cause::root("test"),
        )
        .await
    }
}

fn relay(kind: &str) -> ActorFactory {
    let mut registry = Registry::new();
    registry
        .register_actor_with::<Relay>(kind, ActorConfig::default());
    registry
        .into_factories()
        .remove(kind)
        .expect("the registry holds the kind it registered")
}

fn workspace_address() -> Address {
    Address::new(workspace::KIND, workspace::MACHINE)
}

/// A host on `dir` with the workspace and the callers' kinds, given
/// `options`.
fn open(dir: &Path, options: HostOptions) -> anyhow::Result<Host> {
    let mut kinds = HashMap::from([(
        workspace::KIND.to_owned(),
        workspace::factory(),
    )]);
    for kind in CALLERS {
        kinds.insert(kind.to_owned(), relay(kind));
    }
    Host::open_with(dir, kinds, options)
}

/// A host on `dir` with the workspace made, on the tests' sandbox.
async fn host(dir: &Path) -> Host {
    let host = open(
        dir,
        HostOptions::default()
            .with_value(Arc::new(Sandbox::testing())),
    )
    .unwrap();
    host.get_or_start(workspace::KIND, workspace::MACHINE)
        .await
        .unwrap();
    host
}

fn reach(project: Option<&str>) -> Reach {
    Reach::Agent(Footing {
        project: project.map(|folder| Project {
            folder: folder.into(),
            mode: Mode::Auto,
        }),
        network: false,
        trust: Vec::new(),
        secrets: SecretsVersion(1),
    })
}

/// The arguments of a [`SetReach`] of agent 7.
fn set_reach(version: u64, reach: Reach) -> Vec<u8> {
    let set = SetReach {
        agent: AGENT,
        version: ReachVersion(version),
        reach,
    };
    hamlet_actor::action::encode_positional(&set).unwrap()
}

/// Calls the workspace's `name` with `args` as the actor
/// `<kind>/caller`, made first if it has no file.
async fn call_as(
    host: &Host,
    kind: &str,
    name: &str,
    args: Vec<u8>,
) -> anyhow::Result<Vec<u8>> {
    host.get_or_start(kind, "caller").await?;
    let forward = Forward {
        name: name.to_owned(),
        args,
    };
    let reply = host
        .call(
            &Address::new(kind, "caller"),
            Forward::NAME,
            hamlet_actor::action::encode_positional(&forward)?,
            Duration::from_secs(20),
            Cause::root("test"),
        )
        .await?;
    ciborium::from_reader(reply.as_slice())
        .context("decode the relay's reply")
}

fn decoded<T: serde::de::DeserializeOwned>(reply: &[u8]) -> T {
    ciborium::from_reader(reply).unwrap()
}

/// Calls the workspace with a [`SetReach`] of agent 7, as the realm.
async fn realm_sets(
    host: &Host,
    version: u64,
    reach: Reach,
) -> ReachAnswer {
    let reply = call_as(
        host,
        "realm",
        SetReach::NAME,
        set_reach(version, reach),
    )
    .await
    .unwrap();
    decoded(&reply)
}

fn read_only(dir: &Path) -> Connection {
    Connection::open_with_flags(
        dir.join("actors/workspace/local.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
}

/// The workspace's reach rows, `(agent, version, body)`.
fn reach_rows(dir: &Path) -> Vec<(i64, i64, Reach)> {
    let db = read_only(dir);
    let mut read = db
        .prepare(
            "SELECT agent, version, body FROM reach ORDER BY agent",
        )
        .unwrap();
    read.query_map([], |row| {
        let body: Vec<u8> = row.get(2)?;
        Ok((
            row.get(0)?,
            row.get(1)?,
            ciborium::from_reader(body.as_slice()).unwrap(),
        ))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

/// The workspace's record of refused callers, `(caller, action)`.
fn refusals(dir: &Path) -> Vec<(String, String)> {
    let db = read_only(dir);
    let mut read = db
        .prepare("SELECT caller, action FROM refusals ORDER BY id")
        .unwrap();
    read.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Version 3, then 2, then 3 again with another body: version 3's
/// first body stays in force, and each call answers version 3.
#[tokio::test(flavor = "multi_thread")]
async fn an_older_reach_version_is_ignored() {
    let dir = Scratch::new("workspace-older-reach");
    let host = host(&dir).await;
    let mut answers = Vec::new();
    for (version, project) in
        [(3, None), (2, Some("/work/a")), (3, Some("/work/b"))]
    {
        answers
            .push(realm_sets(&host, version, reach(project)).await);
    }
    host.shutdown().await.unwrap();

    assert_eq!(
        answers,
        vec![ReachAnswer::InForce(ReachVersion(3)); 3]
    );
    assert_eq!(reach_rows(&dir), [(7, 3, reach(None))]);
}

/// W2: an agent, another kind, a client and a call with no caller
/// are each refused, recorded, and change no reach. Their version 5
/// would outrank the realm's version 1 had any of them applied.
#[tokio::test(flavor = "multi_thread")]
async fn only_the_realm_sets_a_reach() {
    let dir = Scratch::new("workspace-reach-not-realm");
    let host = host(&dir).await;
    let mut answers = Vec::new();
    for kind in ["agent", "other"] {
        let reply = call_as(
            &host,
            kind,
            SetReach::NAME,
            set_reach(5, reach(Some("/"))),
        )
        .await
        .unwrap();
        answers.push(decoded::<ReachAnswer>(&reply));
    }
    let to = workspace_address();
    let client = host.call_for_client(
        &to,
        SetReach::NAME,
        set_reach(5, reach(Some("/"))),
        Duration::from_secs(10),
        Cause::root("test"),
    );
    answers.push(decoded(&client.await.unwrap()));
    let none = host.call(
        &to,
        SetReach::NAME,
        set_reach(5, reach(Some("/"))),
        Duration::from_secs(10),
        Cause::root("test"),
    );
    answers.push(decoded(&none.await.unwrap()));
    let realm = realm_sets(&host, 1, reach(None)).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        answers,
        vec![ReachAnswer::Refused(ReachRefusal::NotTheRealm); 4]
    );
    assert_eq!(realm, ReachAnswer::InForce(ReachVersion(1)));
    assert_eq!(reach_rows(&dir), [(7, 1, reach(None))]);
    let refused =
        |caller: &str| (caller.to_owned(), SetReach::NAME.to_owned());
    assert_eq!(
        refusals(&dir),
        [
            refused("agent/caller"),
            refused("other/caller"),
            refused("client"),
            refused("none"),
        ]
    );
}

/// The workspace runs commands only under the sandbox its host was
/// given, and does not start without one. The core reports a failed
/// create as an internal error, without its reason.
#[tokio::test(flavor = "multi_thread")]
async fn the_workspace_does_not_start_without_a_sandbox() {
    let dir = Scratch::new("workspace-no-sandbox");
    let host = open(&dir, HostOptions::default()).unwrap();
    let started =
        host.get_or_start(workspace::KIND, workspace::MACHINE).await;
    host.shutdown().await.unwrap();

    assert!(started.is_err(), "the workspace started");
}
