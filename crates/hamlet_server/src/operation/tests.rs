//! Operations and their markers, through a test-only operation that
//! spends from a purse its holder may use.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use hamlet_testing::Scratch;
use rusqlite::{Connection, params};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::json;

use super::*;
use crate::{Held, Home};

const CHILD_HOME: &str = "HAMLET_SERVER_TEST_HOME";
const CHILD_PAUSE: &str = "HAMLET_SERVER_TEST_PAUSE";

#[derive(Serialize)]
struct Spend {
    actor: &'static str,
    purse: &'static str,
    coins: i64,
}

/// [`Spend`]'s request under another kind.
#[derive(Serialize)]
#[serde(transparent)]
struct Give(Spend);

#[derive(Debug, PartialEq, Deserialize)]
struct Spent {
    left: i64,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Refused {
    NotHolder,
    Short,
}

impl Operation for Spend {
    const KIND: &'static str = "spend";
    type Done = Spent;
    type Refusal = Refused;

    fn authorize(&self, db: &Connection) -> Result<Option<Refused>> {
        let holds = db.query_row(
            "SELECT count(*) FROM holder WHERE purse = ?1 AND actor \
             = ?2",
            params![self.purse, self.actor],
            |row| row.get::<_, i64>(0),
        )?;
        Ok((holds == 0).then_some(Refused::NotHolder))
    }

    // Writes before it checks, so a refusal has something to undo.
    fn apply(
        &self,
        db: &Connection,
    ) -> Result<Outcome<Spent, Refused>> {
        db.execute(
            "INSERT INTO spending VALUES (?1, ?2)",
            params![self.purse, self.coins],
        )?;
        let changed = db.execute(
            "UPDATE purse SET coins = coins - ?2 WHERE name = ?1",
            params![self.purse, self.coins],
        )?;
        if changed == 0 {
            bail!("no purse {}", self.purse);
        }
        let left = db.query_row(
            "SELECT coins FROM purse WHERE name = ?1",
            [self.purse],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(if left < 0 {
            Outcome::Refused(Refused::Short)
        } else {
            Outcome::Done(Spent { left })
        })
    }
}

impl Operation for Give {
    const KIND: &'static str = "give";
    type Done = Spent;
    type Refusal = Refused;

    fn authorize(&self, _: &Connection) -> Result<Option<Refused>> {
        Ok(None)
    }

    fn apply(
        &self,
        _: &Connection,
    ) -> Result<Outcome<Spent, Refused>> {
        Ok(Outcome::Done(Spent { left: 0 }))
    }
}

// The marker is written after the change, so a child process paused
// here has made the change but not committed it.
impl Serialize for Spent {
    fn serialize<S: Serializer>(
        &self,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        pause_at("marker");
        let mut spent = s.serialize_struct("Spent", 1)?;
        spent.serialize_field("left", &self.left)?;
        spent.end()
    }
}

fn id(runtime: &str, key: &str) -> OperationId {
    OperationId {
        runtime: RuntimeInstance::new(runtime).unwrap(),
        key: key.to_owned(),
    }
}

fn spend(actor: &'static str, coins: i64) -> Spend {
    Spend {
        actor,
        purse: "well",
        coins,
    }
}

fn done(left: i64) -> Reply<Spent, Refused> {
    Reply::Outcome(Outcome::Done(Spent { left }))
}

fn refused(refusal: Refused) -> Reply<Spent, Refused> {
    Reply::Outcome(Outcome::Refused(refusal))
}

/// A home whose purse `well` holds 10 coins, held by `ada`.
fn seeded(tag: &str) -> (Scratch, Home) {
    let dir = Scratch::new(tag);
    let mut home = open(&dir);
    home.store()
        .connection()
        .execute_batch(
            "CREATE TABLE purse (name TEXT PRIMARY KEY, coins \
             INTEGER);
             CREATE TABLE holder (purse TEXT, actor TEXT);
             CREATE TABLE spending (purse TEXT, coins INTEGER);
             INSERT INTO purse VALUES ('well', 10);
             INSERT INTO holder VALUES ('well', 'ada');",
        )
        .unwrap();
    (dir, home)
}

fn count(home: &mut Home, sql: &str) -> i64 {
    home.store()
        .connection()
        .query_row(sql, [], |row| row.get::<_, i64>(0))
        .unwrap()
}

/// The coins left in `well`, the spendings and the markers.
fn state(home: &mut Home) -> (i64, i64, i64) {
    (
        count(home, "SELECT coins FROM purse WHERE name = 'well'"),
        count(home, "SELECT count(*) FROM spending"),
        count(home, "SELECT count(*) FROM marker"),
    )
}

fn set(home: &mut Home, sql: &str) {
    home.store().connection().execute_batch(sql).unwrap();
}

fn is_held(dir: &Path) -> bool {
    match Home::open(dir) {
        Ok(_) => false,
        Err(error) => error.downcast_ref::<Held>().is_some(),
    }
}

/// Opens `dir`, waiting briefly while it is still held.
fn open(dir: &Path) -> Home {
    // A child that another test spawns shares this process's sockets
    // until it execs, so a claim just closed can still answer.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match Home::open(dir) {
            Err(error)
                if error.downcast_ref::<Held>().is_some()
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            home => return home.unwrap(),
        }
    }
}

