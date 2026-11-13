//! The explicit create of an actor: it makes the actor and gives it
//! its input, which the actor's kind reads once. A create of a key
//! that holds an actor fails with `actor.duplicate_key` and changes
//! nothing, and one whose destroy is unfinished brings none of the
//! old data back.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use hamlet_actor::{
    Action, Actor, ActorConfig, ColumnValue, Ctx, Handler, Registry,
};
use hamlet_actor_core::ShutdownKind;
use hamlet_host::{Address, Caller, Host, HostOptions, HostRef};
use hamlet_testing::Scratch;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use support::code;

mod support;

/// What the kind counts: the runs of `on_create`, which reads the
/// input, and the starts that reached `create`.
#[derive(Default)]
struct Counts {
    created: AtomicUsize,
    started: AtomicUsize,
}

/// Fails the next start of an actor, once.
struct FailNext(AtomicBool);

/// How long each destroy takes.
struct SlowDestroy(Duration);

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
            .value::<FailNext>()
            .is_some_and(|fail| fail.0.swap(false, Ordering::SeqCst));
        if fails {
            anyhow::bail!("this start fails");
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
            tokio::time::sleep(slow.0).await;
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

/// A host of the kind `made` on `dir`.
fn open(dir: &Scratch, options: HostOptions) -> Host {
    let mut registry = Registry::new();
    registry.register_actor_with::<Made>(
        "made",
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
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
    support::call(host, &Address::new("made", key), action).await
}

/// The error of `made`, which was to fail.
fn failed(made: anyhow::Result<String>) -> anyhow::Error {
    made.expect_err("the create was to fail")
}

/// K1: a create makes the actor and gives it its input, which its
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

/// K1, K3: a create of a key that holds an actor fails with
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

/// K3: a create of an actor asleep with a timer fails with
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

/// K1: of two creates of one key at once, one makes the actor, with
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

/// K2: the actor keeps its input, so a create whose first start fails
/// leaves an actor that the next start, a call's, creates from that
/// input. A second create of the key fails with
/// `actor.duplicate_key`.
#[tokio::test(flavor = "multi_thread")]
async fn a_create_whose_first_start_fails_keeps_its_input() {
    let dir = Scratch::new("create-fails");
    let made = Arc::new(Counts::default());
    let fail = Arc::new(FailNext(AtomicBool::new(true)));
    let host = open(
        &dir,
        HostOptions::default()
            .with_value(made.clone())
            .with_value(fail.clone()),
    );
    let first = failed(host.create("made", "a", cbor("first")).await);
    let input = call(&host, "a", &Input).await.unwrap();
    let notes = call(&host, "a", &Notes).await.unwrap();
    let again =
        failed(host.create("made", "a", cbor("second")).await);
    host.shutdown().await.unwrap();
    // The core reports the failed start as an internal error.
    assert!(!fail.0.load(Ordering::SeqCst), "the first start failed");
    assert_ne!(code(&first), "actor.duplicate_key", "{first:#}");
    assert_eq!(input, "first");
    assert_eq!(notes, ["first"]);
    assert_eq!(code(&again), "actor.duplicate_key", "{again:#}");
    assert_eq!(made.created.load(Ordering::SeqCst), 1);
}

/// K3, K4: a create of a key whose destroy is under way fails at once
/// with `actor.destroying`. Once the destroy ends, a create makes the
/// actor anew from its own input, and none of the old data comes
/// back.
#[tokio::test(flavor = "multi_thread")]
async fn a_create_waits_out_no_destroy_and_brings_no_old_data_back() {
    let dir = Scratch::new("create-destroying");
    let made = Arc::new(Counts::default());
    let host = open(
        &dir,
        HostOptions::default().with_value(made.clone()).with_value(
            Arc::new(SlowDestroy(Duration::from_secs(1))),
        ),
    );
    host.create("made", "a", cbor("old")).await.unwrap();
    let destroy = tokio::spawn({
        let host = host.clone();
        async move { host.stop("made/a", ShutdownKind::Destroy).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
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

/// K4: a create of a key whose file holds a destroy that a kill cut
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
