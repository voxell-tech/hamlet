//! An agent's writes and edits of its project files.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hamlet_messages::workspace::files::{
    Change, ChangeAnswer, EditFile, MOST_CONTENT, Replace, WriteFile,
};
use hamlet_messages::workspace::reach::{
    Access, Footing, Mode, Need, Project, Reach, Refusal,
    SecretsVersion,
};
use hamlet_messages::workspace::{
    AgentNumber, CallId, CallRefusal, CallerRefusal, ConversationId,
    RealmTask, RunAnswer,
};
use hamlet_testing::Scratch;
use hamlet_testing::links::symlink;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use super::files::{ask, project, working_in};
use super::{
    AGENT, Peers, SECRET, agent, agent_runs, filesystem, host_with,
    reach, realm, realm_sets, run,
};

pub(super) fn write(
    call: &str,
    path: &Path,
    content: &str,
) -> WriteFile {
    WriteFile {
        agent: AGENT,
        call: CallId(call.to_owned()),
        conversation: ConversationId("c1".to_owned()),
        realm_task: None,
        path: path.to_owned(),
        content: content.to_owned(),
    }
}

pub(super) fn edit(
    call: &str,
    path: &Path,
    edits: &[(&str, &str)],
) -> EditFile {
    EditFile {
        agent: AGENT,
        call: CallId(call.to_owned()),
        conversation: ConversationId("c1".to_owned()),
        realm_task: None,
        path: path.to_owned(),
        edits: edits
            .iter()
            .map(|(old, new)| Replace {
                old: (*old).to_owned(),
                new: (*new).to_owned(),
            })
            .collect(),
    }
}

pub(super) fn changed(answer: ChangeAnswer) -> Change {
    match answer {
        ChangeAnswer::Changed(change) => change,
        other => panic!("the change answered {other:?}"),
    }
}

pub(super) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub(super) fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// A write makes a file and its folders; an edit replaces its text,
/// keeping the file's mode, its byte-order mark and its line endings,
/// and says what changed.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_write_and_an_edit_change_the_project() {
    use std::os::unix::fs::PermissionsExt;

    let dir = Scratch::new("workspace-change");
    let (_project, folder) = project(
        "workspace-change-project",
        &[("notes.md", "\u{feff}one\r\ntwo\r\n")],
    );
    let notes = folder.join("notes.md");
    std::fs::set_permissions(
        &notes,
        std::fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    let made = folder.join("new/deep/made.md");
    let host = working_in(&dir, &folder, Arc::default()).await;
    let me = agent(AGENT);
    let wrote =
        changed(ask(&host, &me, &write("w1", &made, "hi\n")).await);
    let edited = changed(
        ask(&host, &me, &edit("e1", &notes, &[("two", "2")])).await,
    );
    host.shutdown().await.unwrap();

    assert_eq!(read(&made), "hi\n");
    assert_eq!(
        wrote,
        Change {
            path: made.clone(),
            created: true,
            hash: sha256(b"hi\n"),
            diff: None,
        }
    );
    assert_eq!(read(&notes), "\u{feff}one\r\n2\r\n");
    let mode =
        std::fs::metadata(&notes).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
    assert!(!edited.created);
    assert_eq!(
        edited.hash,
        sha256("\u{feff}one\r\n2\r\n".as_bytes())
    );
    let diff = edited.diff.expect("an edit's diff");
    assert!(
        diff.diff.contains(" 1 one\n-2 two\n+2 2"),
        "{}",
        diff.diff
    );
    assert_eq!(diff.first_changed_line, Some(2));
    assert!(diff.patch.contains("-two\n+2\n"), "{}", diff.patch);
    let mut names = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| {
            entry.unwrap().file_name().into_string().unwrap()
        })
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        ["new", "notes.md"],
        "nothing left beside them"
    );
}

