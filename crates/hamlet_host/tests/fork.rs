//! A fork by file copy: the source copies its database with the
//! fork's id, the copy is synced and put in place, and the host
//! starts it like any actor. The copy's start applies the fork.

use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use act::act;
use anyhow::Result;
use async_trait::async_trait;
use at::at;
use clock::now_ms;
use hamlet_actor::{
    Action, Actor, ActorConfig, Ctx, Handler, Registry,
};
use hamlet_actor_core::{ActorForkStart, ColumnValue};
use hamlet_host::{Address, Caller, Host, HostRef};
use hamlet_testing::Scratch;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

#[path = "../examples/demo/act.rs"]
mod act;
#[path = "support/at.rs"]
mod at;
#[path = "support/clock.rs"]
mod clock;

const NOTES: i64 = 200;
const QUEUED: i64 = 3;

/// When the source's one-shot is due: after its copy has started.
const FIRE_AFTER: Duration = Duration::from_secs(2);

/// The copy whose first start fails once its hook has run.
const FAILS_ONCE: &str = "fails-once";

/// An actor that keeps notes. Made new, it writes [`NOTES`] notes,
/// queues [`QUEUED`] messages, arms a one-shot, sets a run wake an
/// hour ahead and holds a set-aside one-shot. `fork` copies it.
struct Notes;

#[derive(Serialize, Deserialize)]
struct Fork {
    path: String,
    fork_id: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct Fire;

#[derive(Serialize, Deserialize)]
struct Note;

#[derive(Serialize, Deserialize)]
struct Look;

/// What an actor holds, as `look` reads it.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Seen {
    id: String,
    notes: i64,
    queued: i64,
    hook_runs: i64,
    fired: i64,
    timers: i64,
    set_aside: i64,
    /// The actor id of each fork behind it, oldest first.
    chain: Vec<String>,
}

impl Action for Fork {
    type Output = ();
    const NAME: &'static str = "fork";
}

impl Action for Fire {
    type Output = ();
    const NAME: &'static str = "fire";
}

impl Action for Note {
    type Output = ();
    const NAME: &'static str = "note";
}

impl Action for Look {
    type Output = Seen;
    const NAME: &'static str = "look";
}

#[async_trait]
impl Actor for Notes {
    type Input = ();
    type Actions = (Fork, Fire, Note, Look);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(_: &Ctx<Self>) -> Result<Self> {
        Ok(Self)
    }

    async fn on_create(
        self: Arc<Self>,
        ctx: Ctx<Self>,
    ) -> Result<()> {
        ctx.sql()
            .exec(&format!(
                "CREATE TABLE notes (id INTEGER PRIMARY KEY, body \
                 TEXT NOT NULL); CREATE TABLE counts (name TEXT \
                 PRIMARY KEY, value INTEGER NOT NULL); WITH \
                 RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 \
                 FROM n WHERE i < {NOTES}) INSERT INTO notes SELECT \
                 i, 'entry ' || i FROM n; INSERT INTO \
                 _actor_schedule_set_aside (event_id, action, \
                 trigger_at, tries, set_aside_at) VALUES ('aside', \
                 'fire', 0, 5, 0)"
            ))
            .await?;
        for _ in 0..QUEUED {
            ctx.inner().send("work", b"next").await?;
        }
        ctx.schedule().after(FIRE_AFTER, Fire::NAME, &[]).await?;
        ctx.inner()
            .set_run_wake_at(Some(now_ms() + 3_600_000))
            .await?;
        Ok(())
    }

    async fn on_fork(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: ActorForkStart,
    ) -> Result<()> {
        add_one(&ctx, "hook_runs").await
    }

    async fn on_start(self: Arc<Self>, ctx: Ctx<Self>) -> Result<()> {
        let host = HostRef::of(ctx.inner())?;
        if host.own_address().key() == FAILS_ONCE
            && count(&ctx, "hook_runs").await? == 1
        {
            anyhow::bail!("the start after the first hook run fails");
        }
        Ok(())
    }
}

impl Handler<Fork> for Notes {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        fork: Fork,
    ) -> Result<()> {
        ctx.inner().copy_for_fork(&fork.path, fork.fork_id).await
    }
}

impl Handler<Fire> for Notes {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Fire,
    ) -> Result<()> {
        add_one(&ctx, "fired").await
    }
}

impl Handler<Note> for Notes {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Note,
    ) -> Result<()> {
        ctx.sql()
            .exec(
                "INSERT INTO notes (body) VALUES ('the copy''s own')",
            )
            .await?;
        Ok(())
    }
}