#[test]
fn a_replay_returns_the_marked_outcome_without_a_second_change() {
    let (_dir, mut home) = seeded("server-replay");
    let call = id("pi-1", "7");
    assert_eq!(
        home.store().run(&call, &spend("ada", 3)).unwrap(),
        done(7)
    );
    set(&mut home, "DELETE FROM holder");
    assert_eq!(
        home.store().run(&call, &spend("ada", 3)).unwrap(),
        done(7)
    );
    assert_eq!(state(&mut home), (7, 1, 1));
}

#[test]
fn a_changed_request_under_a_marked_identity_is_a_conflict() {
    let (_dir, mut home) = seeded("server-conflict");
    let call = id("pi-1", "7");
    home.store().run(&call, &spend("ada", 3)).unwrap();
    let changed = home.store().run(&call, &spend("ada", 4)).unwrap();
    assert_eq!(changed, Reply::Conflict);
    let other_kind =
        home.store().run(&call, &Give(spend("ada", 3))).unwrap();
    assert_eq!(other_kind, Reply::Conflict);
    assert_eq!(state(&mut home), (7, 1, 1));
    // The same key from another runtime is another operation.
    let elsewhere = id("pi-2", "7");
    let reply =
        home.store().run(&elsewhere, &spend("ada", 4)).unwrap();
    assert_eq!(reply, done(3));
}

#[test]
fn authority_is_checked_as_the_operation_runs_and_a_refusal_is_marked()
 {
    let (_dir, mut home) = seeded("server-authority");
    let call = id("pi-1", "7");
    let first = home.store().run(&call, &spend("bob", 3)).unwrap();
    assert_eq!(first, refused(Refused::NotHolder));
    set(&mut home, "INSERT INTO holder VALUES ('well', 'bob')");
    let again = home.store().run(&call, &spend("bob", 3)).unwrap();
    assert_eq!(again, refused(Refused::NotHolder));
    assert_eq!(state(&mut home), (10, 0, 1));
}

#[test]
fn a_refused_change_leaves_only_its_marker() {
    let (_dir, mut home) = seeded("server-refused");
    let reply = home.store().run(&id("pi-1", "7"), &spend("ada", 30));
    assert_eq!(reply.unwrap(), refused(Refused::Short));
    assert_eq!(state(&mut home), (10, 0, 1));
}

#[test]
fn a_failure_before_the_commit_leaves_nothing() {
    let (_dir, mut home) = seeded("server-failure");
    set(&mut home, "INSERT INTO holder VALUES ('dry', 'ada')");
    let call = id("pi-1", "7");
    let dry = Spend {
        actor: "ada",
        purse: "dry",
        coins: 3,
    };
    assert!(home.store().run(&call, &dry).is_err());
    assert_eq!(state(&mut home), (10, 0, 0));
    set(&mut home, "INSERT INTO purse VALUES ('dry', 5)");
    assert_eq!(home.store().run(&call, &dry).unwrap(), done(2));
    assert_eq!(count(&mut home, "SELECT count(*) FROM spending"), 1);
}