/// A change out of the agent's reach is refused with its path, also
/// in manual mode, before anything else, and nothing is written: by
/// `..`, by a full path, by a link out of the project and by a link
/// into the agent's own filesystem. An edit of a file that is not
/// UTF-8 fails in the words of the server's tools and leaves it as it
/// was.
// Ported from the server's `writes_and_edits_nothing_out_of_reach`.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_change_out_of_reach_names_the_path_and_writes_nothing() {
    let dir = Scratch::new("workspace-change-outside");
    let (scratch, folder) =
        project("workspace-change-outside-project", &[]);
    let elsewhere = scratch.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let own = filesystem(&dir);
    symlink(&elsewhere, folder.join("out")).unwrap();
    symlink(&own, folder.join("mine")).unwrap();
    std::fs::write(folder.join("raw.txt"), [0x61, 0xff, 0x0a])
        .unwrap();
    std::fs::create_dir_all(folder.join("sub")).unwrap();
    let host = host_with(&dir, Arc::default()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    let me = agent(AGENT);
    let up = folder.join("../elsewhere/x");
    let full = elsewhere.join("x");
    let ours = own.join("x");
    let outside = |path: PathBuf| {
        ChangeAnswer::Refused(CallRefusal::Reach(
            Refusal::OutsideReach {
                path,
                access: Access::Change,
            },
        ))
    };

    let by_up = ask(&host, &me, &write("w1", &up, "x")).await;
    let by_full =
        ask(&host, &me, &edit("w2", &full, &[("a", "b")])).await;
    let by_own = ask(&host, &me, &write("w3", &ours, "x")).await;
    host.shutdown().await.unwrap();
    let host = host_with(&dir, Arc::default()).await;
    realm_sets(&host, 2, reach(Some(folder.to_str().unwrap()))).await;
    let by_link =
        ask(&host, &me, &write("w4", &folder.join("out/sub/x"), "x"))
            .await;
    let by_mine =
        ask(&host, &me, &write("w5", &folder.join("mine/x"), "x"))
            .await;
    let raw = folder.join("raw.txt");
    let by_raw =
        ask(&host, &me, &edit("w6", &raw, &[("a", "A")])).await;
    let by_folder = ask(
        &host,
        &me,
        &edit("w7", &folder.join("sub"), &[("a", "A")]),
    )
    .await;
    host.shutdown().await.unwrap();

    assert_eq!(by_up, outside(elsewhere.join("x")));
    assert_eq!(by_full, outside(full));
    assert_eq!(
        by_own,
        ChangeAnswer::Refused(CallRefusal::Reach(
            Refusal::AgentFilesystem { path: ours }
        ))
    );
    for (answer, path) in
        [(&by_link, "out/sub/x"), (&by_mine, "mine/x")]
    {
        assert_eq!(answer, &outside(folder.join(path)));
    }
    assert_eq!(
        std::fs::read_dir(&elsewhere).unwrap().count(),
        0,
        "nothing was made elsewhere"
    );
    assert_eq!(std::fs::read_dir(&own).unwrap().count(), 0);
    assert_eq!(
        by_raw,
        ChangeAnswer::Failed(format!(
            "Could not edit file: {}. Error code: invalid.",
            raw.display()
        ))
    );
    assert_eq!(std::fs::read(&raw).unwrap(), [0x61, 0xff, 0x0a]);
    assert_eq!(
        by_folder,
        ChangeAnswer::Failed(format!(
            "Could not edit file: {}. Path is not a file.",
            folder.join("sub").display()
        ))
    );
}

/// A write's content past 16 MiB is refused with its size and the
/// limit, and nothing is written; one of 16 MiB is written.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_write_past_the_limit_is_refused_and_writes_nothing() {
    let dir = Scratch::new("workspace-change-large");
    let (_project, folder) =
        project("workspace-change-large-project", &[]);
    let host = working_in(&dir, &folder, Arc::default()).await;
    let me = agent(AGENT);
    let most = usize::try_from(MOST_CONTENT).unwrap();
    let large = folder.join("large.txt");
    let largest = folder.join("largest.txt");

    let refused =
        ask(&host, &me, &write("w1", &large, &"x".repeat(most + 1)))
            .await;
    let wrote =
        ask(&host, &me, &write("w2", &largest, &"x".repeat(most)))
            .await;
    host.shutdown().await.unwrap();

    assert_eq!(
        refused,
        ChangeAnswer::Refused(CallRefusal::TooLarge {
            bytes: MOST_CONTENT + 1,
            limit: MOST_CONTENT,
        })
    );
    assert!(!large.exists());
    assert!(changed(wrote).created);
    assert_eq!(
        std::fs::metadata(&largest).unwrap().len(),
        MOST_CONTENT
    );
}

