//! The workspace through a host, called by the actors that call it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorFactory};
use hamlet_actors::workspace;
use hamlet_host::{
    Address, Caller, Cause, Host, HostOptions, HostRef,
};
use hamlet_messages::agent::Finished;
use hamlet_messages::realm::{SecretList, Secrets};
use hamlet_messages::workspace::reach::{
    Footing, Mode, Project, Reach, SecretsVersion,
};
use hamlet_messages::workspace::{
    AgentNumber, CallId, CommandEnd, ConversationId, Exit,
    ReachAnswer, ReachRefusal, ReachVersion, Run, RunAnswer,
    SetReach, Shell,
};
use hamlet_sandbox::Sandbox;
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

const AGENT: AgentNumber = AgentNumber(7);

/// The secret the stub realm holds.
const SECRET: &str = "s3cr3t-value";

/// The kinds that call the workspace in these tests, each a [`Peer`].
const PEERS: [&str; 3] = ["realm", "agent", "other"];

/// What the peers of one host share: the realm's secrets and how
/// long it takes to answer for them, and what the agents were told.
#[derive(Default)]
struct Peers {
    secrets_delay: Duration,
    fetched: AtomicUsize,
    told: Mutex<Vec<(String, Finished)>>,
}

/// Stands in for the realm and the agents: calls the workspace as
/// itself, answers for the realm's secrets, and records what an agent
/// is told.
struct Peer {
    peers: Arc<Peers>,
    key: String,
}

/// What a [`Peer`] calls the workspace with.
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
impl Actor for Peer {
    type Input = ();
    type Actions = (Forward, Secrets, Finished);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        let peers = HostRef::of(ctx.inner())?
            .value::<Peers>()
            .context("the host holds the peers")?;
        Ok(Self {
            peers,
            key: ctx.actor_id().to_owned(),
        })
    }
}

impl Handler<Forward> for Peer {
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
            Duration::from_secs(20),
            Cause::root("test"),
        )
        .await
    }
}

impl Handler<Secrets> for Peer {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Secrets,
    ) -> anyhow::Result<SecretList> {
        tokio::time::sleep(self.peers.secrets_delay).await;
        self.peers.fetched.fetch_add(1, Ordering::SeqCst);
        Ok(SecretList {
            version: SecretsVersion(1),
            values: vec![SECRET.to_owned()],
        })
    }
}

impl Handler<Finished> for Peer {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        finished: Finished,
    ) -> anyhow::Result<()> {
        self.peers
            .told
            .lock()
            .unwrap()
            .push((self.key.clone(), finished));
        Ok(())
    }
}

fn peer(kind: &str) -> ActorFactory {
    let mut registry = Registry::new();
    registry
        .register_actor_with::<Peer>(kind, ActorConfig::default());
    registry
        .into_factories()
        .remove(kind)
        .expect("the registry holds the kind it registered")
}

fn workspace_address() -> Address {
    Address::new(workspace::KIND, workspace::MACHINE)
}

fn realm() -> Address {
    Address::new(workspace::REALM, workspace::REALM_KEY)
}

fn agent(number: AgentNumber) -> Address {
    Address::new(workspace::AGENT, number.0.to_string())
}

/// A host on `dir` with the workspace and the callers' kinds, given
/// `options`.
fn open(dir: &Path, options: HostOptions) -> anyhow::Result<Host> {
    let mut kinds = HashMap::from([(
        workspace::KIND.to_owned(),
        workspace::factory(),
    )]);
    for kind in PEERS {
        kinds.insert(kind.to_owned(), peer(kind));
    }
    Host::open_with(dir, kinds, options)
}

/// A host on `dir` with the workspace made, on the tests' sandbox.
async fn host(dir: &Path) -> Host {
    host_with(dir, Arc::default()).await
}

