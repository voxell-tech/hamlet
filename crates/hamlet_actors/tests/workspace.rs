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
use hamlet_host::{Address, Caller, Host, HostOptions, HostRef};
use hamlet_messages::agent::{Finished, Outcome};
use hamlet_messages::realm::{NeedsChief, SecretList, Secrets};
use hamlet_messages::workspace::reach::{
    Footing, Mode, Project, Reach, SecretsVersion,
};
use hamlet_messages::workspace::{
    AgentNumber, CallId, CallRefusal, CallerRefusal, CommandEnd,
    CommandId, CommandStatus, ConversationId, Exit, Program,
    ReachAnswer, ReachVersion, RealmRefusal, RealmTask, Run,
    RunAnswer, SetReach,
};
use hamlet_sandbox::Sandbox;
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

#[path = "workspace/files.rs"]
mod files;

#[path = "workspace/changes.rs"]
mod changes;

#[path = "workspace/waits.rs"]
mod waits;

#[path = "workspace/cancel.rs"]
mod cancel;

#[path = "workspace/workers.rs"]
mod workers;

#[path = "workspace/tools.rs"]
mod tools;

#[path = "workspace/realm_folder.rs"]
mod realm_folder;

const AGENT: AgentNumber = AgentNumber(7);

/// The secret the stub realm holds.
const SECRET: &str = "s3cr3t-value";

/// The kinds that call the workspace in these tests, each a [`Peer`].
const PEERS: [&str; 3] = ["realm", "agent", "other"];

#[derive(Default)]
struct Peers {
    answer_within: Option<Duration>,
    secrets_delay: Duration,
    fetched: AtomicUsize,
    told: Mutex<Vec<(String, Finished)>>,
    asked: Mutex<Vec<NeedsChief>>,
}

/// Stands in for the realm and the agents: calls the workspace as
/// itself, answers for the realm's secrets, and records what an agent
/// is told.
struct Peer {
    peers: Arc<Peers>,
    key: String,
}

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
    type Actions = (Forward, Secrets, Finished, NeedsChief, Leave);
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
        HostRef::of(ctx.inner())?
            .call(
                &workspace_address(),
                &forward.name,
                forward.args,
                Duration::from_secs(20),
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

impl Handler<NeedsChief> for Peer {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        asked: NeedsChief,
    ) -> anyhow::Result<()> {
        self.peers.asked.lock().unwrap().push(asked);
        Ok(())
    }
}

/// Asks a peer to destroy itself.
#[derive(Serialize, Deserialize)]
struct Leave;

impl Action for Leave {
    type Output = ();
    const NAME: &'static str = "leave";
}

impl Handler<Leave> for Peer {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Leave,
    ) -> anyhow::Result<()> {
        ctx.destroy()
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
    Address::new(workspace::KIND, workspace::MACHINE).unwrap()
}

fn realm() -> Address {
    Address::new(workspace::REALM, workspace::REALM_KEY).unwrap()
}

fn agent(number: AgentNumber) -> Address {
    Address::new(workspace::AGENT, number.0.to_string()).unwrap()
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
    let mut options = HostOptions::default()
        .with_value(Arc::new(Sandbox::testing()))
        .with_value(peers.clone());
    if let Some(answer_within) = peers.answer_within {
        options = options.with_value(Arc::new(workspace::Options {
            answer_within,
        }));
    }
    let host = open(dir, options).unwrap();
    host.get_or_start(&workspace_address()).await.unwrap();
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
    host.get_or_start(from).await?;
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
        let body = row.get::<_, Vec<u8>>(2)?;
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
    let tables = db
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table'",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<String>, _>>()
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

/// an agent, another kind, a client and a call with no caller
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
            &Address::new(kind, "caller").unwrap(),
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
    );
    answers.push(decoded(&client.await.unwrap()));
    let none = host.call(
        &to,
        SetReach::NAME,
        set_reach(5, reach(Some("/"))),
        Duration::from_secs(10),
    );
    answers.push(decoded(&none.await.unwrap()));
    host.shutdown().await.unwrap();
    let after_refusals = rows_in_file(&dir);
    let host = self::host(&dir).await;
    let realm = realm_sets(&host, 1, reach(None)).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        answers,
        vec![ReachAnswer::Refused(RealmRefusal::NotTheRealm); 4]
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
    let started = host.get_or_start(&workspace_address()).await;
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
        program: Program::Script {
            script: script.to_owned(),
            network: false,
            timeout_ms: None,
        },
    }
}

