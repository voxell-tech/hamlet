//! What an actor reaches of its host: the realm folder, the values
//! the program that opened the host gave it, and the other actors,
//! which it calls with its address stamped on the call. A call never
//! makes the actor it calls.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use at::at;
use call::call;
use code::code;
use files::any_file_of;
use hamlet_actor::{
    Action, Actor, ActorConfig, Ctx, Handler, Registry,
};
use hamlet_actor_core::{
    ActorContext, ActorEvent, ActorFactory, ActorStart, ShutdownKind,
};
use hamlet_host::{Address, Caller, Host, HostOptions, HostRef};
use hamlet_testing::Scratch;
use logs::Logs;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[path = "support/at.rs"]
mod at;
#[path = "support/began.rs"]
mod began;
#[path = "support/call.rs"]
mod call;
#[path = "support/code.rs"]
mod code;
#[path = "support/files.rs"]
mod files;
#[path = "support/logs.rs"]
mod logs;

/// The kind `kind`, an actor of the trait API `A`, with a database.
fn kind<A: Actor>(registry: &mut Registry, kind: &str) {
    registry.register_actor_with::<A>(
        kind,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
}

/// Keeps the realm folder its host gave it in `create`.
struct Here {
    home: String,
}

#[derive(Serialize, Deserialize)]
struct Home;

impl Action for Home {
    type Output = String;
    const NAME: &'static str = "home";
}

#[async_trait::async_trait]
impl Actor for Here {
    type Input = String;
    type Actions = (Home,);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        let host = HostRef::of(ctx.inner())?;
        Ok(Self {
            home: host.folder().display().to_string(),
        })
    }
}

impl Handler<Home> for Here {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Home,
    ) -> anyhow::Result<String> {
        Ok(self.home.clone())
    }
}

/// An actor reads, through its [`HostRef`], the realm folder its host
/// was opened on.
#[tokio::test(flavor = "multi_thread")]
async fn an_actor_reaches_the_realm_folder_of_its_host() {
    let dir = Scratch::new("access-home");
    let mut registry = Registry::new();
    registry.register_actor_with::<Here>(
        "here",
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    let host = Host::open(&dir, registry.into_factories()).unwrap();
    host.get_or_start(&at("here", "one")).await.unwrap();
    let reply = host
        .call(
            &at("here", "one"),
            Home::NAME,
            hamlet_actor::action::encode_positional(&Home).unwrap(),
            Duration::from_secs(10),
        )
        .await;
    host.shutdown().await.unwrap();
    let home: String =
        ciborium::from_reader(reply.unwrap().as_slice()).unwrap();
    assert_eq!(home, dir.display().to_string());
}

/// A value the embedder gives its host. Each read of it by an actor
/// counts one.
struct Tally(AtomicUsize);

impl Tally {
    fn read(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// Reads the host's [`Tally`] in `create`, and again in its handler.
struct Reader {
    tally: Arc<Tally>,
}

/// Reads the [`Tally`] again. Answers whether it is the one `create`
/// read, and whether the host holds a `String`, which nobody gave.
#[derive(Serialize, Deserialize)]
struct Read;

impl Action for Read {
    type Output = (bool, bool);
    const NAME: &'static str = "read";
}

#[async_trait::async_trait]
impl Actor for Reader {
    type Input = ();
    type Actions = (Read,);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        let tally = HostRef::of(ctx.inner())?
            .value::<Tally>()
            .context("the host holds no tally")?;
        tally.read();
        Ok(Self { tally })
    }
}

impl Handler<Read> for Reader {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Read,
    ) -> anyhow::Result<(bool, bool)> {
        let host = HostRef::of(ctx.inner())?;
        let tally = host
            .value::<Tally>()
            .context("the host holds no tally")?;
        tally.read();
        Ok((
            Arc::ptr_eq(&tally, &self.tally),
            host.value::<String>().is_some(),
        ))
    }
}

/// The value an actor reads in `create` and in a handler is the
/// one the embedder gave, and a type nobody gave reads as none.
#[tokio::test(flavor = "multi_thread")]
async fn an_actor_reads_its_hosts_values_in_create_and_in_a_handler()
{
    let dir = Scratch::new("access-values");
    let tally = Arc::new(Tally(AtomicUsize::new(0)));
    let mut registry = Registry::new();
    kind::<Reader>(&mut registry, "reader");
    let host = Host::open_with(
        &dir,
        registry.into_factories(),
        HostOptions::default().with_value(tally.clone()),
    )
    .unwrap();
    host.get_or_start(&at("reader", "a")).await.unwrap();
    let read = call(&host, &at("reader", "a"), &Read).await;
    host.shutdown().await.unwrap();
    assert_eq!(read.unwrap(), (true, false));
    assert_eq!(tally.0.load(Ordering::SeqCst), 2);
}

/// Keeps its [`HostRef`] for as long as it runs.
struct Keeper {
    _host: HostRef,
}

#[derive(Serialize, Deserialize)]
struct Ping;

impl Action for Ping {
    type Output = ();
    const NAME: &'static str = "ping";
}

#[async_trait::async_trait]
impl Actor for Keeper {
    type Input = ();
    type Actions = (Ping,);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self {
            _host: HostRef::of(ctx.inner())?,
        })
    }
}

