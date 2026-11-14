//! The explicit create of an actor: it makes the actor and gives it
//! its input, which the actor's kind reads once. A create of a key
//! that holds an actor fails with `actor.duplicate_key` and changes
//! nothing, and one whose destroy is unfinished brings none of the
//! old data back. An actor makes another as the program does.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use at::at;
use began::began;
use code::code;
use files::any_file_of;
use hamlet_actor::{
    Action, Actor, ActorConfig, ColumnValue, Ctx, Handler, Registry,
};
use hamlet_actor_core::{ActorContext, ShutdownKind};
use hamlet_actor_error::ActorError;
use hamlet_host::{Caller, Host, HostOptions, HostRef};
use hamlet_testing::Scratch;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

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

/// What the kind counts: the runs of `on_create`, which reads the
/// input, and the starts that reached `create`.
#[derive(Default)]
struct Counts {
    created: AtomicUsize,
    started: AtomicUsize,
}

/// Fails as many of the next starts as it counts.
struct FailStarts(Mutex<usize>);

/// How long each destroy takes. `began` is notified as each begins.
struct SlowDestroy {
    takes: Duration,
    began: Notify,
}

/// Holds each start: it is told when the start begins, and the start
/// then waits an hour.
struct HeldStart(Notify);

/// Writes its input as a note in `on_create`, and a note each time
/// its timer fires.
struct Made;

/// The actor's notes, oldest first.
#[derive(Serialize, Deserialize)]
struct Notes;

impl Action for Notes {
    type Output = Vec<String>;
    const NAME: &'static str = "notes";
}

/// The input the actor was made with.
#[derive(Serialize, Deserialize)]
struct Input;

impl Action for Input {
    type Output = String;
    const NAME: &'static str = "input";
}

/// Sets a timer that fires [`Fire`] in a second.
#[derive(Serialize, Deserialize)]
struct Arm;

impl Action for Arm {
    type Output = ();
    const NAME: &'static str = "arm";
}

/// Adds the note `fired`.
#[derive(Serialize, Deserialize)]
struct Fire;

impl Action for Fire {
    type Output = ();
    const NAME: &'static str = "fire";
}

fn counts(ctx: &Ctx<Made>) -> anyhow::Result<Arc<Counts>> {
    HostRef::of(ctx.inner())?
        .value::<Counts>()
        .context("the host holds no counts")
}

#[async_trait::async_trait]
impl Actor for Made {
    type Input = String;
    type Actions = (Notes, Input, Arm, Fire);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        let fails = HostRef::of(ctx.inner())?
            .value::<FailStarts>()
            .is_some_and(|fail| {
                let mut left = fail.0.lock().unwrap();
                let fails = *left > 0;
                *left = left.saturating_sub(1);
                fails
            });
        if fails {
            anyhow::bail!("this start fails");
        }
        if let Some(held) =
            HostRef::of(ctx.inner())?.value::<HeldStart>()
        {
            held.0.notify_one();
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
        counts(ctx)?.started.fetch_add(1, Ordering::SeqCst);
        Ok(Self)
    }

    async fn on_create(
        self: Arc<Self>,
        ctx: Ctx<Self>,
    ) -> anyhow::Result<()> {
        let input = ctx.input()?;
        ctx.sql()
            .exec("CREATE TABLE note (v TEXT NOT NULL)")
            .await?;
        ctx.sql()
            .exec(format!("INSERT INTO note VALUES ('{input}')"))
            .await?;
        counts(&ctx)?.created.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn on_destroy(
        self: Arc<Self>,
        ctx: Ctx<Self>,
    ) -> anyhow::Result<()> {
        let slow = HostRef::of(ctx.inner())?.value::<SlowDestroy>();
        if let Some(slow) = slow {
            slow.began.notify_one();
            tokio::time::sleep(slow.takes).await;
        }
        Ok(())
    }
}

impl Handler<Notes> for Made {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Notes,
    ) -> anyhow::Result<Vec<String>> {
        let read = ctx
            .sql()
            .query("SELECT v FROM note ORDER BY rowid", None)
            .await?;
        read.rows
            .into_iter()
            .map(|row| match row.as_slice() {
                [ColumnValue::Text(note)] => Ok(note.clone()),
                other => anyhow::bail!("a note reads {other:?}"),
            })
            .collect()
    }
}

