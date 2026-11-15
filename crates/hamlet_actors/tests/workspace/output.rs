//! An agent's saved command output: gone when its reach ends, and
//! kept under a limit while the agent lives.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use hamlet_host::Host;
use hamlet_messages::workspace::reach::Reach;
use hamlet_messages::workspace::{
    AgentNumber, Answer, Exit, Run, RunAnswer,
};
use hamlet_testing::Scratch;
use tokio::task::JoinHandle;

use super::changes::manual;
use super::files::project;
use super::waits::chief;
use super::{
    AGENT, Peers, agent_runs, done, ended, filesystem, host,
    host_with, ran, reach, realm_sets, run, saved_output, told,
    told_within,
};

/// The files in `folder`, once it holds `n` of them, waiting up to
/// ten seconds.
async fn files_in(folder: &Path, n: usize) -> Vec<PathBuf> {
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let files = std::fs::read_dir(folder)
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().path())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if files.len() >= n || tokio::time::Instant::now() > until {
            return files;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Calls the workspace with `run`, as agent 7, without waiting.
fn spawn_run(host: &Host, run: Run) -> JoinHandle<RunAnswer> {
    let host = host.clone();
    tokio::spawn(async move { agent_runs(&host, &run).await })
}

/// When agent 7's reach ends, its saved output goes, also the file
/// of a command still writing to it then. Agent 8's stays.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_ended_reach_removes_its_saved_output() {
    let dir = Scratch::new("workspace-output-ended");
    filesystem(&dir);
    let others = saved_output(&dir, AgentNumber(8)).join("1.log");
    std::fs::create_dir_all(others.parent().unwrap()).unwrap();
    std::fs::write(&others, "theirs\n").unwrap();
    let host = host(&dir).await;
    realm_sets(&host, 1, reach(None)).await;
    done(agent_runs(&host, &run("c1", "seq 1 3000")).await);
    let writing = spawn_run(
        &host,
        run("c2", "seq 1 3000; while :; do echo x; sleep 0.05; done"),
    );
    let before = files_in(&saved_output(&dir, AGENT), 2).await.len();
    realm_sets(&host, 2, Reach::Ended).await;
    let after = saved_output(&dir, AGENT).exists();
    let stopped = done(writing.await.unwrap());
    host.shutdown().await.unwrap();

    assert_eq!(before, 2);
    assert!(!after, "agent 7's saved output is still there");
    assert_eq!(stopped.exit, Exit::Cancelled);
    assert_eq!(std::fs::read_to_string(&others).unwrap(), "theirs\n");
}

/// Past the limit, a command that saves its output first removes the
/// file of the oldest command that finished, and never the file of
/// one still running, though it is older.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn the_oldest_finished_output_goes_first() {
    let dir = Scratch::new("workspace-output-limit");
    let fs = filesystem(&dir);
    let peers = Arc::new(Peers {
        saved_output: Some(100_000),
        ..Peers::default()
    });
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let waiting = spawn_run(
        &host,
        run(
            "w",
            "seq 1 2001; while [ ! -e go ]; do sleep 0.05; done",
        ),
    );
    let waiting_file = files_in(&saved_output(&dir, AGENT), 1)
        .await
        .pop()
        .expect("the waiting command saved its output");
    let mut saved_files = Vec::new();
    for call in ["a", "b", "c"] {
        // 60,000 bytes each: two fit under the limit, three do not.
        let end = done(
            agent_runs(&host, &run(call, "yes | head -n 30000"))
                .await,
        );
        saved_files
            .push(end.cut.expect("the output was saved").full_output);
    }
    let kept = saved_files
        .iter()
        .map(|file| file.exists())
        .collect::<Vec<_>>();
    std::fs::write(fs.join("go"), "").unwrap();
    let waited = ended(&peers, waiting.await.unwrap()).await;
    host.shutdown().await.unwrap();

    assert_eq!(kept, [false, true, true]);
    assert_eq!(
        waited.cut.expect("the output was saved").full_output,
        waiting_file
    );
    assert_eq!(
        std::fs::read_to_string(&waiting_file).unwrap(),
        (1..=2001).map(|n| format!("{n}\n")).collect::<String>()
    );
}

/// A command a chief allows late keeps the id its wait was given, yet
/// its output is as old as its run: past the limit, the output of the
/// command that ran first goes.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn output_is_as_old_as_its_run() {
    let dir = Scratch::new("workspace-output-run-order");
    filesystem(&dir);
    let (_project, folder) =
        project("workspace-output-run-order-project", &[]);
    let peers = Arc::new(Peers {
        saved_output: Some(150_000),
        ..Peers::default()
    });
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    // 60,000 bytes each: three are past the limit, two are under it.
    let script = "yes | head -n 30000";
    agent_runs(&host, &run("late", script)).await;
    for (ran_before, call) in
        ["a", "b", "late", "c"].into_iter().enumerate()
    {
        if call != "late" {
            agent_runs(&host, &run(call, script)).await;
        }
        chief(&host, call, Answer::Allow).await;
        told_within(&peers, ran_before + 1, Duration::from_secs(20))
            .await;
    }
    let kept = told(&peers)
        .iter()
        .map(|(_, finished)| {
            let cut = ran(finished)
                .cut
                .as_ref()
                .expect("the output was saved");
            (
                finished.call.0.as_str().to_owned(),
                cut.full_output.exists(),
            )
        })
        .collect::<Vec<_>>();
    host.shutdown().await.unwrap();

    assert_eq!(
        kept,
        [("a", false), ("b", true), ("late", true), ("c", true)]
            .map(|(call, kept)| (call.to_owned(), kept))
    );
}