impl Handler<Ping> for Keeper {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Ping,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

fn keepers() -> std::collections::HashMap<String, ActorFactory> {
    let mut registry = Registry::new();
    kind::<Keeper>(&mut registry, "keeper");
    registry.into_factories()
}

/// An actor that keeps its [`HostRef`] does not keep its host:
/// once the last `Host` is dropped, a new host opens the folder.
#[tokio::test(flavor = "multi_thread")]
async fn an_actor_that_keeps_its_host_ref_does_not_keep_the_host() {
    let dir = Scratch::new("access-keeper");
    let host = Host::open(&dir, keepers()).unwrap();
    host.get_or_start(&at("keeper", "a")).await.unwrap();
    call(&host, &at("keeper", "a"), &Ping).await.unwrap();
    drop(host);
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(2);
    let host = loop {
        match Host::open(&dir, keepers()) {
            Ok(host) => break host,
            Err(error) => {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "the folder is still locked: {error:#}"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    };
    host.shutdown().await.unwrap();
}

/// Answers who called it, and holds when asked to.
struct Callee;

/// Answers the caller the host stamped on this call. `claimed` names
/// an actor, as the arguments of a caller that would pass for another
/// would.
#[derive(Serialize, Deserialize)]
struct Who {
    claimed: Option<Caller>,
}

impl Action for Who {
    type Output = Option<Caller>;
    const NAME: &'static str = "who";
}

/// Holds for `ms` milliseconds, then answers.
#[derive(Serialize, Deserialize)]
struct Hold {
    ms: u64,
}

impl Action for Hold {
    type Output = ();
    const NAME: &'static str = "hold";
}

#[async_trait::async_trait]
impl Actor for Callee {
    type Input = ();
    type Actions = (Who, Hold);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(_: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self)
    }
}

impl Handler<Who> for Callee {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Who,
    ) -> anyhow::Result<Option<Caller>> {
        ctx.conn().context("the call has no connection")?.params()
    }
}

impl Handler<Hold> for Callee {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Hold { ms }: Hold,
    ) -> anyhow::Result<()> {
        Holds::begin(ctx.inner())?;
        tokio::time::sleep(Duration::from_millis(ms)).await;
        Ok(())
    }
}

/// How a call went: its reply, or its error's `group.code`, and how
/// long it took.
#[derive(Debug, Serialize, Deserialize)]
struct Outcome {
    reply: Option<Vec<u8>>,
    error: Option<String>,
    ms: u64,
}

impl Outcome {
    async fn of(
        call: impl Future<Output = anyhow::Result<Vec<u8>>>,
    ) -> Self {
        let begun = Instant::now();
        let result = call.await;
        let ms = u64::try_from(begun.elapsed().as_millis()).unwrap();
        match result {
            Ok(reply) => Self {
                reply: Some(reply),
                error: None,
                ms,
            },
            Err(error) => Self {
                reply: None,
                error: Some(code(&error)),
                ms,
            },
        }
    }

    /// The reply, decoded as `T`.
    fn reply<T: DeserializeOwned>(&self) -> T {
        let reply = self
            .reply
            .as_ref()
            .unwrap_or_else(|| panic!("the call failed: {self:?}"));
        ciborium::from_reader(reply.as_slice()).unwrap()
    }
}