#[cfg(unix)]
#[test]
fn a_held_home_refuses_another_claim_by_any_spelling() {
    let scratch = Scratch::new("server-claim");
    let dir = scratch.join("home");
    let home = open(&dir);
    std::os::unix::fs::symlink(&dir, scratch.join("link")).unwrap();
    for spelling in [
        dir.clone(),
        scratch.join("link"),
        scratch.join("home/../home"),
        PathBuf::from(format!("{}/", dir.display())),
    ] {
        assert!(
            is_held(&spelling),
            "{} was not held",
            spelling.display()
        );
    }
    home.close().unwrap();
    open(&scratch.join("link"));
}

#[cfg(unix)]
#[test]
fn a_home_given_through_a_symlink_and_a_parent_is_its_real_folder() {
    let scratch = Scratch::new("server-parent-spelling");
    let a = scratch.join("a");
    let b = scratch.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(b.join("child")).unwrap();
    std::os::unix::fs::symlink(b.join("child"), a.join("link"))
        .unwrap();
    let spelling = a.join("link/..");
    let home = open(&spelling);
    assert_eq!(
        home.layout().root(),
        std::fs::canonicalize(&spelling).unwrap()
    );
    assert!(is_held(&b));
    // The CLI reads the home's files through the spelling it was
    // given.
    std::fs::write(home.layout().root().join("server.json"), "kept")
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(spelling.join("server.json"))
            .unwrap(),
        "kept"
    );
    home.close().unwrap();
}

#[test]
fn a_home_refuses_a_missing_or_substituted_store_or_identity() {
    let a = Scratch::new("server-home-a");
    let b = Scratch::new("server-home-b");
    open(&a).close().unwrap();
    open(&b).close().unwrap();
    std::fs::rename(a.join("realm.sqlite"), a.join("kept.sqlite"))
        .unwrap();
    assert!(Home::open(&a).is_err());
    assert!(!a.join("realm.sqlite").exists());
    std::fs::copy(b.join("realm.sqlite"), a.join("realm.sqlite"))
        .unwrap();
    assert!(Home::open(&a).is_err());
    std::fs::rename(a.join("kept.sqlite"), a.join("realm.sqlite"))
        .unwrap();
    open(&a).close().unwrap();
    std::fs::remove_file(a.join("realm.id")).unwrap();
    assert!(Home::open(&a).is_err());
}

#[test]
fn an_existing_empty_or_foreign_database_is_not_initialized() {
    for version in [0, 1, 2] {
        let dir = Scratch::new("server-foreign-store");
        let db = Connection::open(dir.join("realm.sqlite")).unwrap();
        db.execute_batch(
            "CREATE TABLE foreign_data (value TEXT); INSERT INTO \
             foreign_data VALUES ('kept');",
        )
        .unwrap();
        db.pragma_update(None, "user_version", version).unwrap();
        drop(db);
        let before = std::fs::read(dir.join("realm.sqlite")).unwrap();
        assert!(
            Home::open(&dir).is_err(),
            "accepted version {version}"
        );
        assert_eq!(
            std::fs::read(dir.join("realm.sqlite")).unwrap(),
            before
        );
    }
    let dir = Scratch::new("server-empty-store");
    std::fs::write(dir.join("realm.sqlite"), []).unwrap();
    assert!(Home::open(&dir).is_err());
}