/// A host on `dir` with the workspace made, whose peers share
/// `peers`.
async fn host_with(dir: &Path, peers: Arc<Peers>) -> Host {
    let host = open(
        dir,
        HostOptions::default()
            .with_value(Arc::new(Sandbox::testing()))
            .with_value(peers),
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

/// Calls the workspace's `name` with `args` as the actor at `from`,
/// made first if it has no file.
async fn call_as(
    host: &Host,
    from: &Address,
    name: &str,
    args: Vec<u8>,
) -> anyhow::Result<Vec<u8>> {
    host.get_or_start(&from.kind, &from.key).await?;
    let forward = Forward {
        name: name.to_owned(),
        args,
    };
    let reply = host
        .call(
            from,
            Forward::NAME,
            hamlet_actor::action::encode_positional(&forward)?,
            Duration::from_secs(20),
            Cause::root("test"),
        )
        .await?;
    ciborium::from_reader(reply.as_slice())
        .context("decode the peer's reply")
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
        &realm(),
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

/// The number of rows in every table of the workspace's file.
fn rows_in_file(dir: &Path) -> i64 {
    let db = read_only(dir);
    let tables: Vec<String> = db
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table'",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    tables
        .iter()
        .map(|table| {
            db.query_row(
                &format!("SELECT count(*) FROM \"{table}\""),
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
        })
        .sum()
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
/// are each refused, leave no row, and change no reach. Their
/// version 5 would outrank the realm's version 1 had any of them
/// applied.
#[tokio::test(flavor = "multi_thread")]
async fn only_the_realm_sets_a_reach() {
    let dir = Scratch::new("workspace-reach-not-realm");
    host(&dir).await.shutdown().await.unwrap();
    let before = rows_in_file(&dir);
    let host = host(&dir).await;
    let mut answers = Vec::new();
    for kind in ["agent", "other"] {
        let reply = call_as(
            &host,
            &Address::new(kind, "caller"),
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
    host.shutdown().await.unwrap();
    let after_refusals = rows_in_file(&dir);
    let host = self::host(&dir).await;
    let realm = realm_sets(&host, 1, reach(None)).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        answers,
        vec![ReachAnswer::Refused(ReachRefusal::NotTheRealm); 4]
    );
    assert_eq!(realm, ReachAnswer::InForce(ReachVersion(1)));
    assert_eq!(reach_rows(&dir), [(7, 1, reach(None))]);
    assert_eq!(after_refusals, before, "a refusal left a row");
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

/// A [`Run`] of `script` by agent 7, as its call `call`.
fn run(call: &str, script: &str) -> Run {
    Run {
        agent: AGENT,
        call: CallId(call.to_owned()),
        conversation: ConversationId("c1".to_owned()),
        realm_task: None,
        shell: Shell::Bash,
        script: script.to_owned(),
        network: false,
        timeout_ms: None,
    }
}

/// Calls the workspace with `run`, as agent 7.
async fn agent_runs(host: &Host, run: &Run) -> RunAnswer {
    let reply = call_as(
        host,
        &agent(AGENT),
        Run::NAME,
        hamlet_actor::action::encode_positional(run).unwrap(),
    )
    .await
    .unwrap();
    decoded(&reply)
}

/// Agent 7's filesystem, made by hand as the agent would.
fn filesystem(dir: &Path) -> std::path::PathBuf {
    let fs = dir.join("actors/agent/7.fs");
    std::fs::create_dir_all(&fs).unwrap();
    fs
}

/// The end of a command that answered `Done`.
fn done(answer: RunAnswer) -> CommandEnd {
    match answer {
        RunAnswer::Done(end) => end,
        other => panic!("the command answered {other:?}"),
    }
}

/// W3: no command starts while the workspace holds no list of the
/// realm's secrets, as after every start. It calls the realm for it
/// first, and takes the secret out of the command's output.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_command_waits_for_the_secrets_list() {
    let dir = Scratch::new("workspace-secrets-wait");
    let fs = filesystem(&dir);
    let peers = Arc::new(Peers {
        secrets_delay: Duration::from_millis(1500),
        ..Peers::default()
    });
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let script =
        format!("touch started; echo the secret is {SECRET}");
    let call = {
        let host = host.clone();
        let run = run("c1", &script);
        tokio::spawn(async move { agent_runs(&host, &run).await })
    };
    tokio::time::sleep(Duration::from_millis(700)).await;
    let started_early = fs.join("started").exists();
    let end = done(call.await.unwrap());
    host.shutdown().await.unwrap();

    assert!(!started_early, "the command started before the list");
    assert_eq!(peers.fetched.load(Ordering::SeqCst), 1);
    assert_eq!(end.exit, Exit::Code(0));
    assert_eq!(end.output, "the secret is [REDACTED]\n");
    assert!(fs.join("started").exists());
}

/// W3: a host's restart forgets the list, so the first command after
/// it calls the realm again.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn the_list_is_fetched_again_after_a_restart() {
    let dir = Scratch::new("workspace-secrets-restart");
    filesystem(&dir);
    let script = format!("echo {SECRET}");
    let mut fetched = Vec::new();
    let mut outputs = Vec::new();
    for call in ["c1", "c2"] {
        let peers = Arc::new(Peers::default());
        let host = host_with(&dir, peers.clone()).await;
        realm_sets(&host, 1, reach(None)).await;
        outputs.push(
            done(agent_runs(&host, &run(call, &script)).await).output,
        );
        host.shutdown().await.unwrap();
        fetched.push(peers.fetched.load(Ordering::SeqCst));
    }

    assert_eq!(fetched, [1, 1]);
    assert_eq!(outputs, ["[REDACTED]\n"; 2]);
}

/// W3: a secret a command printed is in no byte of the workspace's
/// file, its write-ahead file or its shared-memory file.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn no_secret_reaches_the_workspaces_file() {
    let dir = Scratch::new("workspace-secrets-file");
    filesystem(&dir);
    let host = host(&dir).await;
    realm_sets(&host, 1, reach(None)).await;
    let end = done(
        agent_runs(
            &host,
            &run("c1", &format!("echo {SECRET}; exit 3")),
        )
        .await,
    );
    host.shutdown().await.unwrap();

    assert_eq!(end.exit, Exit::Code(3));
    let mut searched = 0;
    for suffix in ["", "-wal", "-shm"] {
        let path = dir
            .join(format!("actors/workspace/local.sqlite{suffix}"));
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        searched += 1;
        assert!(
            !bytes
                .windows(SECRET.len())
                .any(|at| at == SECRET.as_bytes()),
            "{} holds the secret",
            path.display()
        );
    }
    assert!(searched > 0, "no file was searched");
}