impl Handler<Look> for Notes {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Look,
    ) -> Result<Seen> {
        Ok(Seen {
            id: ctx.actor_id().to_owned(),
            notes: read(&ctx, "SELECT COUNT(*) FROM notes").await?,
            queued: read(&ctx, "SELECT COUNT(*) FROM _actor_queue")
                .await?,
            hook_runs: count(&ctx, "hook_runs").await?,
            fired: count(&ctx, "fired").await?,
            timers: read(
                &ctx,
                "SELECT COUNT(*) FROM _actor_schedule_events",
            )
            .await?,
            set_aside: read(
                &ctx,
                "SELECT COUNT(*) FROM _actor_schedule_set_aside",
            )
            .await?,
            chain: ctx
                .fork_chain()
                .into_iter()
                .map(|link| link.actor_id)
                .collect(),
        })
    }
}

async fn read(ctx: &Ctx<Notes>, sql: &str) -> Result<i64> {
    let result = ctx.sql().query(sql, None).await?;
    match result.rows.first().and_then(|row| row.first()) {
        Some(ColumnValue::Integer(value)) => Ok(*value),
        other => anyhow::bail!("{sql} read {other:?}"),
    }
}

async fn count(ctx: &Ctx<Notes>, name: &str) -> Result<i64> {
    read(
        ctx,
        &format!(
            "SELECT COALESCE((SELECT value FROM counts WHERE name = \
             '{name}'), 0)"
        ),
    )
    .await
}

async fn add_one(ctx: &Ctx<Notes>, name: &str) -> Result<()> {
    ctx.sql()
        .exec(&format!(
            "INSERT INTO counts (name, value) VALUES ('{name}', 1) \
             ON CONFLICT (name) DO UPDATE SET value = value + 1"
        ))
        .await?;
    Ok(())
}

fn open(dir: &Path) -> Host {
    let mut registry = Registry::new();
    registry.register_actor_with::<Notes>(
        "notes",
        ActorConfig {
            name: Some("notes".to_owned()),
            has_database: true,
            ..ActorConfig::default()
        },
    );
    Host::open(dir, registry.into_factories()).unwrap()
}

async fn look(host: &Host, at: &Address) -> Seen {
    let bytes = act(host, at, Look::NAME, Vec::new()).await.unwrap();
    ciborium::from_reader(bytes.as_slice()).unwrap()
}

/// The steps of a fork of `source` into `copy`, each of which may run
/// again.
struct Steps {
    source: Address,
    folder: PathBuf,
    temp: PathBuf,
    file: PathBuf,
}

impl Steps {
    fn new(dir: &Path, source: &Address, copy: &Address) -> Self {
        let folder = dir.join("actors").join(copy.kind());
        Self {
            source: source.clone(),
            temp: folder.join(format!("{}.fork", copy.key())),
            file: folder.join(format!("{}.sqlite", copy.key())),
            folder,
        }
    }

    /// The source copies its database under the temporary name with
    /// the fork's id. A copy left there before is in flight, so it
    /// goes first.
    async fn copy(&self, host: &Host, fork_id: &[u8]) {
        remove_if_present(&self.temp);
        let mut args = Vec::new();
        ciborium::into_writer(
            &Fork {
                path: self.temp.display().to_string(),
                fork_id: fork_id.to_vec(),
            },
            &mut args,
        )
        .unwrap();
        act(host, &self.source, Fork::NAME, args).await.unwrap();
    }

    fn sync(&self) {
        if self.temp.exists() {
            File::open(&self.temp).unwrap().sync_all().unwrap();
        }
    }

    /// Puts the copy in place without replacing a copy already
    /// there, which may be running.
    fn place(&self) {
        if !self.temp.exists() {
            return;
        }
        match fs::hard_link(&self.temp, &self.file) {
            Ok(()) => {}
            Err(error)
                if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => panic!("place the copy: {error}"),
        }
        remove_if_present(&self.temp);
        File::open(&self.folder).unwrap().sync_all().unwrap();
    }

    /// The timers the copy holds before its start: one-shots,
    /// set-aside one-shots and a run wake.
    fn carried(&self) -> (i64, i64, bool) {
        let conn = rusqlite::Connection::open_with_flags(
            &self.temp,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let read = |sql: &str| -> i64 {
            conn.query_row(sql, [], |row| row.get(0)).unwrap()
        };
        (
            read("SELECT COUNT(*) FROM _actor_schedule_events"),
            read("SELECT COUNT(*) FROM _actor_schedule_set_aside"),
            read(
                "SELECT COUNT(*) FROM _actor_meta WHERE key = \
                 'run_wake_at'",
            ) == 1,
        )
    }
}

fn remove_if_present(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => panic!("remove {}: {error}", path.display()),
    }
}

