//! Workers' scratch folders (H42): made when a worker's
//! reach arrives, removed when it ends, and swept at the workspace's
//! start when a kill left one behind.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hamlet_messages::workspace::files::{
    FileAnswer, ReadFile, WriteFile,
};
use hamlet_messages::workspace::reach::{
    Footing, Mode, Project, Reach, Refusal, SecretsVersion,
};
use hamlet_messages::workspace::{
    AgentNumber, CallId, CallRefusal, ConversationId, Exit,
    ReachAnswer, ReachVersion, RunAnswer,
};
use hamlet_testing::Scratch;

use super::changes::read;
use super::files::{ask, project};
use super::{AGENT, agent, agent_runs, done, host, realm_sets, run};

const STARTER: AgentNumber = AgentNumber(3);

/// Agent 7 as a worker of agent 3, working in `project` when given.
fn worker(project: Option<&Path>) -> Reach {
    Reach::Worker {
        starter: Some(STARTER),
        footing: Footing {
            project: project.map(|folder| Project {
                folder: folder.to_owned(),
                mode: Mode::Auto,
            }),
            network: false,
            trust: Vec::new(),
            secrets: SecretsVersion(1),
        },
    }
}

fn scratch_of(dir: &Path) -> PathBuf {
    dir.join("workers/7")
}

/// A worker's scratch folder is made, private, when its reach
/// arrives, and its commands work in it; a folder gone before a
/// command, as after a kill between the reach and the folder, is made
/// again. The folder is removed when the worker's reach ends, with
/// what its commands wrote.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_workers_scratch_folder_lives_as_long_as_the_worker() {
    use std::os::unix::fs::PermissionsExt;

    let dir = Scratch::new("workspace-worker-scratch");
    let scratch = scratch_of(&dir);
    let host = host(&dir).await;
    let set = realm_sets(&host, 1, worker(None)).await;
    let made = scratch.is_dir();
    let mode = std::fs::metadata(&scratch)
        .map(|meta| meta.permissions().mode() & 0o777)
        .ok();
    let first = done(
        agent_runs(
            &host,
            &run("c1", "echo hi > note; cat note; pwd"),
        )
        .await,
    );
    std::fs::remove_dir_all(&scratch).unwrap();
    let again = done(
        agent_runs(&host, &run("c2", "echo again > note")).await,
    );
    let ended = realm_sets(&host, 2, Reach::Ended).await;
    let removed = !scratch.exists();
    host.shutdown().await.unwrap();

    assert_eq!(
        set,
        ReachAnswer::InForce {
            version: ReachVersion(1)
        }
    );
    assert!(made, "no scratch folder at the worker's start");
    assert_eq!(mode, Some(0o700));
    assert_eq!(
        (first.exit, first.output),
        (
            Exit::Code { code: 0 },
            format!("hi\n{}\n", scratch.display())
        )
    );
    assert_eq!(again.exit, Exit::Code { code: 0 });
    assert_eq!(
        ended,
        ReachAnswer::InForce {
            version: ReachVersion(2)
        }
    );
    assert!(removed, "the scratch folder outlived the worker");
}

/// H42: a worker reads its starter's filesystem and changes nothing
/// in it, by a command or a file tool; it reads no other agent's
/// filesystem. So with a project folder and without one.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_worker_reads_its_starters_files_and_changes_none() {
    for with_project in [false, true] {
        let dir = Scratch::new("workspace-worker-starter");
        let (_project, folder) =
            project("workspace-worker-starter-p", &[]);
        let starters = dir.join("actors/agent/3.fs");
        let others = dir.join("actors/agent/9.fs");
        for (fs, words) in
            [(&starters, "remember"), (&others, "not-for-workers")]
        {
            std::fs::create_dir_all(fs).unwrap();
            std::fs::write(fs.join("notes.md"), words).unwrap();
        }
        let host = host(&dir).await;
        realm_sets(
            &host,
            1,
            worker(with_project.then_some(folder.as_path())),
        )
        .await;
        let notes = starters.join("notes.md");
        let reads = done(
            agent_runs(
                &host,
                &run("c1", &format!("cat {}", notes.display())),
            )
            .await,
        );
        let writes = done(
            agent_runs(
                &host,
                &run(
                    "c2",
                    &format!("echo changed > {}", notes.display()),
                ),
            )
            .await,
        );
        let other = done(
            agent_runs(
                &host,
                &run(
                    "c3",
                    &format!(
                        "cat {}",
                        others.join("notes.md").display()
                    ),
                ),
            )
            .await,
        );
        let read_tool = ask(
            &host,
            &agent(AGENT),
            &ReadFile {
                agent: AGENT,
                path: notes.clone(),
                offset: None,
                limit: None,
            },
        )
        .await;
        let write_tool = ask(
            &host,
            &agent(AGENT),
            &WriteFile {
                agent: AGENT,
                call: CallId("w1".into()),
                conversation: ConversationId("c1".into()),
                realm_task: None,
                path: notes.clone(),
                content: "changed".into(),
            },
        )
        .await;
        host.shutdown().await.unwrap();

        let case = format!("with a project: {with_project}");
        assert_eq!(
            (reads.exit, reads.output.as_str()),
            (Exit::Code { code: 0 }, "remember"),
            "{case}"
        );
        assert_ne!(
            writes.exit,
            Exit::Code { code: 0 },
            "{case}: {writes:?}"
        );
        assert_ne!(
            other.exit,
            Exit::Code { code: 0 },
            "{case}: {other:?}"
        );
        assert!(!other.output.contains("not-for-workers"), "{case}");
        let FileAnswer::Text(text) = read_tool else {
            panic!("{case}: the read answered {read_tool:?}");
        };
        assert_eq!(text.text, "remember", "{case}");
        assert_eq!(
            write_tool,
            hamlet_messages::workspace::files::ChangeAnswer::Refused { why: CallRefusal::Reach { why: Refusal::AgentFilesystem {
                    path: notes.clone()
                } } },
            "{case}"
        );
        assert_eq!(read(&notes), "remember", "{case}");
    }
}

/// At its start the workspace removes the scratch folder of a worker
/// whose reach ended before a kill let it go. A live worker's folder
/// stays, and so does a folder under `workers/` it knows nothing of.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn the_start_removes_the_folders_of_ended_workers() {
    let dir = Scratch::new("workspace-worker-sweep");
    let host = host(&dir).await;
    realm_sets(&host, 1, worker(None)).await;
    realm_sets(&host, 2, Reach::Ended).await;
    super::cancel::realm_sets_for(
        &host,
        AgentNumber(8),
        1,
        worker(None),
    )
    .await;
    host.shutdown().await.unwrap();
    // As a kill between the ended reach and the folder's removal
    // leaves it.
    let ended = scratch_of(&dir);
    std::fs::create_dir_all(ended.join("deep")).unwrap();
    std::fs::write(ended.join("deep/file"), "left").unwrap();
    let unknown = dir.join("workers/9");
    std::fs::create_dir_all(&unknown).unwrap();

    let host = super::host(&dir).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let swept = !ended.exists();
    let live = dir.join("workers/8").is_dir();
    let kept = unknown.is_dir();
    let after = agent_runs(&host, &run("c1", "true")).await;
    host.shutdown().await.unwrap();

    assert!(swept, "an ended worker's folder survived the start");
    assert!(live, "a live worker's folder was removed");
    assert!(kept, "a folder of no worker was removed");
    assert_eq!(
        after,
        RunAnswer::Refused {
            why: CallRefusal::Reach {
                why: Refusal::NoReach
            }
        }
    );
}