/// Calls the workspace with `run`, as agent 7.
async fn agent_runs(host: &Host, run: &Run) -> RunAnswer {
    runs_as(host, AGENT, run).await
}

/// Calls the workspace with `run`, as agent `from`.
async fn runs_as(
    host: &Host,
    from: AgentNumber,
    run: &Run,
) -> RunAnswer {
    let reply = call_as(
        host,
        &agent(from),
        Run::NAME,
        hamlet_actor::action::encode_positional(run).unwrap(),
    )
    .await
    .unwrap();
    decoded(&reply)
}

/// Asks the workspace, as agent `from`, how agent 7's `command`
/// stands.
async fn status_as(
    host: &Host,
    from: AgentNumber,
    command: CommandId,
) -> RunAnswer {
    let status = CommandStatus {
        agent: from,
        command,
    };
    let reply = call_as(
        host,
        &agent(from),
        CommandStatus::NAME,
        hamlet_actor::action::encode_positional(&status).unwrap(),
    )
    .await
    .unwrap();
    decoded(&reply)
}

/// The end of the command a [`Finished`] tells of.
fn ran(finished: &Finished) -> &CommandEnd {
    match &finished.outcome {
        Outcome::Ran(end) => end,
        other => panic!("the agent was told {other:?}"),
    }
}

fn told(peers: &Peers) -> Vec<(String, Finished)> {
    peers.told.lock().unwrap().clone()
}

/// Waits up to `within` for the agents to have been told `n` things.
async fn told_within(peers: &Peers, n: usize, within: Duration) {
    let until = tokio::time::Instant::now() + within;
    while told(peers).len() < n && tokio::time::Instant::now() < until
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The command a call answered `Running` for.
fn running(answer: &RunAnswer) -> CommandId {
    match answer {
        RunAnswer::Running { command } => *command,
        other => panic!("the command answered {other:?}"),
    }
}

/// Peers whose workspace answers `Running` after `ms`.
fn answering_within(ms: u64) -> Arc<Peers> {
    Arc::new(Peers {
        answer_within: Some(Duration::from_millis(ms)),
        ..Peers::default()
    })
}

/// Agent 7's filesystem, made by hand as the agent would.
fn filesystem(dir: &Path) -> std::path::PathBuf {
    let fs = dir.join("actors/agent/7.fs");
    std::fs::create_dir_all(&fs).unwrap();
    fs
}

/// The files under the realm folder `dir` whose bytes hold `needle`.
fn files_holding(
    dir: &Path,
    needle: &str,
) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![dir.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else if std::fs::read(&path).is_ok_and(|bytes| {
                bytes
                    .windows(needle.len())
                    .any(|at| at == needle.as_bytes())
            }) {
                found.push(path);
            }
        }
    }
    found
}

/// The end of a command that answered `Done`.
fn done(answer: RunAnswer) -> CommandEnd {
    match answer {
        RunAnswer::Done(end) => end,
        other => panic!("the command answered {other:?}"),
    }
}

/// C32: no command starts while the workspace holds no list of the
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

/// C32: a host's restart forgets the list, so the first command after
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

/// C32: a secret a command printed is in no byte of the workspace's
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

/// A command that ends within the wait answers `Done`, and the agent
/// is called with nothing.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_command_that_ends_in_time_answers_done() {
    let dir = Scratch::new("workspace-done");
    filesystem(&dir);
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let end = done(agent_runs(&host, &run("c1", "echo hi")).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        (end.exit, end.output.as_str()),
        (Exit::Code(0), "hi\n")
    );
    assert_eq!(told(&peers), []);
}