/// The contexts of the [`Asker`]s a host started, in order, when the
/// host holds this value.
#[derive(Default)]
struct Contexts(Mutex<Vec<ActorContext>>);

/// Calls another actor from its own handler.
struct Asker;

/// Calls the action `name` of the actor at `to` with `args`, within
/// `deadline_ms`, and answers how that went.
#[derive(Serialize, Deserialize)]
struct Ask {
    to: Address,
    name: String,
    args: Vec<u8>,
    deadline_ms: u64,
}

impl Action for Ask {
    type Output = Outcome;
    const NAME: &'static str = "ask";
}

#[async_trait::async_trait]
impl Actor for Asker {
    type Input = ();
    type Actions = (Ask, Ping);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        if let Some(contexts) =
            HostRef::of(ctx.inner())?.value::<Contexts>()
        {
            contexts.0.lock().unwrap().push(ctx.inner().clone());
        }
        Ok(Self)
    }
}

impl Handler<Ask> for Asker {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        ask: Ask,
    ) -> anyhow::Result<Outcome> {
        Ok(Outcome::of(async {
            HostRef::of(ctx.inner())?
                .call(
                    &ask.to,
                    &ask.name,
                    ask.args,
                    Duration::from_millis(ask.deadline_ms),
                )
                .await
        })
        .await)
    }
}

impl Handler<Ping> for Asker {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Ping,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

/// The kind `raw`, on the core's API. Its action `params` answers
/// with its call's connection parameters, as they came; `hold` holds
/// for a minute. Its stop waits three seconds.
fn raw() -> ActorFactory {
    ActorFactory::new(
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
        |start: ActorStart| {
            Box::pin(async move {
                let ActorStart {
                    ctx, mut events, ..
                } = start;
                while let Some(event) = events.recv().await {
                    match event {
                        ActorEvent::Action {
                            name,
                            conn,
                            reply,
                            ..
                        } => {
                            let ctx = ctx.clone();
                            tokio::spawn(async move {
                                if name == "hold" {
                                    let _ = Holds::begin(&ctx);
                                    tokio::time::sleep(
                                        Duration::from_secs(60),
                                    )
                                    .await;
                                }
                                reply.send(Ok(conn
                                    .map(|conn| conn.params())
                                    .unwrap_or_default()));
                            });
                        }
                        ActorEvent::RunGracefulCleanup {
                            reply,
                            ..
                        } => {
                            tokio::spawn(async move {
                                tokio::time::sleep(
                                    Duration::from_secs(3),
                                )
                                .await;
                                reply.send(Ok(()));
                            });
                        }
                        ActorEvent::SerializeState {
                            reply, ..
                        } => {
                            reply.send(Ok(Vec::new()));
                        }
                        ActorEvent::ConnectionPreflight {
                            reply,
                            ..
                        }
                        | ActorEvent::ConnectionOpen {
                            reply, ..
                        }
                        | ActorEvent::DisconnectConn {
                            reply, ..
                        } => {
                            reply.send(Ok(()));
                        }
                        _ => {}
                    }
                }
                Ok(())
            })
        },
    )
}

/// The kinds `asker`, `callee` and `raw`, in a host that holds
/// `contexts`.
fn open_callers(dir: &Scratch, contexts: Arc<Contexts>) -> Host {
    open_callers_with(
        dir,
        HostOptions::default().with_value(contexts),
    )
}

/// The kinds `asker`, `callee` and `raw`, in a host of `options`.
fn open_callers_with(dir: &Scratch, options: HostOptions) -> Host {
    let mut registry = Registry::new();
    kind::<Asker>(&mut registry, "asker");
    kind::<Callee>(&mut registry, "callee");
    let mut factories = registry.into_factories();
    factories.insert("raw".to_owned(), raw());
    Host::open_with(dir, factories, options).unwrap()
}

/// Counts the holds that began, `hold` of [`raw`] and [`Hold`] of
/// [`Callee`], in a host that holds it: each adds a permit as it
/// begins.
struct Holds(tokio::sync::Semaphore);

impl Holds {
    fn new() -> Arc<Self> {
        Arc::new(Self(tokio::sync::Semaphore::new(0)))
    }