#[test]
fn a_store_of_another_version_is_refused() {
    for version in [0, 2, 99] {
        let dir = Scratch::new("server-version");
        open(&dir).close().unwrap();
        let db = Connection::open(dir.join("realm.sqlite")).unwrap();
        db.pragma_update(None, "user_version", version).unwrap();
        drop(db);
        assert!(
            Home::open(&dir).is_err(),
            "accepted version {version}"
        );
        let db = Connection::open(dir.join("realm.sqlite")).unwrap();
        let kept = db
            .query_row("PRAGMA user_version", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        assert_eq!(kept, version);
    }
}

#[test]
fn a_crash_between_the_change_and_its_marker_keeps_neither() {
    let (_dir, mut home) = crashed("server-crash-marker", "marker");
    assert_eq!(state(&mut home), (10, 0, 0));
    let reply = home.store().run(&id("pi-1", "7"), &spend("ada", 3));
    assert_eq!(reply.unwrap(), done(7));
    assert_eq!(state(&mut home), (7, 1, 1));
}

#[test]
fn a_crash_after_the_commit_replays_the_marker() {
    let (_dir, mut home) = crashed("server-crash-reply", "reply");
    assert_eq!(state(&mut home), (7, 1, 1));
    let reply = home.store().run(&id("pi-1", "7"), &spend("ada", 3));
    assert_eq!(reply.unwrap(), done(7));
    assert_eq!(state(&mut home), (7, 1, 1));
}

/// A seeded home after a child process holding it ran `spend("ada",
/// 3)` and was killed at `pause`, reopened.
fn crashed(tag: &str, pause: &str) -> (Scratch, Home) {
    let (dir, home) = seeded(tag);
    home.close().unwrap();
    let paused = paused_file(&dir);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "operation::tests::child",
            "--ignored",
            "--nocapture",
        ])
        .env(CHILD_HOME, &*dir)
        .env(CHILD_PAUSE, pause)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !paused.exists() {
        assert!(
            child.try_wait().unwrap().is_none(),
            "the child ended"
        );
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "no pause"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(is_held(&dir), "the child does not hold the home");
    child.kill().unwrap();
    child.wait().unwrap();
    let home = open(&dir);
    (dir, home)
}

#[test]
#[ignore = "the process the crash tests kill"]
#[expect(
    clippy::print_stdout,
    reason = "it shows the reply when the child is run by hand"
)]
fn child() {
    let Some(dir) = std::env::var_os(CHILD_HOME) else {
        return;
    };
    let mut home = open(Path::new(&dir));
    let reply = home.store().run(&id("pi-1", "7"), &spend("ada", 3));
    pause_at("reply");
    println!("{:?}", reply.unwrap());
}

fn paused_file(dir: &Path) -> PathBuf {
    dir.join("paused")
}

/// Stops this process for good at `point` when it is the child told
/// to.
fn pause_at(point: &str) {
    let (Some(dir), Ok(pause)) =
        (std::env::var_os(CHILD_HOME), std::env::var(CHILD_PAUSE))
    else {
        return;
    };
    if pause == point {
        std::fs::write(paused_file(Path::new(&dir)), point).unwrap();
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

#[test]
fn a_changed_request_under_a_used_key_runs_nothing() {
    let scratch = Scratch::new("operation-changed");
    let mut store =
        Store::open(&scratch.join("realm.sqlite")).unwrap();
    let id = OperationId {
        runtime: RuntimeInstance::new("pi-1").unwrap(),
        key: "7".into(),
    };
    let asked = |name: &str| {
        Request::tool("set_model", &json!({"name": name})).unwrap()
    };
    let first = store
        .once(&id, &asked("ana"), |_| Ok("ana's model".to_owned()))
        .unwrap();
    assert_eq!(first.as_deref(), Some("ana's model"));
    let again = store
        .once(&id, &asked("ana"), |_| -> Result<String> {
            panic!("a marked call ran again")
        })
        .unwrap();
    assert_eq!(again.as_deref(), Some("ana's model"));
    let changed = store
        .once(&id, &asked("bo"), |_| -> Result<String> {
            panic!("a changed request ran under a used key")
        })
        .unwrap();
    assert_eq!(changed, None);
    assert_eq!(
        marked::<String>(store.connection(), &id, &asked("bo"))
            .unwrap(),
        Some(Marked::Conflict)
    );
}