/// A command that runs past the wait answers `Running` with its id,
/// and the agent is called with its end once, when it ends. Its
/// status then answers that end.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_long_command_answers_running_then_finished() {
    let dir = Scratch::new("workspace-running");
    filesystem(&dir);
    let peers = answering_within(300);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let asked = std::time::Instant::now();
    let answer =
        agent_runs(&host, &run("c1", "sleep 1; echo late")).await;
    let answered_in = asked.elapsed();
    let command = running(&answer);
    let while_running = status_as(&host, AGENT, command).await;
    told_within(&peers, 1, Duration::from_secs(5)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after = status_as(&host, AGENT, command).await;
    let agent_id = agent(AgentNumber(7));
    host.get_or_start(&agent_id).await.unwrap();
    host.shutdown().await.unwrap();

    assert!(answered_in < Duration::from_secs(1), "{answered_in:?}");
    assert_eq!(while_running, RunAnswer::Running { command });
    let told = told(&peers);
    assert_eq!(told.len(), 1, "{told:?}");
    let (to, finished) = &told[0];
    assert_eq!(to, &agent_id.to_string());
    assert_eq!(finished.call, CallId("c1".into()));
    assert_eq!(ran(finished).command, command);
    assert_eq!(
        (&ran(finished).exit, ran(finished).output.as_str()),
        (&Exit::Code(0), "late\n")
    );
    assert_eq!(after, RunAnswer::Done(ran(finished).clone()));
}

/// A repeat of a call that arrives while the first still waits gets
/// the first's answer: `Done` for both when the command ends in time,
/// `Running` for both when it does not, and then one `Finished`.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_repeat_within_the_wait_answers_as_the_first() {
    for (wait_ms, script, both_done) in [
        (2000, "sleep 0.6; echo x >> runs", true),
        (400, "sleep 1.2; echo x >> runs", false),
    ] {
        let dir = Scratch::new("workspace-repeat-waiting");
        let fs = filesystem(&dir);
        let peers = answering_within(wait_ms);
        let host = host_with(&dir, peers.clone()).await;
        realm_sets(&host, 1, reach(None)).await;
        let call = run("c1", script);
        let first = {
            let (host, call) = (host.clone(), call.clone());
            tokio::spawn(
                async move { agent_runs(&host, &call).await },
            )
        };
        tokio::time::sleep(Duration::from_millis(200)).await;
        let repeat = agent_runs(&host, &call).await;
        let first = first.await.unwrap();
        told_within(&peers, 1, Duration::from_secs(3)).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        host.shutdown().await.unwrap();

        assert_eq!(repeat, first, "wait {wait_ms} ms");
        assert_eq!(matches!(first, RunAnswer::Done(_)), both_done);
        assert_eq!(told(&peers).len(), usize::from(!both_done));
        assert_eq!(
            std::fs::read_to_string(fs.join("runs")).unwrap(),
            "x\n"
        );
    }
}

/// A repeat after the command ended answers its end from the row and
/// runs nothing again; one with another request is refused.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_repeat_answers_from_the_row_and_runs_nothing() {
    let dir = Scratch::new("workspace-repeat");
    let fs = filesystem(&dir);
    let host = host(&dir).await;
    realm_sets(&host, 1, reach(None)).await;
    let call = run("c1", "echo x >> runs; echo ran");
    let first = agent_runs(&host, &call).await;
    let repeat = agent_runs(&host, &call).await;
    let other_task = Run {
        realm_task: Some(RealmTask("task-2".into())),
        ..call.clone()
    };
    let reused = agent_runs(&host, &other_task).await;
    host.shutdown().await.unwrap();

    assert!(matches!(first, RunAnswer::Done(_)), "{first:?}");
    assert_eq!(repeat, first);
    assert_eq!(
        reused,
        RunAnswer::Refused(CallRefusal::CallReused(CallId(
            "c1".into()
        )))
    );
    assert_eq!(
        std::fs::read_to_string(fs.join("runs")).unwrap(),
        "x\n"
    );
}

