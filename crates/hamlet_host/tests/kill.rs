//! A real kill of the host. The test runs its own binary again as a
//! child host, stops the child at a fault point, kills it with
//! SIGKILL, and opens the folder in a new host; or the child's own
//! timer aborts it.

#![cfg(unix)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use act::act;
use at::at;
use clock::now_ms;
use hamlet_actor_core::ActorFactory;
use hamlet_host::{Host, HostEvent};
use hamlet_testing::Scratch;

#[path = "../examples/demo/act.rs"]
mod act;
#[path = "support/at.rs"]
mod at;
#[path = "support/clock.rs"]
mod clock;
#[path = "../examples/demo/counter.rs"]
mod counter;

fn factories() -> HashMap<String, ActorFactory> {
    counter::factories(&["counter"], counter::Sleep::WhenIdle)
}

/// The realm folder the child host opens. Unset, the child test does
/// nothing.
const CHILD_HOME: &str = "HAMLET_HOST_TEST_HOME";

/// How far ahead, in milliseconds, the child schedules its one-shot.
const CHILD_SCHEDULE_MS: &str = "HAMLET_HOST_TEST_SCHEDULE_MS";

/// Set, the child destroys `counter/a`.
const CHILD_DESTROY: &str = "HAMLET_HOST_TEST_DESTROY";

/// How far ahead the child schedules a one-shot that must not come
/// due before a kill, a restart and a clean stop.
const LATE_MS: u64 = 4000;

/// How far ahead the child schedules a one-shot it runs before the
/// kill.
const SOON_MS: u64 = 800;

/// What a child host does before it waits to be killed.
#[derive(Clone, Copy)]
enum Child {
    /// Boots the folder.
    Boot,
    /// Schedules a one-shot `increment` on `counter/a` that many ms
    /// ahead, prints `acked` once the action replied, and passes the
    /// fault point `schedule-acked`.
    Schedule(u64),
    /// Increments `counter/a` twice, then destroys it.
    Destroy,
}