    /// Adds the permit of a hold that began, in a host that holds
    /// [`Holds`].
    fn begin(ctx: &ActorContext) -> anyhow::Result<()> {
        if let Some(holds) = HostRef::of(ctx)?.value::<Self>() {
            holds.0.add_permits(1);
        }
        Ok(())
    }

    /// Waits for `n` holds to begin.
    async fn began(&self, n: u32) {
        began::began("the holds", self.0.acquire_many(n))
            .await
            .unwrap()
            .forget();
    }
}

/// Has the actor at `from` call `action` of the actor at `to`, within
/// `deadline`, and answers how that went.
async fn ask<M: Action>(
    host: &Host,
    from: &Address,
    to: &Address,
    action: &M,
    deadline: Duration,
) -> Outcome {
    call(
        host,
        from,
        &Ask {
            to: to.clone(),
            name: M::NAME.to_owned(),
            args: hamlet_actor::action::encode_positional(action)
                .unwrap(),
            deadline_ms: u64::try_from(deadline.as_millis()).unwrap(),
        },
    )
    .await
    .unwrap()
}

/// Makes each actor of `at`.
async fn make(host: &Host, at: &[&Address]) {
    for at in at {
        host.get_or_start(at).await.unwrap();
    }
}

/// The callee reads the caller the host stamped, whatever the
/// call's arguments say: each of two actors as itself, the embedder's
/// call as none and a client's as a client.
#[tokio::test(flavor = "multi_thread")]
async fn the_callee_reads_who_called_whatever_the_arguments_say() {
    let dir = Scratch::new("call-who");
    let host = open_callers(&dir, Arc::default());
    let callee = at("callee", "x");
    let (a, b) = (at("asker", "a"), at("asker", "b"));
    make(&host, &[&callee, &a, &b]).await;
    let who = Who {
        claimed: Some(Caller::Actor(at("asker", "z"))),
    };
    for from in [&a, &b] {
        let outcome =
            ask(&host, from, &callee, &who, Duration::from_secs(10))
                .await;
        assert_eq!(
            outcome.reply::<Option<Caller>>(),
            Some(Caller::Actor(from.clone())),
        );
    }
    let args = hamlet_actor::action::encode_positional(&who).unwrap();
    let embedder = Outcome::of(host.call(
        &callee,
        Who::NAME,
        args.clone(),
        Duration::from_secs(10),
    ))
    .await;
    let client = Outcome::of(host.call_for_client(
        &callee,
        Who::NAME,
        args,
        Duration::from_secs(10),
    ))
    .await;
    host.shutdown().await.unwrap();
    assert_eq!(embedder.reply::<Option<Caller>>(), None);
    assert_eq!(
        client.reply::<Option<Caller>>(),
        Some(Caller::Client)
    );
}

/// A call's log line names its caller, its target and its action:
/// an actor's call its address, a client's `client`, and the
/// embedder's call no caller. What the call logs as it opens its
/// connection carries that line too.
#[tokio::test]
async fn a_calls_log_line_names_its_caller_target_and_action() {
    let dir = Scratch::new("call-log");
    let host = open_callers(&dir, Arc::default());
    let callee = at("callee", "x");
    let a = at("asker", "a");
    make(&host, &[&callee, &a]).await;
    let who = Who { claimed: None };
    let args = hamlet_actor::action::encode_positional(&who).unwrap();
    let deadline = Duration::from_secs(10);
    let logs = Logs::default();
    let logging =
        logs.record(tracing_subscriber::filter::LevelFilter::DEBUG);
    ask(&host, &a, &callee, &who, deadline).await;
    host.call_for_client(&callee, Who::NAME, args.clone(), deadline)
        .await
        .unwrap();
    host.call(&callee, Who::NAME, args, deadline).await.unwrap();
    drop(logging);
    host.shutdown().await.unwrap();
    let logged = logs.text();
    let calls = logged
        .lines()
        .filter(|line| line.ends_with(": new"))
        .filter_map(|line| {
            let span = &line[line.find("call{")?..];
            Some(span[..=span.find('}')?].to_owned())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        calls,
        [
            r#"call{actor_id="asker/a" action="ask"}"#,
            r#"call{actor_id="callee/x" action="who" caller=asker/a}"#,
            r#"call{actor_id="callee/x" action="who" caller=client}"#,
            r#"call{actor_id="callee/x" action="who"}"#,
        ]
    );
    let opened = logged
        .lines()
        .filter(|line| line.contains("connection added"))
        .collect::<Vec<_>>();
    assert_eq!(opened.len(), 4, "{opened:#?}");
    assert!(
        opened.iter().all(|line| line.contains(" call{")),
        "{opened:#?}"
    );
}

#[test]
fn an_address_in_an_actions_arguments_has_fixed_bytes() {
    let args = hamlet_actor::action::encode_positional(&Ask {
        to: at("raw", "x"),
        name: "params".to_owned(),
        args: Vec::new(),
        deadline_ms: 10_000,
    })
    .unwrap();
    assert_eq!(
        args,
        b"\x81\xa4\x64args\x80\x6bdeadline_ms\x19\x27\x10\x64name\x66params\x62to\xa2\x63key\x61x\x64kind\x63raw"
    );
}

/// The stamp's bytes, which other hosts and other languages read:
/// none is CBOR null, a client the text `client`, and an actor a map
/// from `actor` to its address.
#[tokio::test(flavor = "multi_thread")]
async fn the_stamp_has_fixed_bytes() {
    let dir = Scratch::new("call-stamp");
    let host = open_callers(&dir, Arc::default());
    let raw = at("raw", "x");
    let a = at("asker", "a");
    make(&host, &[&raw, &a]).await;
    let params = |caller: Option<Caller>| {
        let host = host.clone();
        let raw = raw.clone();
        async move {
            let deadline = Duration::from_secs(10);
            match caller {
                None => {
                    host.call(&raw, "params", Vec::new(), deadline)
                        .await
                }
                Some(_) => {
                    host.call_for_client(
                        &raw,
                        "params",
                        Vec::new(),
                        deadline,
                    )
                    .await
                }
            }
            .unwrap()
        }
    };
    let none = params(None).await;
    let client = params(Some(Caller::Client)).await;
    let actor = call(
        &host,
        &a,
        &Ask {
            to: raw.clone(),
            name: "params".to_owned(),
            args: Vec::new(),
            deadline_ms: 10_000,
        },
    )
    .await
    .unwrap()
    .reply
    .unwrap();
    host.shutdown().await.unwrap();
    assert_eq!(none, [0xf6]);
    assert_eq!(client, b"\x66client");
    assert_eq!(
        actor,
        b"\xa1\x65actor\xa2\x64kind\x65asker\x63key\x61a".to_vec()
    );
}

/// An actor's call starts a callee that sleeps.
#[tokio::test(flavor = "multi_thread")]
async fn an_actors_call_starts_a_sleeping_callee() {
    let dir = Scratch::new("call-sleeping");
    let host = open_callers(&dir, Arc::default());
    let callee = at("callee", "x");
    let a = at("asker", "a");
    make(&host, &[&callee, &a]).await;
    host.stop(&callee, ShutdownKind::Sleep).await.unwrap();
    assert!(!host.is_running(&callee));
    let outcome = ask(
        &host,
        &a,
        &callee,
        &Who { claimed: None },
        Duration::from_secs(10),
    )
    .await;
    host.shutdown().await.unwrap();
    assert_eq!(
        outcome.reply::<Option<Caller>>(),
        Some(Caller::Actor(a))
    );
}

/// An actor's call fails at its deadline, neither before it nor
/// long after.
#[tokio::test(flavor = "multi_thread")]
async fn an_actors_call_fails_at_its_deadline() {
    let dir = Scratch::new("call-deadline");
    let host = open_callers(&dir, Arc::default());
    let callee = at("callee", "x");
    let a = at("asker", "a");
    make(&host, &[&callee, &a]).await;
    let outcome = ask(
        &host,
        &a,
        &callee,
        &Hold { ms: 5_000 },
        Duration::from_millis(300),
    )
    .await;
    host.shutdown().await.unwrap();
    assert_eq!(
        outcome.error.as_deref(),
        Some("actor.action_timed_out"),
        "{outcome:?}"
    );
    assert!((300..2_000).contains(&outcome.ms), "{outcome:?}");
}

/// When the host begins to shut down, a call whose action runs
/// fails at once with `actor.stopping`, the embedder's and an
/// actor's, though the callee's stop takes seconds. Each call is
/// awaited on a task of its own, outside any handler.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_fails_at_once_when_its_host_begins_to_shut_down() {
    let dir = Scratch::new("call-shutdown");
    let contexts = Arc::new(Contexts::default());
    let holds = Holds::new();
    let host = open_callers_with(
        &dir,
        HostOptions::default()
            .with_value(contexts.clone())
            .with_value(holds.clone()),
    );
    let raw = at("raw", "x");
    make(&host, &[&raw, &at("asker", "a")]).await;
    let ctx = contexts.0.lock().unwrap()[0].clone();
    let hold = |from: Option<ActorContext>| {
        let host = host.clone();
        let raw = raw.clone();
        tokio::spawn(async move {
            let deadline = Duration::from_secs(60);
            let result = match from {
                None => {
                    host.call(&raw, "hold", Vec::new(), deadline)
                        .await
                }
                Some(ctx) => {
                    async {
                        HostRef::of(&ctx)?
                            .call(&raw, "hold", Vec::new(), deadline)
                            .await
                    }
                    .await
                }
            };
            (result.map_err(|error| code(&error)), Instant::now())
        })
    };
    let calls = [hold(None), hold(Some(ctx))];
    holds.began(2).await;
    let begun = Instant::now();
    let shutdown = tokio::spawn({
        let host = host.clone();
        async move { host.shutdown().await }
    });
    for call in calls {
        let (result, at) = call.await.unwrap();
        assert_eq!(result, Err("actor.stopping".to_owned()));
        let after = at.duration_since(begun);
        assert!(after < Duration::from_millis(300), "{after:?}");
    }
    shutdown.await.unwrap().unwrap();
}

/// While one handler of an actor waits on its call, another
/// handler of that actor answers.
#[tokio::test(flavor = "multi_thread")]
async fn a_handler_that_calls_does_not_hold_up_its_actor() {
    let dir = Scratch::new("call-concurrent");
    let holds = Holds::new();
    let host = open_callers_with(
        &dir,
        HostOptions::default().with_value(holds.clone()),
    );
    let callee = at("callee", "x");
    let a = at("asker", "a");
    make(&host, &[&callee, &a]).await;
    let waiting = tokio::spawn({
        let host = host.clone();
        let (a, callee) = (a.clone(), callee.clone());
        async move {
            ask(
                &host,
                &a,
                &callee,
                &Hold { ms: 5_000 },
                Duration::from_secs(10),
            )
            .await
        }
    });
    holds.began(1).await;
    let ping = Outcome::of(async {
        call(&host, &a, &Ping).await.map(|()| Vec::new())
    })
    .await;
    assert!(!waiting.is_finished());
    waiting.abort();
    host.shutdown().await.unwrap();
    assert_eq!(ping.error, None, "{ping:?}");
    assert!(ping.ms < 1_000, "{ping:?}");
}

/// Calls itself from its `create`, and keeps how that went in its
/// host's [`Outcomes`].
struct SelfCaller;

/// How the calls of the [`SelfCaller`]s went, in order.
#[derive(Default)]
struct Outcomes(Mutex<Vec<Outcome>>);

#[async_trait::async_trait]
impl Actor for SelfCaller {
    type Input = ();
    type Actions = (Ping,);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        let host = HostRef::of(ctx.inner())?;
        let outcome = Outcome::of(host.call(
            host.own_address(),
            Ping::NAME,
            hamlet_actor::action::encode_positional(&Ping)?,
            Duration::from_secs(30),
        ))
        .await;
        HostRef::of(ctx.inner())?
            .value::<Outcomes>()
            .context("the host holds no outcomes")?
            .0
            .lock()
            .unwrap()
            .push(outcome);
        Ok(Self)
    }
}