/// an agent's call naming another agent is refused and runs
/// nothing; a status asked for another agent's command finds none.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn the_agent_is_the_stamped_caller() {
    let dir = Scratch::new("workspace-stamped");
    let fs = filesystem(&dir);
    let peers = answering_within(200);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let other = AgentNumber(8);
    let named = runs_as(&host, other, &run("c1", "touch ran")).await;
    let command =
        running(&agent_runs(&host, &run("c2", "sleep 1")).await);
    let asked = status_as(&host, other, command).await;
    told_within(&peers, 1, Duration::from_secs(5)).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        named,
        RunAnswer::Refused(CallRefusal::Caller(
            CallerRefusal::NotTheCaller {
                named: AGENT,
                caller: other,
            }
        ))
    );
    assert!(!fs.join("ran").exists());
    assert_eq!(
        asked,
        RunAnswer::Refused(CallRefusal::NoSuchCommand(command))
    );
}

/// A command the workspace's host stopped under is gone, and at the
/// next start its row reads cut off; the workspace calls no one.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_running_row_is_cut_off_at_the_next_start() {
    let dir = Scratch::new("workspace-cut-off");
    let fs = filesystem(&dir);
    let peers = answering_within(200);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let command = running(
        &agent_runs(&host, &run("c1", "sleep 1; touch late")).await,
    );
    host.shutdown().await.unwrap();
    let again = host_with(&dir, peers.clone()).await;
    let status = status_as(&again, AGENT, command).await;
    let repeat =
        agent_runs(&again, &run("c1", "sleep 1; touch late")).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    again.shutdown().await.unwrap();

    let cut_off = RunAnswer::Done(CommandEnd {
        command,
        exit: Exit::CutOff,
        output: String::new(),
        cut: None,
        policy: String::new(),
        took_ms: 0,
    });
    assert_eq!(status, cut_off);
    assert_eq!(repeat, cut_off);
    assert!(!fs.join("late").exists(), "the command ran on");
    assert_eq!(told(&peers), []);
}

/// A call does not make its target: `Finished` to an agent that was
/// destroyed fails, the workspace drops it, and the row keeps the
/// end.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_finished_to_an_agent_that_is_gone_is_dropped() {
    let dir = Scratch::new("workspace-agent-gone");
    filesystem(&dir);
    let peers = answering_within(200);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let command = running(
        &agent_runs(&host, &run("c1", "sleep 0.8; echo end")).await,
    );
    host.call(
        &agent(AGENT),
        Leave::NAME,
        hamlet_actor::action::encode_positional(&Leave).unwrap(),
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let agent_file = dir.join("actors/agent/7.sqlite").exists();
    host.shutdown().await.unwrap();

    assert_eq!(told(&peers), []);
    assert!(!agent_file, "the call made the agent again");
    let db = read_only(&dir);
    let state = db
        .query_row::<String, _, _>(
            "SELECT state FROM calls WHERE id = ?1",
            [i64::try_from(command.0).unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "ended");
}

/// A command that passes its time limit is killed whole and ends as
/// timed out: answered `Done` when the limit comes within the wait,
/// else `Running` and then one `Finished`.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_command_past_its_limit_ends_timed_out() {
    for (wait_ms, limit_ms, in_the_answer) in
        [(3000, 300, true), (200, 800, false)]
    {
        let dir = Scratch::new("workspace-timed-out");
        let fs = filesystem(&dir);
        let peers = answering_within(wait_ms);
        let host = host_with(&dir, peers.clone()).await;
        realm_sets(&host, 1, reach(None)).await;
        let mut call =
            run("c1", "(sleep 2; touch late) & sleep 2; touch late");
        if let Program::Script { timeout_ms, .. } = &mut call.program
        {
            *timeout_ms = Some(limit_ms);
        }
        let answer = agent_runs(&host, &call).await;
        told_within(&peers, 1, Duration::from_secs(3)).await;
        tokio::time::sleep(Duration::from_millis(2500)).await;
        host.shutdown().await.unwrap();

        let end = match answer {
            RunAnswer::Done(end) => {
                assert!(in_the_answer, "answered Done past the wait");
                assert_eq!(told(&peers), []);
                end
            }
            RunAnswer::Running { command } => {
                assert!(!in_the_answer, "answered Running in time");
                let told = told(&peers);
                assert_eq!(told.len(), 1, "{told:?}");
                assert_eq!(ran(&told[0].1).command, command);
                ran(&told[0].1).clone()
            }
            other => panic!("the command answered {other:?}"),
        };
        assert_eq!(end.exit, Exit::TimedOut, "limit {limit_ms} ms");
        assert!(!fs.join("late").exists(), "the command ran on");
    }
}

