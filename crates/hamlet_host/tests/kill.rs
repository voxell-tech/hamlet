//! A real kill of the host. The test runs its own binary again as a
//! child host, stops the child at a fault point, kills it with
//! SIGKILL, and opens the folder in a new host.

#![cfg(unix)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use hamlet_actor_core::ActorFactory;
use hamlet_host::Host;
use hamlet_testing::Scratch;

#[path = "../examples/demo/counter.rs"]
mod counter;

fn factories() -> HashMap<String, ActorFactory> {
    counter::factories(counter::Sleep::WhenIdle)
}

/// The realm folder the child host opens. Unset, the child test does
/// nothing.
const CHILD_HOME: &str = "HAMLET_HOST_TEST_HOME";

/// How far ahead, in milliseconds, the child schedules its one-shot.
/// Unset, the child boots the folder instead.
const CHILD_SCHEDULE_MS: &str = "HAMLET_HOST_TEST_SCHEDULE_MS";

/// How far ahead the child schedules a one-shot that must not come
/// due before a kill, a restart and a clean stop.
const LATE_MS: u64 = 4000;

/// How far ahead the child schedules a one-shot it runs before the
/// kill.
const SOON_MS: u64 = 800;

/// Runs as the child. With a schedule, schedules a one-shot
/// `increment` on `counter/a`, prints `acked` once the action
/// replied, and passes the fault point `schedule-acked`; without one,
/// boots the folder. Then waits to be killed.
#[test]
fn child_host() {
    let Some(home) = std::env::var_os(CHILD_HOME) else {
        return;
    };
    let schedule_ms = std::env::var(CHILD_SCHEDULE_MS)
        .ok()
        .map(|ms| ms.parse::<u64>().unwrap());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let host = Host::open(Path::new(&home), factories()).unwrap();
        if let Some(ms) = schedule_ms {
            let id = host.get_or_start("counter", "a").await.unwrap();
            host.action(&id, "schedule", ms.to_le_bytes().to_vec())
                .await
                .unwrap();
            writeln!(std::io::stdout(), "acked").unwrap();
            hamlet_host::fault::point("schedule-acked");
        } else {
            host.boot().await.unwrap();
        }
        std::future::pending::<()>().await;
    });
}