impl Handler<Ping> for SelfCaller {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Ping,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

/// An actor that calls itself, here from its `create`, where a
/// wait for itself would last until the call's deadline, fails at
/// once with `actor.invalid_operation`, and its start goes on.
#[tokio::test(flavor = "multi_thread")]
async fn an_actor_that_calls_itself_fails_at_once() {
    let dir = Scratch::new("call-self");
    let outcomes = Arc::new(Outcomes::default());
    let mut registry = Registry::new();
    kind::<SelfCaller>(&mut registry, "self");
    let host = Host::open_with(
        &dir,
        registry.into_factories(),
        HostOptions::default().with_value(outcomes.clone()),
    )
    .unwrap();
    let started = tokio::time::timeout(
        Duration::from_secs(5),
        host.get_or_start(&at("self", "a")),
    )
    .await;
    let ping = call(&host, &at("self", "a"), &Ping).await;
    host.shutdown().await.unwrap();
    started.expect("the start waited for its own call").unwrap();
    ping.unwrap();
    let outcomes = outcomes.0.lock().unwrap();
    assert_eq!(
        outcomes[0].error.as_deref(),
        Some("actor.invalid_operation"),
        "{outcomes:?}"
    );
    assert!(outcomes[0].ms < 1_000, "{outcomes:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_generation_calls_and_makes_nothing() {
    let dir = Scratch::new("call-ended");
    let contexts = Arc::new(Contexts::default());
    let host = open_callers(&dir, contexts.clone());
    let callee = at("callee", "x");
    let a = at("asker", "a");
    make(&host, &[&callee, &a]).await;
    host.stop(&a, ShutdownKind::Sleep).await.unwrap();
    make(&host, &[&a]).await;
    host.stop(&a, ShutdownKind::Destroy).await.unwrap();
    make(&host, &[&a]).await;
    let contexts = contexts.0.lock().unwrap().clone();
    let mut outcomes = Vec::new();
    for ctx in &contexts {
        let args = hamlet_actor::action::encode_positional(&Who {
            claimed: None,
        })
        .unwrap();
        outcomes.push(
            Outcome::of(async {
                HostRef::of(ctx)?
                    .call(
                        &callee,
                        Who::NAME,
                        args,
                        Duration::from_secs(10),
                    )
                    .await
            })
            .await,
        );
    }
    let mut made = Vec::new();
    for ctx in &contexts[..2] {
        let view = HostRef::of(ctx).unwrap();
        assert_eq!(view.own_address(), &a);
        made.push(view.create(&at("callee", "y"), Vec::new()).await);
        made.push(view.get_or_start(&at("callee", "z")).await);
    }
    host.shutdown().await.unwrap();
    for ended in &made {
        let error = ended.as_ref().unwrap_err();
        assert_eq!(code(error), "actor.stopping", "{error:#}");
    }
    assert!(!any_file_of(&dir, "callee", "y"));
    assert!(!any_file_of(&dir, "callee", "z"));
    assert_eq!(outcomes.len(), 3);
    for ended in &outcomes[..2] {
        assert_eq!(
            ended.error.as_deref(),
            Some("actor.stopping"),
            "{outcomes:?}"
        );
        assert!(ended.ms < 1_000, "{outcomes:?}");
    }
    assert_eq!(
        outcomes[2].reply::<Option<Caller>>(),
        Some(Caller::Actor(a))
    );
}

/// An actor's call to a kind the host does not hold fails at once.
#[tokio::test(flavor = "multi_thread")]
async fn an_actors_call_to_an_unknown_kind_fails_at_once() {
    let dir = Scratch::new("call-unknown");
    let host = open_callers(&dir, Arc::default());
    let a = at("asker", "a");
    make(&host, &[&a]).await;
    let outcome = ask(
        &host,
        &a,
        &at("nobody", "x"),
        &Ping,
        Duration::from_secs(30),
    )
    .await;
    host.shutdown().await.unwrap();
    assert_eq!(
        outcome.error.as_deref(),
        Some("actor.not_registered"),
        "{outcome:?}"
    );
    assert!(outcome.ms < 1_000, "{outcome:?}");
}

/// Declares connection parameters that cannot hold a call's stamp.
struct Unstamped;

#[async_trait::async_trait]
impl Actor for Unstamped {
    type Input = ();
    type Actions = (Ping,);
    type Events = ();
    type ConnParams = ();
    type Action = ();

    async fn create(_: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self)
    }
}

impl Handler<Ping> for Unstamped {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Ping,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

/// A kind whose connection parameters cannot hold the stamp of
/// an actor's call or a client's would refuse every such call. The
/// host refuses it when it opens, naming it and what to declare, and
/// leaves the folder as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_kind_that_cannot_read_who_calls_it_is_refused_at_open() {
    let dir = Scratch::new("call-unstamped");
    let mut registry = Registry::new();
    kind::<Callee>(&mut registry, "callee");
    kind::<Unstamped>(&mut registry, "unstamped");
    let error =
        Host::open(&dir, registry.into_factories()).err().expect(
            "the host opened with a kind that cannot read its caller",
        );
    let text = format!("{error:#}");
    assert!(text.contains("`unstamped`"), "{text}");
    assert!(
        text.contains(
            "type ConnParams = Option<hamlet_host::Caller>"
        ),
        "{text}"
    );
    assert!(!text.contains("`callee`"), "{text}");
    assert!(!dir.join("actors").exists());
}

/// The embedder's call and an actor's call to an actor that was
/// to be made the same way fail.
async fn calls_to(
    host: &Host,
    from: &Address,
    to: &Address,
) -> [Outcome; 2] {
    let who = Who { claimed: None };
    let args = hamlet_actor::action::encode_positional(&who).unwrap();
    let embedder = Outcome::of(host.call(
        to,
        Who::NAME,
        args,
        Duration::from_secs(10),
    ))
    .await;
    let actor =
        ask(host, from, to, &who, Duration::from_secs(10)).await;
    [embedder, actor]
}

/// A call, the embedder's or an actor's, to a key that was never
/// made fails with `actor.not_found` and makes no file.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_to_a_key_never_made_fails_and_leaves_no_file() {
    let dir = Scratch::new("call-never-made");
    let host = open_callers(&dir, Arc::default());
    let a = at("asker", "a");
    make(&host, &[&a]).await;
    let never = at("callee", "never");
    let outcomes = calls_to(&host, &a, &never).await;
    host.shutdown().await.unwrap();
    for outcome in &outcomes {
        assert_eq!(
            outcome.error.as_deref(),
            Some("actor.not_found"),
            "{outcomes:?}"
        );
    }
    assert!(!any_file_of(&dir, never.kind(), never.key()));
}