impl Handler<Input> for Made {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Input,
    ) -> anyhow::Result<String> {
        ctx.input()
    }
}

impl Handler<Arm> for Made {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Arm,
    ) -> anyhow::Result<()> {
        ctx.schedule()
            .after(Duration::from_secs(1), Fire::NAME, &[])
            .await
            .map(drop)
    }
}

impl Handler<Fire> for Made {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Fire,
    ) -> anyhow::Result<()> {
        ctx.sql()
            .exec("INSERT INTO note VALUES ('fired')")
            .await
            .map(drop)
    }
}

/// Makes other actors, as itself.
struct Maker;

/// Makes the actor of `kind` and `key`: by a create with `input`, by
/// a get-or-start without one. Answers the failure's `group.code`.
#[derive(Serialize, Deserialize)]
struct Make {
    kind: String,
    key: String,
    input: Option<Vec<u8>>,
}

impl Action for Make {
    type Output = Option<String>;
    const NAME: &'static str = "make";
}

/// In a host that holds it, each maker makes itself in its `create`,
/// by a create and by a get-or-start, and keeps here how each went
/// and how long it took.
#[derive(Default)]
struct MakeSelf(Mutex<Vec<(Option<String>, Duration)>>);

/// The contexts of the makers a host that holds it started, in order.
#[derive(Default)]
struct Contexts(Mutex<Vec<ActorContext>>);

/// Makes the actor of `kind` and `key` from the actor of `ctx`, as
/// [`Make`] says.
async fn make(
    ctx: &ActorContext,
    kind: &str,
    key: &str,
    input: Option<Vec<u8>>,
) -> Option<String> {
    let made = match input {
        Some(input) => Host::create_from(ctx, kind, key, input).await,
        None => Host::get_or_start_from(ctx, kind, key).await,
    };
    made.err().map(|error| code(&error))
}

#[async_trait::async_trait]
impl Actor for Maker {
    type Input = ();
    type Actions = (Make,);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        let host = HostRef::of(ctx.inner())?;
        if let Some(contexts) = host.value::<Contexts>() {
            contexts.0.lock().unwrap().push(ctx.inner().clone());
        }
        if let Some(made) = host.value::<MakeSelf>() {
            let (kind, key) = ctx
                .inner()
                .actor_id()
                .split_once('/')
                .context("the actor id has no kind")?;
            for input in [Some(cbor("self")), None] {
                let begun = Instant::now();
                let outcome =
                    make(ctx.inner(), kind, key, input).await;
                made.0
                    .lock()
                    .unwrap()
                    .push((outcome, begun.elapsed()));
            }
        }
        Ok(Self)
    }
}

impl Handler<Make> for Maker {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        make_it: Make,
    ) -> anyhow::Result<Option<String>> {
        Ok(make(
            ctx.inner(),
            &make_it.kind,
            &make_it.key,
            make_it.input,
        )
        .await)
    }
}

/// A host of the kinds `made` and `maker` on `dir`.
fn open(dir: &Scratch, options: HostOptions) -> Host {
    let mut registry = Registry::new();
    let config = ActorConfig {
        has_database: true,
        ..ActorConfig::default()
    };
    registry.register_actor_with::<Made>("made", config.clone());
    registry.register_actor_with::<Maker>("maker", config);
    Host::open_with(dir, registry.into_factories(), options).unwrap()
}

/// The CBOR of `text`, as the kind reads its input.
fn cbor(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::into_writer(text, &mut bytes).unwrap();
    bytes
}

/// Calls `action` of the actor `made/<key>`, and decodes its answer.
async fn call<M: Action>(
    host: &Host,
    key: &str,
    action: &M,
) -> anyhow::Result<M::Output>
where
    M::Output: DeserializeOwned,
{
    call::call(host, &at("made", key), action).await
}

/// The error of `made`, which was to fail.
fn failed(made: anyhow::Result<String>) -> anyhow::Error {
    made.expect_err("the create was to fail")
}