/// The deadline the actor's last stop left, read with its file
/// closed.
fn wake_at(dir: &Path, at: &Address) -> Option<i64> {
    rusqlite::Connection::open_with_flags(
        dir.join(format!("actors/{at}.sqlite")),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
    .query_row("SELECT wake_at FROM _hamlet_actor", [], |row| {
        row.get(0)
    })
    .unwrap()
}

/// The names of the files in the copy's folder.
fn files(steps: &Steps) -> Vec<String> {
    let mut names = fs::read_dir(&steps.folder)
        .unwrap()
        .map(|entry| {
            entry.unwrap().file_name().into_string().unwrap()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// A copy starts with its source's notes and queued messages, runs
/// its hook once with the source as its fork, and holds none of its
/// source's timers: the one-shot fires in the source alone, and the
/// copy's stop leaves no deadline.
#[tokio::test(flavor = "multi_thread")]
async fn a_copy_starts_with_its_sources_data_and_none_of_its_timers()
{
    let dir = Scratch::new("fork-copy");
    let host = open(&dir);
    let source = at("notes", "source");
    let copy = at("notes", "copy");
    host.get_or_start(&source).await.unwrap();
    let steps = Steps::new(&dir, &source, &copy);
    steps.copy(&host, b"fork-1").await;
    assert_eq!(steps.carried(), (1, 1, true));
    steps.sync();
    steps.place();
    host.get_or_start(&copy).await.unwrap();

    let from = look(&host, &source).await;
    let seen = look(&host, &copy).await;
    assert_eq!(
        seen,
        Seen {
            id: seen.id.clone(),
            notes: NOTES,
            queued: QUEUED,
            hook_runs: 1,
            fired: 0,
            timers: 0,
            set_aside: 0,
            chain: vec![from.id],
        }
    );
    for _ in 0..3_000 {
        if look(&host, &source).await.fired == 1 {
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(look(&host, &source).await.fired, 1);
    assert_eq!(look(&host, &copy).await.fired, 0);
    host.shutdown().await.unwrap();
    assert!(wake_at(&dir, &source).is_some());
    assert_eq!(wake_at(&dir, &copy), None);
}

/// Every step of the fork run twice, a boot between putting the copy
/// in place and starting it, and the copy and its placing run again
/// once the copy has started, leave one copy whose hook ran once and
/// whose own writes are kept.
#[tokio::test(flavor = "multi_thread")]
async fn each_step_twice_leaves_one_copy_whose_hook_ran_once() {
    let dir = Scratch::new("fork-twice");
    let host = open(&dir);
    let source = at("notes", "source");
    let copy = at("notes", "copy");
    host.get_or_start(&source).await.unwrap();
    let steps = Steps::new(&dir, &source, &copy);
    for _ in 0..2 {
        steps.copy(&host, b"fork-1").await;
    }
    for _ in 0..2 {
        steps.sync();
    }
    for _ in 0..2 {
        steps.place();
    }
    host.shutdown().await.unwrap();
    drop(host);

    let host = open(&dir);
    assert!(host.boot().await.unwrap().contains(&copy));
    for _ in 0..2 {
        host.get_or_start(&copy).await.unwrap();
    }
    host.get_or_start(&source).await.unwrap();
    steps.copy(&host, b"fork-1").await;
    steps.sync();
    steps.place();
    act(&host, &copy, Note::NAME, Vec::new()).await.unwrap();
    host.shutdown().await.unwrap();
    drop(host);

    let host = open(&dir);
    host.boot().await.unwrap();
    let seen = look(&host, &copy).await;
    assert_eq!(
        (seen.hook_runs, seen.notes, seen.chain.len()),
        (1, NOTES + 1, 1)
    );
    host.shutdown().await.unwrap();
    assert_eq!(
        files(&steps),
        [".lock", "copy.sqlite", "source.sqlite"]
    );
}

/// A start that fails after the hook ran and before the fork is
/// recorded runs the hook again at the next start, which records the
/// fork once; a later start does not run it.
#[tokio::test(flavor = "multi_thread")]
async fn a_start_cut_off_before_the_fork_is_recorded_runs_the_hook_again()
 {
    let dir = Scratch::new("fork-cut-off");
    let host = open(&dir);
    let source = at("notes", "source");
    let copy = at("notes", FAILS_ONCE);
    host.get_or_start(&source).await.unwrap();
    let steps = Steps::new(&dir, &source, &copy);
    steps.copy(&host, b"fork-1").await;
    steps.sync();
    steps.place();
    assert!(host.get_or_start(&copy).await.is_err());
    host.get_or_start(&copy).await.unwrap();
    host.shutdown().await.unwrap();
    drop(host);

    let host = open(&dir);
    host.get_or_start(&copy).await.unwrap();
    let seen = look(&host, &copy).await;
    assert_eq!((seen.hook_runs, seen.chain.len()), (2, 1));
    host.shutdown().await.unwrap();
}
