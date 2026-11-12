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

/// How far ahead the child schedules its one-shot.
const SCHEDULE_MS: u64 = 1500;

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

/// Runs a child host that schedules a one-shot and is killed once
/// the schedule is acknowledged and its alarm write is stopped at
/// the fault point `alarm-store`. Returns when the one-shot is due.
fn kill_after_the_ack_before_the_alarm_is_stored(
    home: &Path,
) -> Instant {
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
    let due = Instant::now() + Duration::from_millis(SCHEDULE_MS);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !faults.join("host-paused").exists() {
        assert!(Instant::now() < deadline, "the child never paused");
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(acked, "the child never acknowledged the schedule");
    due
}

fn sleep_until(instant: Instant) {
    std::thread::sleep(
        instant.saturating_duration_since(Instant::now()),
    );
}

async fn count(host: &Host, actor_id: &str) -> i64 {
    let reply =
        host.action(actor_id, "get", Vec::new()).await.unwrap();
    i64::from_le_bytes(reply.try_into().unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_schedule_acknowledged_before_its_alarm_was_stored_fires_once_after_a_kill()
 {
    let home = Scratch::new("kill-due");
    let due = kill_after_the_ack_before_the_alarm_is_stored(&home);
    sleep_until(due + Duration::from_millis(200));

    let host = Host::open(&home, factories()).unwrap();
    assert_eq!(host.boot().await.unwrap(), vec!["counter/a"]);
    assert_eq!(count(&host, "counter/a").await, 1);
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
    assert!(Instant::now() < due, "the first start came after due");
    sleep_until(due + Duration::from_millis(200));

    let host = Host::open(&home, factories()).unwrap();
    assert_eq!(host.boot().await.unwrap(), vec!["counter/a"]);
    assert_eq!(count(&host, "counter/a").await, 1);
    host.stop_all().await.unwrap();
}