/// A create makes the actor and gives it its input, which its
/// kind reads once: a start from its file after a sleep reads the
/// same input, and creates nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_create_makes_the_actor_with_its_input_read_once() {
    let dir = Scratch::new("create-input");
    let made = Arc::new(Counts::default());
    let host =
        open(&dir, HostOptions::default().with_value(made.clone()));
    let id = host.create("made", "a", cbor("first")).await.unwrap();
    let input = call(&host, "a", &Input).await.unwrap();
    host.stop("made/a", ShutdownKind::Sleep).await.unwrap();
    let notes = call(&host, "a", &Notes).await.unwrap();
    let again = call(&host, "a", &Input).await.unwrap();
    host.shutdown().await.unwrap();
    assert_eq!(id, "made/a");
    assert_eq!((input.as_str(), again.as_str()), ("first", "first"));
    assert_eq!(notes, ["first"]);
    assert_eq!(made.created.load(Ordering::SeqCst), 1);
    assert_eq!(made.started.load(Ordering::SeqCst), 2);
}

/// A create of a key that holds an actor fails with
/// `actor.duplicate_key`, the actor running or asleep, and changes
/// nothing: the actor keeps its input and its notes, and its kind
/// creates nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_create_of_a_key_that_holds_an_actor_fails_and_changes_nothing()
 {
    let dir = Scratch::new("create-duplicate");
    let made = Arc::new(Counts::default());
    let host =
        open(&dir, HostOptions::default().with_value(made.clone()));
    host.create("made", "a", cbor("first")).await.unwrap();
    let running =
        failed(host.create("made", "a", cbor("second")).await);
    host.stop("made/a", ShutdownKind::Sleep).await.unwrap();
    let asleep =
        failed(host.create("made", "a", cbor("third")).await);
    let notes = call(&host, "a", &Notes).await.unwrap();
    let input = call(&host, "a", &Input).await.unwrap();
    host.shutdown().await.unwrap();
    for error in [&running, &asleep] {
        assert_eq!(code(error), "actor.duplicate_key", "{error:#}");
    }
    assert_eq!(notes, ["first"]);
    assert_eq!(input, "first");
    assert_eq!(made.created.load(Ordering::SeqCst), 1);
}

/// A create of an actor asleep with a timer fails with
/// `actor.duplicate_key` and leaves the timer, which still fires and
/// starts the actor though nothing calls it.
#[tokio::test(flavor = "multi_thread")]
async fn a_create_of_an_actor_asleep_with_a_timer_leaves_the_timer() {
    let dir = Scratch::new("create-timer");
    let made = Arc::new(Counts::default());
    let host =
        open(&dir, HostOptions::default().with_value(made.clone()));
    host.create("made", "a", cbor("first")).await.unwrap();
    call(&host, "a", &Arm).await.unwrap();
    host.stop("made/a", ShutdownKind::Sleep).await.unwrap();
    let asleep =
        failed(host.create("made", "a", cbor("second")).await);
    let begun = Instant::now();
    while made.started.load(Ordering::SeqCst) < 2
        && begun.elapsed() < Duration::from_secs(5)
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let started = made.started.load(Ordering::SeqCst);
    let notes = call(&host, "a", &Notes).await.unwrap();
    host.shutdown().await.unwrap();
    assert_eq!(code(&asleep), "actor.duplicate_key", "{asleep:#}");
    assert_eq!(started, 2, "the timer started the actor");
    assert_eq!(notes, ["first", "fired"]);
}

/// Of two creates of one key at once, one makes the actor, with
/// its input, and the other fails with `actor.duplicate_key`.
#[tokio::test(flavor = "multi_thread")]
async fn of_two_creates_of_one_key_at_once_one_makes_the_actor() {
    let dir = Scratch::new("create-race");
    let made = Arc::new(Counts::default());
    let host =
        open(&dir, HostOptions::default().with_value(made.clone()));
    for round in 0..50 {
        let key = format!("k{round}");
        let (x, y) = tokio::join!(
            host.create("made", &key, cbor("x")),
            host.create("made", &key, cbor("y")),
        );
        let (maker, error) = match (x, y) {
            (Ok(_), Err(error)) => ("x", error),
            (Err(error), Ok(_)) => ("y", error),
            (x, y) => panic!("round {round}: {x:?} and {y:?}"),
        };
        assert_eq!(code(&error), "actor.duplicate_key", "{error:#}");
        let notes = call(&host, &key, &Notes).await.unwrap();
        assert_eq!(notes, [maker], "round {round}");
    }
    host.shutdown().await.unwrap();
    assert_eq!(made.created.load(Ordering::SeqCst), 50);
}