/// Runs as the child, as [`Child`] says, then waits to be killed.
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
        if std::env::var_os(CHILD_DESTROY).is_some() {
            let id = host.get_or_start("counter", "a").await.unwrap();
            for action in ["increment", "increment", "destroy"] {
                act(&host, &id, action, Vec::new()).await.unwrap();
            }
        } else if let Some(ms) = schedule_ms {
            let id = host.get_or_start("counter", "a").await.unwrap();
            act(&host, &id, "schedule", ms.to_le_bytes().to_vec())
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

/// Runs a child host on `home` that does `what` until it stops at
/// the fault point `fault`, and kills it there. For a schedule, the
/// result is when its acknowledgement was read.
fn kill_child_at(
    home: &Path,
    what: Child,
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
        .stdout(Stdio::piped());
    match what {
        Child::Boot => {}
        Child::Schedule(ms) => {
            command.env(CHILD_SCHEDULE_MS, ms.to_string());
        }
        Child::Destroy => {
            command.env(CHILD_DESTROY, "1");
        }
    }
    let mut child = command.spawn().unwrap();
    // Read on a thread of its own, so a child that stops before it
    // prints keeps no test waiting.
    let stdout = child.stdout.take().unwrap();
    let acked = std::thread::spawn(move || {
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
    let acked = acked.join().unwrap();
    if let Child::Schedule(_) = what {
        assert!(
            acked.is_some(),
            "the child never acknowledged the schedule"
        );
    }
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
    let acked = kill_child_at(
        home,
        Child::Schedule(LATE_MS),
        "schedule-acked",
    );
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
    let reply = act(host, actor_id, "get", Vec::new()).await.unwrap();
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

/// What `counter/<key>`'s file holds, read with no host open.
#[derive(Debug, PartialEq)]
struct Stored {
    count: i64,
    schedules: i64,
}

fn stored(home: &Path, key: &str) -> Stored {
    let conn = rusqlite::Connection::open_with_flags(
        home.join(format!("actors/counter/{key}.sqlite")),
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

/// Opens `home` in a new host and boots it, waits for the starts the
/// boot began, then stops every actor, which waits for the scheduled
/// work the starts began. Returns what the boot began and what the
/// file holds after the stop.
async fn restart(home: &Path) -> (Vec<String>, Stored) {
    let host = Host::open(home, factories()).unwrap();
    let started = host.boot().await.unwrap();
    for id in &started {
        act(&host, id, "get", Vec::new()).await.unwrap();
    }
    host.shutdown().await.unwrap();
    drop(host);
    (started, stored(home, "a"))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_schedule_acknowledged_before_a_kill_fires_once() {
    let home = Scratch::new("kill-due");
    let due = kill_after_the_ack(&home);
    sleep_until(due.latest + Duration::from_millis(200));

    let host = Host::open(&home, factories()).unwrap();
    assert_eq!(host.boot().await.unwrap(), vec!["counter/a"]);
    wait_for_count(&host, "counter/a", 1).await;
    host.shutdown().await.unwrap();
    drop(host);

    let host = Host::open(&home, factories()).unwrap();
    assert!(host.boot().await.unwrap().is_empty());
    let id = host.get_or_start("counter", "a").await.unwrap();
    assert_eq!(count(&host, &id).await, 1);
    host.shutdown().await.unwrap();
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
    host.shutdown().await.unwrap();
    drop(host);
    assert!(
        Instant::now() < due.earliest,
        "the first start came after due"
    );
    sleep_until(due.latest + Duration::from_millis(200));

    let host = Host::open(&home, factories()).unwrap();
    assert_eq!(host.boot().await.unwrap(), vec!["counter/a"]);
    wait_for_count(&host, "counter/a", 1).await;
    host.shutdown().await.unwrap();
}

/// The scheduled increment is killed inside its transaction, so the
/// kill leaves neither its change nor its end. The next start runs
/// it, once.
#[tokio::test(flavor = "multi_thread")]
async fn a_kill_while_a_one_shot_runs_runs_it_once() {
    let home = Scratch::new("kill-one-shot");
    kill_child_at(&home, Child::Schedule(SOON_MS), "tx-open");
    assert_eq!(
        stored(&home, "a").count,
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
/// that starts the actor for it is killed before the start.
/// The next boot runs the one-shot, once.
#[tokio::test(flavor = "multi_thread")]
async fn a_kill_on_the_boot_that_fires_a_due_alarm_runs_it_once() {
    let home = Scratch::new("kill-timer-fired");
    {
        let host = Host::open(&home, factories()).unwrap();
        let id = host.get_or_start("counter", "a").await.unwrap();
        act(&host, &id, "increment", Vec::new()).await.unwrap();
        act(&host, &id, "schedule", 500u64.to_le_bytes().to_vec())
            .await
            .unwrap();
        host.shutdown().await.unwrap();
    }
    assert_eq!(
        stored(&home, "a"),
        Stored {
            count: 1,
            schedules: 1
        }
    );
    let stopped = marks(&home);
    assert!(stopped.wake_at.is_some(), "the stop left no deadline");
    tokio::time::sleep(Duration::from_millis(900)).await;
    kill_child_at(&home, Child::Boot, "timer-fired");
    assert_eq!(
        (stored(&home, "a"), marks(&home)),
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
    kill_child_at(&home, Child::Schedule(SOON_MS), "tx-committed");
    assert_eq!(
        stored(&home, "a"),
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

/// Where a kill cuts a destroy off once its actor asked for it: as
/// its stop marks the run ended, once the destroy is stored in the
/// actor's file, and before each of its three files is removed.
const DESTROY_CUTS: [&str; 5] = [
    "run-ended",
    "destroy-marked",
    "destroy-remove:1",
    "destroy-remove:2",
    "destroy-remove:3",
];

/// A destroy that a kill cut off is finished at the next boot: the
/// actor does not come back, its three files are gone, and a new
/// start of its key reads 0.
#[tokio::test(flavor = "multi_thread")]
async fn a_destroy_a_kill_cut_off_is_finished_at_the_next_boot() {
    for cut in DESTROY_CUTS {
        let home = Scratch::new("kill-destroy");
        kill_child_at(&home, Child::Destroy, cut);
        let file = home.join("actors/counter/a.sqlite");
        assert!(
            file.exists(),
            "{cut}: the kill came after the removal"
        );

        let host = Host::open(&home, factories()).unwrap();
        assert!(host.boot().await.unwrap().is_empty(), "{cut}");
        for suffix in ["", "-wal", "-shm"] {
            let mut path = file.clone().into_os_string();
            path.push(suffix);
            assert!(!Path::new(&path).exists(), "{cut}: {path:?}");
        }
        let id = host.get_or_start("counter", "a").await.unwrap();
        assert_eq!(count(&host, &id).await, 0, "{cut}");
        host.shutdown().await.unwrap();
    }
}

/// What the aborting child does: `arm`, `boot` or `rearm`. Unset, the
/// aborting child does nothing.
const ABORT_STEP: &str = "HAMLET_HOST_TEST_ABORT_STEP";

/// How long the aborting child stays up after its boot.
const STAY: Duration = Duration::from_secs(2);

/// How far ahead the bystander's one-shot is due, so that it comes
/// due while a child that survives its boot stays up, or before.
const BYSTANDER_DUE_MS: u64 = 1500;

/// How far ahead a re-armed timer is due, so that the child prints
/// before the timer aborts it.
const REARM_DUE_MS: i64 = 500;

/// Runs as the child of the test of a timer that aborts its host.
/// `counter/a`'s one-shot `abort` aborts the process; `counter/k`
/// stands by.
/// - `arm` counts `counter/k` to 7 and schedules its `increment`
///   `BYSTANDER_DUE_MS` ahead, prints `armed`, then schedules
///   `counter/a`'s `abort` 100 ms ahead and waits.
/// - `boot` boots and stays up `STAY`. Then it prints each timer
///   `counter/a` set aside, as `timer <action> <tries> <error|->`,
///   whether the host's events are one set-aside event for each, and
///   `survived` once it has shut the host down.
/// - `rearm` puts each timer `counter/a` set aside back,
///   `REARM_DUE_MS` ahead, prints `rearmed <true|false>`, and waits.
#[test]
fn aborting_child() {
    let (Some(home), Ok(step)) =
        (std::env::var_os(CHILD_HOME), std::env::var(ABORT_STEP))
    else {
        return;
    };
    let mut out = std::io::stdout();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let host = Host::open(Path::new(&home), factories()).unwrap();
        let at = at("counter", "a");
        match step.as_str() {
            "arm" => {
                let k =
                    host.get_or_start("counter", "k").await.unwrap();
                for _ in 0..7 {
                    act(&host, &k, "increment", Vec::new())
                        .await
                        .unwrap();
                }
                let due = BYSTANDER_DUE_MS.to_le_bytes().to_vec();
                act(&host, &k, "schedule", due).await.unwrap();
                writeln!(out, "armed").unwrap();
                let a =
                    host.get_or_start("counter", "a").await.unwrap();
                let mut abort = 100u64.to_le_bytes().to_vec();
                abort.extend_from_slice(b"abort");
                act(&host, &a, "schedule", abort).await.unwrap();
            }
            "boot" => {
                let mut events = host.events();
                host.boot().await.unwrap();
                tokio::time::sleep(STAY).await;
                let timers =
                    host.set_aside_timers(&at).await.unwrap();
                let set_aside: Vec<HostEvent> = timers
                    .iter()
                    .map(|timer| HostEvent::TimerSetAside {
                        actor: at.clone(),
                        event_id: timer.event_id.clone(),
                        action: timer.action.clone(),
                        due_at: timer.due_at,
                        tries: timer.tries,
                        error: timer.last_error.clone(),
                    })
                    .collect();
                let raised: Vec<HostEvent> =
                    std::iter::from_fn(|| events.try_recv().ok())
                        .collect();
                for timer in &timers {
                    writeln!(
                        out,
                        "timer {} {} {}",
                        timer.action,
                        timer.tries,
                        timer.last_error.as_deref().unwrap_or("-")
                    )
                    .unwrap();
                }
                writeln!(
                    out,
                    "set-aside events match: {}",
                    raised == set_aside
                )
                .unwrap();
                host.shutdown().await.unwrap();
                writeln!(out, "survived").unwrap();
                return;
            }
            "rearm" => {
                let due = now_ms() + REARM_DUE_MS;
                for timer in host.set_aside_timers(&at).await.unwrap()
                {
                    let rearmed = host
                        .rearm_timer(&at, &timer.event_id, due)
                        .await
                        .unwrap();
                    writeln!(out, "rearmed {rearmed}").unwrap();
                }
            }
            other => panic!("unknown step {other}"),
        }
        std::future::pending::<()>().await;
    });
}

/// How one aborting child ended, and what it printed.
#[derive(Debug)]
struct Ended {
    status: ExitStatus,
    lines: Vec<String>,
}

impl Ended {
    fn aborted(&self) -> bool {
        self.status.signal() == Some(6)
    }
}

/// Runs the aborting child's `step` on `home` until it exits, with
/// core dumps off. Fails if it runs for more than 30 seconds.
fn run_aborting_child(home: &Path, step: &str) -> Ended {
    let mut child = Command::new("sh")
        .args(["-c", "ulimit -c 0 && exec \"$0\" \"$@\""])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", "aborting_child", "--nocapture"])
        .env(CHILD_HOME, home)
        .env(ABORT_STEP, step)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut text = String::new();
        stdout.read_to_string(&mut text).unwrap();
        text
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("the {step} child ran for more than 30 s");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let known = [
        "armed",
        "timer ",
        "set-aside events ",
        "survived",
        "rearmed ",
    ];
    let lines = out
        .join()
        .unwrap()
        .lines()
        .filter(|line| {
            known.iter().any(|word| line.starts_with(word))
        })
        .map(str::to_owned)
        .collect();
    Ended { status, lines }
}

/// A one-shot whose handler aborts the process. The process that
/// armed it and the first four boots after it each try it and die.
/// The fifth boot finds it tried five times and sets it aside, with
/// no outcome recorded, and its process stays up. The bystander
/// `counter/k` in the same folder keeps its count, and its own
/// one-shot runs once. A re-arm tries the timer again.
#[test]
fn a_one_shot_that_kills_its_host_is_set_aside_at_the_fifth_boot() {
    let home = Scratch::new("kill-abort-loop");
    let armed = run_aborting_child(&home, "arm");
    assert!(armed.aborted(), "{armed:?}");
    assert_eq!(armed.lines, ["armed"]);
    for boot in 1..=4 {
        let booted = run_aborting_child(&home, "boot");
        assert!(booted.aborted(), "boot {boot}: {booted:?}");
    }
    let booted = run_aborting_child(&home, "boot");
    assert!(booted.status.success(), "boot 5: {booted:?}");
    assert_eq!(
        booted.lines,
        [
            "timer abort 5 -",
            "set-aside events match: true",
            "survived"
        ]
    );
    assert_eq!(
        stored(&home, "a"),
        Stored {
            count: 5,
            schedules: 0
        }
    );
    assert_eq!(
        stored(&home, "k"),
        Stored {
            count: 8,
            schedules: 0
        }
    );

    let rearmed = run_aborting_child(&home, "rearm");
    assert!(rearmed.aborted(), "{rearmed:?}");
    assert_eq!(rearmed.lines, ["rearmed true"]);
    assert_eq!(stored(&home, "a").count, 6);
}