/// Runs a child host on `home` until it stops at the fault point
/// `fault`, and kills it there. With `schedule_ms`, the child
/// schedules a one-shot that far ahead, and the result is when its
/// acknowledgement was read. Without, the child boots the folder,
/// and prints nothing: its boot may be what stops at the point.
fn kill_child_at(
    home: &Path,
    schedule_ms: Option<u64>,
    fault: &str,
) -> Option<Instant> {
    let faults = home.join("faults");
    std::fs::create_dir_all(&faults).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "child_host", "--nocapture"])
        .env(CHILD_HOME, home)
        .env("HAMLET_FAULT", fault)
        .env("HAMLET_FAULT_DIR", &faults)
        .stdout(Stdio::null());
    if let Some(ms) = schedule_ms {
        command
            .env(CHILD_SCHEDULE_MS, ms.to_string())
            .stdout(Stdio::piped());
    }
    let mut child = command.spawn().unwrap();
    let acked = child.stdout.take().and_then(|stdout| {
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .any(|line| line == "acked")
            .then(Instant::now)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !faults.join("host-paused").exists() {
        assert!(Instant::now() < deadline, "the child never paused");
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(
        acked.is_some(),
        schedule_ms.is_some(),
        "the child never acknowledged the schedule"
    );
    acked
}

/// When the child's one-shot is due: no earlier than `earliest`, no
/// later than `latest`.
struct Due {
    earliest: Instant,
    latest: Instant,
}

/// Runs a child host that schedules a one-shot and is killed once
/// the schedule is acknowledged, while the actor runs: the host
/// stores no running actor's alarm.
fn kill_after_the_ack(home: &Path) -> Due {
    let schedule = Duration::from_millis(LATE_MS);
    let earliest = Instant::now() + schedule;
    let acked = kill_child_at(home, Some(LATE_MS), "schedule-acked");
    Due {
        earliest,
        latest: acked.unwrap() + schedule,
    }
}

fn sleep_until(instant: Instant) {
    std::thread::sleep(
        instant.saturating_duration_since(Instant::now()),
    );
}

async fn count(host: &Host, actor_id: &str) -> i64 {
    let reply =
        host.action(actor_id, "get", Vec::new()).await.unwrap();
    counter::count(reply).unwrap()
}

/// Waits for the actor's count to reach `want`: a start runs an
/// overdue one-shot on a task of its own, after it returns.
async fn wait_for_count(host: &Host, actor_id: &str, want: i64) {
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(10);
    while count(host, actor_id).await != want {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the count never reached {want}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// What `counter/a`'s file holds, read with no host open.
#[derive(Debug, PartialEq)]
struct Stored {
    count: i64,
    schedules: i64,
}

fn stored(home: &Path) -> Stored {
    let conn = rusqlite::Connection::open_with_flags(
        home.join("actors/counter/a.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let read = |sql: &str| -> i64 {
        conn.query_row(sql, [], |row| row.get(0)).unwrap()
    };
    Stored {
        count: read(
            "SELECT COALESCE((SELECT value FROM counter WHERE id = \
             0), 0)",
        ),
        schedules: read(
            "SELECT COUNT(*) FROM _actor_schedule_events",
        ),
    }
}

/// The host's marks in `counter/a`'s file, read with no host open:
/// whether a run is under way, the deadline the last clean stop left,
/// and the schedule rows a generation claimed.
#[derive(Debug, PartialEq)]
struct Marks {
    running: i64,
    wake_at: Option<i64>,
    claimed: i64,
}

fn marks(home: &Path) -> Marks {
    let conn = rusqlite::Connection::open_with_flags(
        home.join("actors/counter/a.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let (running, wake_at) = conn
        .query_row(
            "SELECT running, wake_at FROM _hamlet_actor",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let claimed = conn
        .query_row(
            "SELECT COUNT(*) FROM _actor_schedule_events WHERE \
             claimed_generation IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    Marks {
        running,
        wake_at,
        claimed,
    }
}

/// Opens `home` in a new host and boots it, then stops every actor,
/// which waits for the scheduled work the boot started. Returns what
/// the boot started and what the file holds after the stop.
async fn restart(home: &Path) -> (Vec<String>, Stored) {
    let host = Host::open(home, factories()).unwrap();
    let started = host.boot().await.unwrap();
    host.stop_all().await.unwrap();
    drop(host);
    (started, stored(home))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_schedule_acknowledged_before_a_kill_fires_once() {
    let home = Scratch::new("kill-due");
    let due = kill_after_the_ack(&home);
    sleep_until(due.latest + Duration::from_millis(200));

    let host = Host::open(&home, factories()).unwrap();
    assert_eq!(host.boot().await.unwrap(), vec!["counter/a"]);
    wait_for_count(&host, "counter/a", 1).await;
    host.stop_all().await.unwrap();
    drop(host);

    let host = Host::open(&home, factories()).unwrap();
    assert!(host.boot().await.unwrap().is_empty());
    let id = host.get_or_start("counter", "a").await.unwrap();
    assert_eq!(count(&host, &id).await, 1);
    host.stop_all().await.unwrap();
}

/// The first start after the kill comes before the one-shot is due
/// and stops cleanly, so only the deadline that stop stored can wake
/// the actor at the second.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_before_the_schedule_is_due_stores_its_deadline() {
    let home = Scratch::new("kill-early");
    let due = kill_after_the_ack(&home);

    let host = Host::open(&home, factories()).unwrap();
    host.boot().await.unwrap();
    host.stop_all().await.unwrap();
    drop(host);
    assert!(
        Instant::now() < due.earliest,
        "the first start came after due"
    );
    sleep_until(due.latest + Duration::from_millis(200));

    let host = Host::open(&home, factories()).unwrap();
    assert_eq!(host.boot().await.unwrap(), vec!["counter/a"]);
    wait_for_count(&host, "counter/a", 1).await;
    host.stop_all().await.unwrap();
}

/// The scheduled increment is killed inside its transaction, so the
/// kill leaves neither its change nor its end. The next start runs
/// it, once.
#[tokio::test(flavor = "multi_thread")]
async fn a_kill_while_a_one_shot_runs_runs_it_once() {
    let home = Scratch::new("kill-one-shot");
    kill_child_at(&home, Some(SOON_MS), "tx-open");
    assert_eq!(
        stored(&home).count,
        0,
        "the kill came after the commit"
    );

    let (started, after) = restart(&home).await;
    assert_eq!(started, vec!["counter/a"]);
    assert_eq!(
        after,
        Stored {
            count: 1,
            schedules: 0
        }
    );

    let (started, after) = restart(&home).await;
    assert!(started.is_empty());
    assert_eq!(
        after,
        Stored {
            count: 1,
            schedules: 0
        }
    );
}

/// The one-shot comes due while the actor is stopped, and the boot
/// whose timer starts the actor for it is killed before the start.
/// The next boot runs the one-shot, once.
#[tokio::test(flavor = "multi_thread")]
async fn a_kill_on_the_boot_that_fires_a_due_alarm_runs_it_once() {
    let home = Scratch::new("kill-timer-fired");
    {
        let host = Host::open(&home, factories()).unwrap();
        let id = host.get_or_start("counter", "a").await.unwrap();
        host.action(&id, "increment", Vec::new()).await.unwrap();
        host.action(&id, "schedule", 500u64.to_le_bytes().to_vec())
            .await
            .unwrap();
        host.stop_all().await.unwrap();
    }
    assert_eq!(
        stored(&home),
        Stored {
            count: 1,
            schedules: 1
        }
    );
    let stopped = marks(&home);
    assert!(stopped.wake_at.is_some(), "the stop left no deadline");
    tokio::time::sleep(Duration::from_millis(900)).await;
    kill_child_at(&home, None, "timer-fired");
    assert_eq!(
        (stored(&home), marks(&home)),
        (
            Stored {
                count: 1,
                schedules: 1
            },
            Marks {
                running: 0,
                claimed: 0,
                ..stopped
            }
        ),
        "the kill came after the start"
    );

    let (started, after) = restart(&home).await;
    assert_eq!(started, vec!["counter/a"]);
    assert_eq!(
        after,
        Stored {
            count: 2,
            schedules: 0
        }
    );

    let (started, after) = restart(&home).await;
    assert!(started.is_empty());
    assert_eq!(
        after,
        Stored {
            count: 2,
            schedules: 0
        }
    );
}

/// The scheduled increment is killed after its transaction commits
/// and before the core hears it replied. Its transaction ended the
/// one-shot with the change, so no start runs it again.
#[tokio::test(flavor = "multi_thread")]
async fn a_kill_after_the_timer_handler_commits_runs_it_once() {
    let home = Scratch::new("kill-committed");
    kill_child_at(&home, Some(SOON_MS), "tx-committed");
    assert_eq!(
        stored(&home),
        Stored {
            count: 1,
            schedules: 0
        }
    );

    let (started, after) = restart(&home).await;
    assert_eq!(started, vec!["counter/a"]);
    assert_eq!(
        after,
        Stored {
            count: 1,
            schedules: 0
        }
    );

    let (started, after) = restart(&home).await;
    assert!(started.is_empty());
    assert_eq!(
        after,
        Stored {
            count: 1,
            schedules: 0
        }
    );
}