/// Agent 7's reach on `folder` in manual mode, trusting no
/// conversation.
pub(super) fn manual(folder: &Path) -> Reach {
    Reach::Agent(Footing {
        project: Some(Project {
            folder: folder.to_owned(),
            mode: Mode::Manual,
        }),
        network: false,
        trust: Vec::new(),
        secrets: SecretsVersion(1),
    })
}

/// A change is for the agent the host stamped as the caller.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_change_is_for_the_agent_that_called() {
    let dir = Scratch::new("workspace-change-caller");
    let (_project, folder) =
        project("workspace-change-caller-project", &[("a.md", "a")]);
    let host = working_in(&dir, &folder, Arc::default()).await;
    let asked = write("w1", &folder.join("a.md"), "b");
    let other = ask(&host, &agent(AgentNumber(8)), &asked).await;
    let from_realm = ask(&host, &realm(), &asked).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        other,
        ChangeAnswer::Refused(CallRefusal::Caller(
            CallerRefusal::NotTheCaller {
                named: AGENT,
                caller: AgentNumber(8),
            }
        ))
    );
    assert_eq!(
        from_realm,
        ChangeAnswer::Refused(CallRefusal::Caller(
            CallerRefusal::NotAnAgent
        ))
    );
    assert_eq!(read(&folder.join("a.md")), "a");
}

/// A repeat of a change answers from its row and writes nothing
/// again, also after a restart and after the file changed; the same
/// id with another request, as another task, is refused.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_repeat_of_a_change_answers_from_its_row() {
    let dir = Scratch::new("workspace-change-repeat");
    let (_project, folder) = project(
        "workspace-change-repeat-project",
        &[("a.md", "a\n")],
    );
    let file = folder.join("a.md");
    let host = working_in(&dir, &folder, Arc::default()).await;
    let me = agent(AGENT);
    let asked = edit("e1", &file, &[("a", "b")]);
    let first = ask(&host, &me, &asked).await;
    let repeat = ask(&host, &me, &asked).await;
    host.shutdown().await.unwrap();
    std::fs::write(&file, "theirs\n").unwrap();
    let host = working_in(&dir, &folder, Arc::default()).await;
    let after_restart = ask(&host, &me, &asked).await;
    let other_task = EditFile {
        realm_task: Some(RealmTask("task-2".into())),
        ..asked.clone()
    };
    let reused = ask(&host, &me, &other_task).await;
    host.shutdown().await.unwrap();

    assert_eq!(changed(first.clone()).hash, sha256(b"b\n"));
    assert_eq!(repeat, first);
    assert_eq!(after_restart, first);
    assert_eq!(
        reused,
        ChangeAnswer::Refused(CallRefusal::CallReused(CallId(
            "e1".into()
        )))
    );
    assert_eq!(read(&file), "theirs\n");
}

/// Copies of one change called at once write once, and each hears the
/// first one's answer.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn copies_of_a_change_called_at_once_write_once() {
    let dir = Scratch::new("workspace-change-copies");
    let (_project, folder) = project(
        "workspace-change-copies-project",
        &[("a.md", "a\n")],
    );
    let file = folder.join("a.md");
    let host = working_in(&dir, &folder, Arc::default()).await;
    let me = agent(AGENT);
    let asked = edit("e1", &file, &[("a", "ab")]);
    let (a, b, c, d) = tokio::join!(
        ask(&host, &me, &asked),
        ask(&host, &me, &asked),
        ask(&host, &me, &asked),
        ask(&host, &me, &asked),
    );
    let answers = [a, b, c, d];
    host.shutdown().await.unwrap();

    assert_eq!(read(&file), "ab\n");
    assert_eq!(changed(answers[0].clone()).hash, sha256(b"ab\n"));
    for answer in &answers {
        assert_eq!(answer, &answers[0]);
    }
}