/// A create whose own first start fails leaves nothing of the actor:
/// no file, a call fails with `actor.not_found`, and the key is free
/// for a create that makes the actor from its own input. So for a
/// kind whose start fails once, and for an input the kind cannot
/// read, empty bytes included.
#[tokio::test(flavor = "multi_thread")]
async fn a_create_whose_first_start_fails_leaves_no_actor() {
    let dir = Scratch::new("create-fails");
    let made = Arc::new(Counts::default());
    let fail = Arc::new(FailStarts(Mutex::new(0)));
    let host = open(
        &dir,
        HostOptions::default()
            .with_value(made.clone())
            .with_value(fail.clone()),
    );
    for (key, input, fails) in [
        ("fails-once", cbor("first"), true),
        ("unreadable", vec![0xff], false),
        ("empty", Vec::new(), false),
    ] {
        *fail.0.lock().unwrap() = usize::from(fails);
        let first = failed(host.create("made", key, input).await);
        assert_ne!(code(&first), "actor.duplicate_key", "{first:#}");
        assert!(
            !any_file_of(&dir, "made", key),
            "{key}: a file is left"
        );
        let error = call(&host, key, &Input)
            .await
            .expect_err("a call to an actor whose create failed");
        assert_eq!(code(&error), "actor.not_found", "{error:#}");
        host.create("made", key, cbor("second")).await.unwrap();
        let input = call(&host, key, &Input).await.unwrap();
        let notes = call(&host, key, &Notes).await.unwrap();
        assert_eq!(input, "second", "{key}");
        assert_eq!(notes, ["second"], "{key}");
    }
    host.shutdown().await.unwrap();
    assert_eq!(*fail.0.lock().unwrap(), 0, "the start did not fail");
    assert_eq!(made.created.load(Ordering::SeqCst), 3);
}