/// A call that comes after its callee was destroyed fails with
/// `actor.not_found` and does not bring it back: no file is left.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_after_a_destroy_fails_and_leaves_no_file() {
    let dir = Scratch::new("call-destroyed");
    let host = open_callers(&dir, Arc::default());
    let a = at("asker", "a");
    let callee = at("callee", "x");
    make(&host, &[&a, &callee]).await;
    host.stop(&callee, ShutdownKind::Destroy).await.unwrap();
    let outcomes = calls_to(&host, &a, &callee).await;
    host.shutdown().await.unwrap();
    for outcome in &outcomes {
        assert_eq!(
            outcome.error.as_deref(),
            Some("actor.not_found"),
            "{outcomes:?}"
        );
    }
    assert!(!any_file_of(&dir, callee.kind(), callee.key()));
}

/// A start that finds no actor does not hand that to the starts
/// that wait for it. A `get_or_start` of a fresh key, begun as a call
/// to it starts, makes it, whatever the call finds.
#[tokio::test(flavor = "multi_thread")]
async fn a_get_or_start_that_waits_for_a_calls_start_makes_the_actor()
{
    let dir = Scratch::new("call-then-make");
    let host = open_callers(&dir, Arc::default());
    for round in 0..100 {
        let key = format!("k{round}");
        let to = at("callee", &key);
        let args = hamlet_actor::action::encode_positional(&Who {
            claimed: None,
        })
        .unwrap();
        let (called, made) = tokio::join!(
            host.call(&to, Who::NAME, args, Duration::from_secs(10)),
            host.get_or_start(&to),
        );
        if let Err(error) = &called {
            assert_eq!(code(error), "actor.not_found", "{error:#}");
        }
        made.unwrap_or_else(|error| {
            panic!("round {round}: {error:#}")
        });
    }
    host.shutdown().await.unwrap();
}