/// A call id is one call's, whatever it does: a command's id reused
/// by a change, and a change's by a command or by another change, is
/// refused; and a start leaves a finished change's row.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_call_id_is_one_calls_whatever_it_does() {
    let dir = Scratch::new("workspace-change-ids");
    filesystem(&dir);
    let (_project, folder) =
        project("workspace-change-ids-project", &[("a.md", "a")]);
    let file = folder.join("a.md");
    let host = working_in(&dir, &folder, Arc::default()).await;
    let me = agent(AGENT);
    let ran = agent_runs(&host, &run("c1", "true")).await;
    let write_after_run =
        ask(&host, &me, &write("c1", &file, "x")).await;
    let wrote = ask(&host, &me, &write("c2", &file, "y")).await;
    let run_after_write = agent_runs(&host, &run("c2", "true")).await;
    let edit_after_write =
        ask(&host, &me, &edit("c2", &file, &[("y", "z")])).await;
    host.shutdown().await.unwrap();
    let host = working_in(&dir, &folder, Arc::default()).await;
    let after_restart =
        ask(&host, &me, &write("c2", &file, "y")).await;
    host.shutdown().await.unwrap();

    let reused =
        |id: &str| CallRefusal::CallReused(CallId(id.to_owned()));
    assert!(matches!(ran, RunAnswer::Done(_)), "{ran:?}");
    assert_eq!(write_after_run, ChangeAnswer::Refused(reused("c1")));
    assert_eq!(run_after_write, RunAnswer::Refused(reused("c2")));
    assert_eq!(edit_after_write, ChangeAnswer::Refused(reused("c2")));
    assert_eq!(after_restart, wrote);
    assert_eq!(read(&file), "y");
}

/// What a change hands back has the realm's secrets taken out, and
/// none reaches the workspace's file: an edit's diff shows the lines
/// around the change, and a failure's words name the path.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn what_a_change_hands_back_is_scrubbed() {
    let dir = Scratch::new("workspace-change-scrub");
    let named = format!("{SECRET}.md");
    let (_project, folder) = project(
        "workspace-change-scrub-project",
        &[("a.md", &format!("key = {SECRET}\nline\n"))],
    );
    let host =
        working_in(&dir, &folder, Arc::new(Peers::default())).await;
    let me = agent(AGENT);
    let edited = changed(
        ask(
            &host,
            &me,
            &edit("e1", &folder.join("a.md"), &[("line", "row")]),
        )
        .await,
    );
    let missing = ask(
        &host,
        &me,
        &edit("e2", &folder.join(&named), &[("x", "y")]),
    )
    .await;
    host.shutdown().await.unwrap();

    let diff = edited.diff.unwrap();
    assert!(
        diff.diff.contains(" 1 key = [REDACTED]\n-2 line\n+2 row"),
        "{}",
        diff.diff
    );
    assert!(!diff.diff.contains(SECRET));
    assert!(
        diff.patch.contains(" key = [REDACTED]\n"),
        "{}",
        diff.patch
    );
    assert!(!diff.patch.contains(SECRET));
    assert_eq!(
        missing,
        ChangeAnswer::Failed(format!(
            "Could not edit file: {}. Error code: not_found.",
            folder.join("[REDACTED].md").display()
        ))
    );
    assert_eq!(
        super::files_holding(&dir, SECRET),
        Vec::<PathBuf>::new()
    );
}

/// The workspace's file, opened to plant rows while the host is down.
fn writable(dir: &Path) -> Connection {
    Connection::open(dir.join("actors/workspace/local.sqlite"))
        .unwrap()
}

/// Plants an unfinished change of agent 7's call `call` whose new
/// file is `temp`, as a kill between its row and its record leaves.
fn plant_unfinished(db: &Connection, call: &str, temp: &Path) {
    db.execute(
        "INSERT INTO calls (agent, call, op, request_hash, state, \
         temp, opened_at) VALUES (7, ?1, 'write', x'00', \
         'changing', ?2, 0)",
        rusqlite::params![call, temp.to_str().unwrap()],
    )
    .unwrap();
}