/// Why a start failed, as the error a host answers it with carries
/// it: in the error's metadata, under `error`.
fn why(error: &anyhow::Error) -> String {
    ActorError::extract(error)
        .metadata()
        .and_then(|meta| meta["error"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A start that fails for a reason of its kind's own tells it to each
/// caller it fails: a create, and each call that began a start of the
/// actor or waited for one. The error is `core.internal_error`, whose
/// message hides the reason.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_start_tells_each_caller_why() {
    let dir = Scratch::new("create-why");
    let fail = Arc::new(FailStarts(Mutex::new(1)));
    let host = open(
        &dir,
        HostOptions::default()
            .with_value(Arc::new(Counts::default()))
            .with_value(fail.clone()),
    );
    let created = failed(host.create("made", "a", cbor("a")).await);
    host.create("made", "a", cbor("a")).await.unwrap();
    host.stop("made/a", ShutdownKind::Sleep).await.unwrap();
    *fail.0.lock().unwrap() = usize::MAX;
    let mut calls = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let host = host.clone();
        calls.spawn(async move { call(&host, "a", &Notes).await });
    }
    let calls = calls.join_all().await;
    host.shutdown().await.unwrap();
    assert_eq!(code(&created), "core.internal_error", "{created:#}");
    assert!(
        why(&created).contains("this start fails"),
        "{created:#}"
    );
    for called in calls {
        let error = called.expect_err("a call whose start failed");
        assert!(
            why(&error).contains("this start fails"),
            "{error:#}"
        );
    }
}

/// Of two creates of one key at once, one with an input the kind
/// cannot read, the other makes the actor, also when it waited for
/// the failed one's start: that start tells its waiters nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_create_that_fails_leaves_the_key_to_a_create_that_waited()
{
    let dir = Scratch::new("create-fails-race");
    let made = Arc::new(Counts::default());
    let host =
        open(&dir, HostOptions::default().with_value(made.clone()));
    for round in 0..50 {
        let key = format!("k{round}");
        let (bad, good) = tokio::join!(
            host.create("made", &key, vec![0xff]),
            host.create("made", &key, cbor("good")),
        );
        let bad = failed(bad);
        if let Err(error) = good {
            panic!("round {round}: {error:#}; the other: {bad:#}");
        }
        let notes = call(&host, &key, &Notes).await.unwrap();
        assert_eq!(notes, ["good"], "round {round}");
    }
    host.shutdown().await.unwrap();
    assert_eq!(made.created.load(Ordering::SeqCst), 50);
}

/// A create whose start a shutdown cuts off fails with
/// `actor.not_ready` and removes the actor it made, its files with
/// it, so a create in the next host makes the actor from its own
/// input.
#[tokio::test(flavor = "multi_thread")]
async fn a_create_a_shutdown_cuts_off_leaves_no_actor() {
    let dir = Scratch::new("create-shutdown");
    let made = Arc::new(Counts::default());
    let held = Arc::new(HeldStart(Notify::new()));
    let host = open(
        &dir,
        HostOptions::default()
            .with_value(made.clone())
            .with_value(held.clone()),
    );
    let create = tokio::spawn({
        let host = host.clone();
        async move { host.create("made", "a", cbor("cut")).await }
    });
    began("the held start", held.0.notified()).await;
    tokio::time::timeout(Duration::from_secs(10), host.shutdown())
        .await
        .expect("the shutdown waited for the held start")
        .unwrap();
    let cut = failed(
        tokio::time::timeout(Duration::from_secs(5), create)
            .await
            .expect("the create outlived the shutdown")
            .unwrap(),
    );
    let left = any_file_of(&dir, "made", "a");
    drop(host);

    let host =
        open(&dir, HostOptions::default().with_value(made.clone()));
    host.create("made", "a", cbor("next")).await.unwrap();
    let notes = call(&host, "a", &Notes).await.unwrap();
    host.shutdown().await.unwrap();
    assert_eq!(code(&cut), "actor.not_ready", "{cut:#}");
    assert!(!left, "the cut-off create left a file");
    assert_eq!(notes, ["next"]);
    assert_eq!(made.created.load(Ordering::SeqCst), 1);
}

/// A create of a key whose destroy is under way fails at once
/// with `actor.destroying`. Once the destroy ends, a create makes the
/// actor anew from its own input, and none of the old data comes
/// back.
#[tokio::test(flavor = "multi_thread")]
async fn a_create_waits_out_no_destroy_and_brings_no_old_data_back() {
    let dir = Scratch::new("create-destroying");
    let made = Arc::new(Counts::default());
    let slow = Arc::new(SlowDestroy {
        takes: Duration::from_secs(1),
        began: Notify::new(),
    });
    let host = open(
        &dir,
        HostOptions::default()
            .with_value(made.clone())
            .with_value(slow.clone()),
    );
    host.create("made", "a", cbor("old")).await.unwrap();
    let destroy = tokio::spawn({
        let host = host.clone();
        async move { host.stop("made/a", ShutdownKind::Destroy).await }
    });
    began("the destroy", slow.began.notified()).await;
    let begun = Instant::now();
    let during = failed(host.create("made", "a", cbor("new")).await);
    let waited = begun.elapsed();
    destroy.await.unwrap().unwrap();
    host.create("made", "a", cbor("new")).await.unwrap();
    let notes = call(&host, "a", &Notes).await.unwrap();
    host.shutdown().await.unwrap();
    assert_eq!(code(&during), "actor.destroying", "{during:#}");
    assert!(waited < Duration::from_millis(500), "{waited:?}");
    assert_eq!(notes, ["new"]);
    assert_eq!(made.created.load(Ordering::SeqCst), 2);
}

/// A create of a key whose file holds a destroy that a kill cut
/// off finishes the destroy first, as boot would, and makes the actor
/// anew from its own input: none of the old data comes back.
#[tokio::test(flavor = "multi_thread")]
async fn a_create_finishes_a_destroy_a_kill_cut_off() {
    let dir = Scratch::new("create-marked");
    let made = Arc::new(Counts::default());
    let options = HostOptions::default().with_value(made.clone());
    let host = open(&dir, options.clone());
    host.create("made", "a", cbor("old")).await.unwrap();
    host.shutdown().await.unwrap();
    drop(host);
    // What a destroy leaves when a kill cuts it off after its mark.
    rusqlite::Connection::open(dir.join("actors/made/a.sqlite"))
        .unwrap()
        .execute("UPDATE _hamlet_actor SET destroying = 1", [])
        .unwrap();

    let host = open(&dir, options);
    host.create("made", "a", cbor("new")).await.unwrap();
    let notes = call(&host, "a", &Notes).await.unwrap();
    host.shutdown().await.unwrap();
    assert_eq!(notes, ["new"]);
    assert_eq!(made.created.load(Ordering::SeqCst), 2);
}

/// Has the maker `maker/<from>` make the actor of `kind` and `key`,
/// as [`Make`] says.
async fn ask(
    host: &Host,
    from: &str,
    kind: &str,
    key: &str,
    input: Option<Vec<u8>>,
) -> Option<String> {
    let make = Make {
        kind: kind.to_owned(),
        key: key.to_owned(),
        input,
    };
    call::call(host, &at("maker", from), &make).await.unwrap()
}

/// An actor makes another as the program does: a create gives the
/// input, which the made actor reads once, a second create of the key
/// fails with `actor.duplicate_key` and changes nothing, and a
/// get-or-start makes a fresh key and changes nothing of one that
/// holds an actor.
#[tokio::test(flavor = "multi_thread")]
async fn an_actor_makes_another_as_the_program_does() {
    let dir = Scratch::new("create-from");
    let made = Arc::new(Counts::default());
    let host =
        open(&dir, HostOptions::default().with_value(made.clone()));
    host.get_or_start("maker", "m").await.unwrap();
    let created =
        ask(&host, "m", "made", "b", Some(cbor("from m"))).await;
    let again =
        ask(&host, "m", "made", "b", Some(cbor("again"))).await;
    let fetched = ask(&host, "m", "made", "b", None).await;
    let fresh = ask(&host, "m", "maker", "n", None).await;
    let notes = call(&host, "b", &Notes).await.unwrap();
    let input = call(&host, "b", &Input).await.unwrap();
    let n_runs = host.is_running("maker/n");
    host.shutdown().await.unwrap();
    assert_eq!(created, None);
    assert_eq!(again.as_deref(), Some("actor.duplicate_key"));
    assert_eq!((fetched, fresh), (None, None));
    assert_eq!(notes, ["from m"]);
    assert_eq!(input, "from m");
    assert!(n_runs, "the get-or-start made maker/n");
    assert_eq!(made.created.load(Ordering::SeqCst), 1);
}

/// An actor that makes itself, here from its `create`, where a wait
/// for its own start would last until the start's deadline, fails at
/// once with `actor.invalid_operation`, by a create and by a
/// get-or-start, and its start goes on.
#[tokio::test(flavor = "multi_thread")]
async fn an_actor_that_makes_itself_fails_at_once() {
    let dir = Scratch::new("create-self");
    let outcomes = Arc::new(MakeSelf::default());
    let host = open(
        &dir,
        HostOptions::default().with_value(outcomes.clone()),
    );
    let started = tokio::time::timeout(
        Duration::from_secs(5),
        host.get_or_start("maker", "s"),
    )
    .await;
    host.shutdown().await.unwrap();
    started.expect("the start waited for itself").unwrap();
    let outcomes = outcomes.0.lock().unwrap();
    assert_eq!(outcomes.len(), 2, "{outcomes:?}");
    for (error, took) in outcomes.iter() {
        assert_eq!(
            error.as_deref(),
            Some("actor.invalid_operation"),
            "{outcomes:?}"
        );
        assert!(*took < Duration::from_secs(1), "{outcomes:?}");
    }
}

/// The context of a generation that has ended makes nothing, by a
/// create or a get-or-start. The running generation's makes as the
/// actor.
#[tokio::test(flavor = "multi_thread")]
async fn an_ended_generation_makes_nothing() {
    let dir = Scratch::new("create-ended");
    let made = Arc::new(Counts::default());
    let contexts = Arc::new(Contexts::default());
    let host = open(
        &dir,
        HostOptions::default()
            .with_value(made.clone())
            .with_value(contexts.clone()),
    );
    host.get_or_start("maker", "m").await.unwrap();
    host.stop("maker/m", ShutdownKind::Sleep).await.unwrap();
    host.get_or_start("maker", "m").await.unwrap();
    let contexts = contexts.0.lock().unwrap().clone();
    let mut outcomes = Vec::new();
    for (n, ctx) in contexts.iter().enumerate() {
        let key = format!("k{n}");
        outcomes.push((
            make(ctx, "made", &key, Some(cbor("x"))).await,
            make(ctx, "maker", &key, None).await,
        ));
    }
    let ended_made = dir.join("actors/maker/k0.sqlite").exists();
    host.shutdown().await.unwrap();
    let stopping = Some("actor.stopping".to_owned());
    assert_eq!(
        outcomes,
        [(stopping.clone(), stopping), (None, None)],
        "{outcomes:?}"
    );
    assert!(!ended_made, "the ended generation made maker/k0");
    assert_eq!(made.created.load(Ordering::SeqCst), 1);
}

/// Another program's connection to the file at `path`, which holds
/// its write lock from [`hold`] until `release` ends it.
struct Holder {
    release: std::sync::mpsc::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

/// Opens the file at `path`, made if it is missing, and holds its
/// write lock in a thread of its own until [`Holder::release`].
fn hold(path: &std::path::Path) -> Holder {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let holder = rusqlite::Connection::open(path).unwrap();
    holder
        .execute_batch(
            "PRAGMA journal_mode = WAL; CREATE TABLE IF NOT EXISTS \
             held (x); BEGIN EXCLUSIVE; INSERT INTO held VALUES (1);",
        )
        .unwrap();
    let (release, released) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let _ = released.recv();
        holder.execute_batch("ROLLBACK").unwrap();
    });
    Holder { release, thread }
}