/// The end of `answer`, `Done` or told in one `Finished`.
async fn ended(peers: &Peers, answer: RunAnswer) -> CommandEnd {
    match answer {
        RunAnswer::Done(end) => end,
        RunAnswer::Running { .. } => {
            told_within(peers, 1, Duration::from_secs(20)).await;
            ran(&told(peers).pop().expect("a Finished").1).clone()
        }
        other => panic!("the command answered {other:?}"),
    }
}

/// Output past the tail's limits goes whole, scrubbed, to the spill
/// file the end names; the end carries only the tail. Output within
/// them leaves no file.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn output_past_the_tail_spills_to_a_file() {
    let dir = Scratch::new("workspace-spill");
    filesystem(&dir);
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let short =
        done(agent_runs(&host, &run("c1", "seq 1 2000")).await);
    let script = format!("seq 1 3000; echo {SECRET}");
    let long =
        ended(&peers, agent_runs(&host, &run("c2", &script)).await)
            .await;
    host.shutdown().await.unwrap();

    assert_eq!(short.cut, None);
    assert_eq!(short.output.lines().count(), 2000);
    assert_eq!(long.exit, Exit::Code(0));
    let cut = long.cut.expect("the output was cut");
    let mut all =
        (1..=3000).map(|n| format!("{n}\n")).collect::<String>();
    all.push_str("[REDACTED]\n");
    assert_eq!(cut.total_bytes, all.len() as u64);
    assert_eq!(cut.total_lines, 3001);
    assert_eq!(
        cut.full_output,
        dir.join(format!(
            "actors/workspace/local.output/7/{}.log",
            long.command.0
        ))
    );
    assert_eq!(
        std::fs::read_to_string(&cut.full_output).unwrap(),
        all
    );
    assert!(
        long.output.starts_with("1002\n"),
        "{}",
        &long.output[..9]
    );
    assert!(long.output.ends_with("3000\n[REDACTED]\n"));
    let spilled =
        std::fs::read_dir(cut.full_output.parent().unwrap())
            .unwrap()
            .count();
    assert_eq!(spilled, 1, "output within the tail left a file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&cut.full_output)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

/// An agent's commands can read its own spilled output and no other
/// agent's, with a project and without.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_command_reads_only_its_own_agents_spill() {
    let dir = Scratch::new("workspace-spill-reach");
    filesystem(&dir);
    let (_project, project) =
        files::project("workspace-spill-p", &[]);
    let other = dir.join("actors/workspace/local.output/8");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("1.log"), "theirs\n").unwrap();
    let host = host(&dir).await;
    let spilled = done(
        agent_runs(&host, &{
            realm_sets(&host, 1, reach(None)).await;
            run("c1", "seq 1 3000")
        })
        .await,
    );
    let own = spilled.cut.expect("the output was cut").full_output;
    let mut seen = Vec::new();
    for (version, footing) in [
        (1, reach(None)),
        (2, reach(Some(project.to_str().unwrap()))),
    ] {
        realm_sets(&host, version, footing).await;
        for (name, path) in
            [("own", own.clone()), ("other", other.join("1.log"))]
        {
            let script = format!("tail -n 1 {}", path.display());
            let end = done(
                agent_runs(
                    &host,
                    &run(&format!("{name}-{version}"), &script),
                )
                .await,
            );
            seen.push((
                version,
                name,
                end.exit == Exit::Code(0),
                end.output,
            ));
        }
    }
    host.shutdown().await.unwrap();

    for (version, name, read, output) in seen {
        match name {
            "own" => assert!(
                read && output == "3000\n",
                "version {version}: {output}"
            ),
            _ => assert!(
                !read && !output.contains("theirs"),
                "version {version}: {output}"
            ),
        }
    }
}
