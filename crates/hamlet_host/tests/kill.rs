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

/// How far ahead the child schedules its one-shot: room for a kill,
/// a restart and a clean stop before it is due.
const SCHEDULE_MS: u64 = 4000;

/// Runs as the child: schedules a one-shot on `counter/a`, prints
/// `acked` once the action replied, and waits to be killed.
#[test]
fn child_host() {
    let Some(home) = std::env::var_os(CHILD_HOME) else {
        return;
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let host = Host::open(Path::new(&home), factories()).unwrap();
        let id = host.get_or_start("counter", "a").await.unwrap();
        host.action(
            &id,
            "schedule",
            SCHEDULE_MS.to_le_bytes().to_vec(),
        )
        .await
        .unwrap();
        writeln!(std::io::stdout(), "acked").unwrap();
        std::future::pending::<()>().await;
    });
}

/// When the child's one-shot is due: no earlier than `earliest`, no
/// later than `latest`.
struct Due {
    earliest: Instant,
    latest: Instant,
}

/// Runs a child host that schedules a one-shot and is killed once
/// the schedule is acknowledged and its alarm write is stopped at
/// the fault point `alarm-store`.
fn kill_after_the_ack_before_the_alarm_is_stored(home: &Path) -> Due {
    let schedule = Duration::from_millis(SCHEDULE_MS);
    let earliest = Instant::now() + schedule;
    let faults = home.join("faults");
    std::fs::create_dir_all(&faults).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_host", "--nocapture"])
        .env(CHILD_HOME, home)
        .env("HAMLET_FAULT", "alarm-store")
        .env("HAMLET_FAULT_DIR", &faults)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let acked = stdout
        .lines()
        .map_while(Result::ok)
        .any(|line| line == "acked");
    let latest = Instant::now() + schedule;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !faults.join("host-paused").exists() {
        assert!(Instant::now() < deadline, "the child never paused");
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(acked, "the child never acknowledged the schedule");
    Due { earliest, latest }
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

#[tokio::test(flavor = "multi_thread")]
async fn a_schedule_acknowledged_before_its_alarm_was_stored_fires_once_after_a_kill()
 {
    let home = Scratch::new("kill-due");
    let due = kill_after_the_ack_before_the_alarm_is_stored(&home);
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
/// and stops cleanly, so only an alarm that start stored again can
/// wake the actor at the second.
#[tokio::test(flavor = "multi_thread")]
async fn a_start_before_the_schedule_is_due_stores_its_alarm_again() {
    let home = Scratch::new("kill-early");
    let due = kill_after_the_ack_before_the_alarm_is_stored(&home);

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