/// A change a kill cut off is gone at the next start: its new file
/// is removed, also when that fails, and its row with it, so a
/// repeat is a fresh call. A file no row names is left.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_change_cut_off_by_a_kill_is_gone_at_the_next_start() {
    let dir = Scratch::new("workspace-change-killed");
    let (_project, folder) =
        project("workspace-change-killed-project", &[("a.md", "a")]);
    let host = working_in(&dir, &folder, Arc::default()).await;
    host.shutdown().await.unwrap();
    let temp = folder.join(".a.md.left");
    let stuck = folder.join(".stuck");
    let alike = folder.join(".a.md.alike");
    std::fs::write(&temp, "half").unwrap();
    std::fs::create_dir_all(stuck.join("inside")).unwrap();
    std::fs::write(&alike, "kept").unwrap();
    {
        let db = writable(&dir);
        plant_unfinished(&db, "w1", &temp);
        plant_unfinished(&db, "w2", &stuck);
    }
    let host = working_in(&dir, &folder, Arc::default()).await;
    let me = agent(AGENT);
    let first =
        ask(&host, &me, &write("w1", &folder.join("a.md"), "b"))
            .await;
    let second =
        ask(&host, &me, &write("w2", &folder.join("c.md"), "c"))
            .await;
    host.shutdown().await.unwrap();

    assert!(!temp.exists(), "the new file a kill left is removed");
    assert!(stuck.exists(), "a folder is not removed");
    assert_eq!(read(&alike), "kept");
    assert_eq!(changed(first).hash, sha256(b"b"));
    assert_eq!(changed(second).hash, sha256(b"c"));
    assert_eq!(read(&folder.join("a.md")), "b");
}

/// The size of the files the large writes replace.
#[cfg(target_os = "linux")]
const LARGE: u64 = 384 << 20;

/// The host's peak memory in KiB, as the system counts it.
#[cfg(target_os = "linux")]
fn peak_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.trim().strip_suffix("kB"))
        .and_then(|kib| kib.trim().parse().ok())
        .expect("a peak in /proc/self/status")
}

/// A write over a large file holds little of the old file in memory,
/// also in manual mode, where a chief is shown its start. Run in a
/// process of its own, so that no other test raises the peak.
#[cfg(target_os = "linux")]
#[test]
fn a_write_over_a_large_file_keeps_little_of_it_in_memory() {
    const MOST_KIB: u64 = 128 << 10;
    let ran =
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "changes::large_writes_alone",
                "--ignored",
                "--nocapture",
            ])
            .output()
            .unwrap();
    let printed = String::from_utf8_lossy(&ran.stdout);
    assert!(
        ran.status.success(),
        "{printed}{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let peak = printed
        .lines()
        .find_map(|line| line.strip_prefix("peak KiB: "))
        .and_then(|kib| kib.parse::<u64>().ok())
        .unwrap_or_else(|| panic!("no peak in {printed}"));
    assert!(peak < MOST_KIB, "the host's peak was {peak} KiB");
}

/// The writes
/// [`a_write_over_a_large_file_keeps_little_of_it_in_memory`]
/// measures.
#[cfg(target_os = "linux")]
#[ignore = "a_write_over_a_large_file_keeps_little_of_it_in_memory \
            runs it alone"]
#[tokio::test(flavor = "multi_thread")]
#[expect(
    clippy::print_stdout,
    reason = "it prints the peak it measured"
)]
async fn large_writes_alone() {
    let dir = Scratch::new("workspace-change-large");
    let (_project, folder) = project("workspace-change-large-p", &[]);
    let (auto, manual_file) =
        (folder.join("auto"), folder.join("manual"));
    for file in [&auto, &manual_file] {
        std::fs::File::create(file).unwrap().set_len(LARGE).unwrap();
    }
    let peers = Arc::new(Peers::default());
    let host = working_in(&dir, &folder, peers.clone()).await;
    let me = agent(AGENT);
    let wrote = ask(&host, &me, &write("w1", &auto, "small")).await;
    realm_sets(&host, 2, manual(&folder)).await;
    let waits =
        ask(&host, &me, &write("w2", &manual_file, "small")).await;
    host.shutdown().await.unwrap();
    let peak = peak_kib();

    assert!(!changed(wrote).created);
    assert_eq!(read(&auto), "small");
    assert_eq!(waits, ChangeAnswer::WaitsForChief(vec![Need::Edit]));
    let subject = peers.asked.lock().unwrap()[0].subject.clone();
    let shown = format!(
        "{}\nReplace:\n{}\n[{} more characters]\nWith:\nsmall",
        manual_file.display(),
        "\0".repeat(2000),
        LARGE - 2000
    );
    assert_eq!(subject, shown);
    println!("peak KiB: {peak}");
}