impl Holder {
    /// Lets the file go, then waits long enough for a start that
    /// waited for it to open it and write, as SQLite tries again
    /// every 100 ms at most.
    async fn release(self) {
        self.release.send(()).unwrap();
        self.thread.join().unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
}

/// The actor's row in the file at `path`, read with its own
/// connection: its generation and its running mark, or `None` for a
/// file with no actor.
fn stored_actor(path: &std::path::Path) -> Option<(i64, i64)> {
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let has_table = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = \
             '_hamlet_actor'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    if has_table == 0 {
        return None;
    }
    conn.query_row(
        "SELECT generation, running FROM _hamlet_actor",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .ok()
}

/// A create whose file another program holds past the start deadline
/// fails with `actor.not_ready`. When the program lets the file go,
/// the given-up start writes nothing to it, so the key holds no
/// actor, and the next create makes the actor from its own input.
#[tokio::test(flavor = "multi_thread")]
async fn a_create_given_up_while_its_file_is_held_leaves_no_actor() {
    let dir = Scratch::new("create-held-file");
    let path = dir.join("actors/made/a.sqlite");
    let made = Arc::new(Counts::default());
    let mut options = HostOptions::default().with_value(made.clone());
    options.start_deadline = Duration::from_millis(300);
    let host = open(&dir, options);
    let holder = hold(&path);
    let first = failed(host.create("made", "a", cbor("first")).await);
    holder.release().await;
    let left = stored_actor(&path);
    let second = host.create("made", "a", cbor("second")).await;
    let input = call(&host, "a", &Input).await;
    host.shutdown().await.unwrap();
    assert_eq!(code(&first), "actor.not_ready", "{first:#}");
    assert_eq!(left, None, "the given-up start wrote its actor");
    second.unwrap();
    assert_eq!(input.unwrap(), "second");
    assert_eq!(made.created.load(Ordering::SeqCst), 1);
}

/// A start of an actor asleep in its file, which another program
/// holds past the start deadline, fails with `actor.not_ready`. When
/// the program lets the file go, the given-up start writes nothing:
/// the actor's generation and its running mark are as its last stop
/// left them, so no boot starts it.
#[tokio::test(flavor = "multi_thread")]
async fn a_start_given_up_while_its_file_is_held_writes_nothing() {
    let dir = Scratch::new("start-held-file");
    let path = dir.join("actors/made/a.sqlite");
    let made = Arc::new(Counts::default());
    let host =
        open(&dir, HostOptions::default().with_value(made.clone()));
    host.create("made", "a", cbor("first")).await.unwrap();
    host.shutdown().await.unwrap();
    drop(host);
    let slept = stored_actor(&path);

    let mut options = HostOptions::default().with_value(made.clone());
    options.start_deadline = Duration::from_millis(300);
    let host = open(&dir, options);
    let holder = hold(&path);
    let started = host
        .get_or_start("made", "a")
        .await
        .expect_err("a start of a held file");
    holder.release().await;
    let left = stored_actor(&path);
    host.shutdown().await.unwrap();
    assert_eq!(code(&started), "actor.not_ready", "{started:#}");
    assert_eq!(slept.map(|(_, running)| running), Some(0));
    assert_eq!(left, slept, "the given-up start wrote to the file");
}

/// A shutdown ends a create whose file another program holds, and
/// returns while the program still holds it. The next host opens the
/// folder, and when the program lets the file go, the old host's
/// given-up start writes nothing to it: a create in the next host
/// makes the actor from its own input.
#[tokio::test(flavor = "multi_thread")]
async fn a_shutdown_returns_while_a_start_waits_for_a_held_file() {
    let dir = Scratch::new("shutdown-held-file");
    let path = dir.join("actors/made/a.sqlite");
    let made = Arc::new(Counts::default());
    let host =
        open(&dir, HostOptions::default().with_value(made.clone()));
    let holder = hold(&path);
    let create = tokio::spawn({
        let host = host.clone();
        async move { host.create("made", "a", cbor("first")).await }
    });
    // Long enough for the create to reach its file.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!create.is_finished(), "the create ended at once");
    // Well under rusqlite's default 5 s busy timeout, after which the
    // open would give up by itself: a shutdown that waited for it
    // would still return before a longer bound.
    tokio::time::timeout(Duration::from_secs(3), host.shutdown())
        .await
        .expect("the shutdown waited for the held file")
        .unwrap();
    let first = failed(create.await.unwrap());
    drop(host);

    let host =
        open(&dir, HostOptions::default().with_value(made.clone()));
    holder.release().await;
    let left = stored_actor(&path);
    let second = host.create("made", "a", cbor("second")).await;
    let input = call(&host, "a", &Input).await;
    host.shutdown().await.unwrap();
    assert_eq!(code(&first), "actor.not_ready", "{first:#}");
    assert_eq!(left, None, "the old host's start wrote its actor");
    second.unwrap();
    assert_eq!(input.unwrap(), "second");
    assert_eq!(made.created.load(Ordering::SeqCst), 1);
}

/// Creates with no time to start, which the timer's tick lets a few
/// of make their actor, leave no file when they fail with
/// `actor.not_ready`, however their open and their start's deadline
/// fall: an open that claimed its write first made the actor, which
/// its start then removes; one its start gave up first makes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn creates_with_no_time_to_start_leave_no_file_when_they_fail()
{
    let dir = Scratch::new("create-no-time");
    let made = Arc::new(Counts::default());
    let mut options = HostOptions::default().with_value(made.clone());
    options.start_deadline = Duration::ZERO;
    let host = open(&dir, options);
    let mut outcomes = Vec::new();
    for n in 0..50 {
        let key = format!("k{n}");
        let created = host.create("made", &key, cbor("x")).await;
        outcomes.push((key, created.map_err(|error| code(&error))));
    }
    // Long enough for an open its start gave up to write, if it
    // would.
    tokio::time::sleep(Duration::from_millis(500)).await;
    host.shutdown().await.unwrap();
    let wrong = outcomes
        .into_iter()
        .filter(|(key, created)| match created {
            Ok(_) => !any_file_of(&dir, "made", key),
            Err(code) => {
                code != "actor.not_ready"
                    || any_file_of(&dir, "made", key)
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(wrong, Vec::new());
}
