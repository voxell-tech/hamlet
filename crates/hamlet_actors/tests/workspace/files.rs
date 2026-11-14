//! An agent's file tools on its project files.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use hamlet_actor::Action;
use hamlet_host::{Address, Host};
use hamlet_messages::workspace::files::{
    FileAnswer, Find, ReadFile, Search, Text,
};
use hamlet_messages::workspace::reach::{Access, Refusal};
use hamlet_messages::workspace::{
    AgentNumber, CallRefusal, CallerRefusal,
};
use hamlet_testing::Scratch;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{
    AGENT, Peers, SECRET, agent, call_as, decoded, host_with, reach,
    realm, realm_sets, rows_in_file,
};

/// Calls the workspace with `asked`, as the actor at `from`.
async fn ask<A>(host: &Host, from: &Address, asked: &A) -> A::Output
where
    A: Action + Serialize,
    A::Output: DeserializeOwned,
{
    let reply = call_as(
        host,
        from,
        A::NAME,
        hamlet_actor::action::encode_positional(asked).unwrap(),
    )
    .await
    .unwrap();
    decoded(&reply)
}

fn read_file(path: &Path) -> ReadFile {
    ReadFile {
        agent: AGENT,
        path: path.to_owned(),
        offset: None,
        limit: None,
    }
}

fn search(under: &Path, pattern: &str) -> Search {
    Search {
        agent: AGENT,
        under: under.to_owned(),
        pattern: pattern.to_owned(),
        name: None,
        ignore_case: false,
    }
}

fn find(under: &Path) -> Find {
    Find {
        agent: AGENT,
        under: under.to_owned(),
        name: None,
    }
}

fn text(answer: FileAnswer) -> Text {
    match answer {
        FileAnswer::Text(text) => text,
        other => panic!("the call answered {other:?}"),
    }
}

/// A project folder outside the realm folder, holding `files`.
fn project(name: &str, files: &[(&str, &str)]) -> (Scratch, PathBuf) {
    let scratch = Scratch::new(name);
    let folder = scratch.join("project");
    for (file, content) in files {
        let path = folder.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    std::fs::create_dir_all(&folder).unwrap();
    (scratch, folder)
}

/// A host on `dir` where agent 7 works in `folder`.
async fn working_in(
    dir: &Path,
    folder: &Path,
    peers: Arc<Peers>,
) -> Host {
    let host = host_with(dir, peers).await;
    realm_sets(&host, 1, reach(Some(folder.to_str().unwrap()))).await;
    host
}

/// A read, a search and a find of the project answer what is there,
/// cut as today's tools cut it.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn the_file_tools_read_the_project() {
    let dir = Scratch::new("workspace-files-read");
    let lines =
        (1..=5).map(|n| format!("line {n}\n")).collect::<String>();
    let (_project, folder) = project(
        "workspace-files-read-project",
        &[("notes.md", &lines), ("src/main.rs", "fn main() {}\n")],
    );
    let host = working_in(&dir, &folder, Arc::default()).await;
    let me = agent(AGENT);

    let whole = text(
        ask(&host, &me, &read_file(&folder.join("notes.md"))).await,
    );
    let part = text(
        ask(
            &host,
            &me,
            &ReadFile {
                offset: Some(2),
                limit: Some(2),
                ..read_file(&folder.join("notes.md"))
            },
        )
        .await,
    );
    let found =
        text(ask(&host, &me, &search(&folder, "fn main")).await);
    let listed = text(ask(&host, &me, &find(&folder)).await);
    let missing =
        ask(&host, &me, &read_file(&folder.join("gone.md"))).await;
    host.shutdown().await.unwrap();

    assert_eq!(whole.text, lines);
    assert_eq!(part.text, "line 2\nline 3");
    assert_eq!(
        part.notes[0].message,
        "3 more lines in file. Use offset=4 to continue."
    );
    assert_eq!(found.text, "src/main.rs:1: fn main() {}");
    assert_eq!(listed.text, "notes.md\nsrc/");
    assert!(
        matches!(&missing, FileAnswer::Failed(words) if words.contains("ENOENT")),
        "{missing:?}"
    );
}

/// A path out of the agent's reach is refused with its path, whether
/// a file is there or not, so the refusal tells nothing of it.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_read_outside_the_reach_names_the_path() {
    let dir = Scratch::new("workspace-files-outside");
    let (_project, folder) =
        project("workspace-files-outside-project", &[("a.md", "a")]);
    std::fs::write(dir.join("credentials.json"), "{}").unwrap();
    let host = working_in(&dir, &folder, Arc::default()).await;
    let me = agent(AGENT);
    let mut answers = Vec::new();
    for path in [dir.join("credentials.json"), dir.join("gone.json")]
    {
        answers.push((
            path.clone(),
            ask(&host, &me, &read_file(&path)).await,
            ask(&host, &me, &search(&path, "x")).await,
            ask(&host, &me, &find(&path)).await,
        ));
    }
    host.shutdown().await.unwrap();

    for (path, read, searched, found) in answers {
        let refused = FileAnswer::Refused(CallRefusal::Reach(
            Refusal::OutsideReach {
                path,
                access: Access::Read,
            },
        ));
        assert_eq!(read, refused);
        assert_eq!(searched, refused);
        assert_eq!(found, refused);
    }
}

/// An agent's own filesystem is its own to read: the workspace
/// refuses it, and the agent's adapter reads it.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_agents_filesystem_is_not_the_workspaces_to_read() {
    let dir = Scratch::new("workspace-files-own");
    let (_project, folder) =
        project("workspace-files-own-project", &[("a.md", "a")]);
    let notes = super::filesystem(&dir).join("notes.md");
    std::fs::write(&notes, "mine").unwrap();
    let host = working_in(&dir, &folder, Arc::default()).await;
    let read = ask(&host, &agent(AGENT), &read_file(&notes)).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        read,
        FileAnswer::Refused(CallRefusal::Reach(
            Refusal::AgentFilesystem { path: notes }
        ))
    );
}

/// A file tool is for the agent the host stamped as the caller: an
/// agent naming another and an actor that is not an agent are
/// refused.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_read_is_for_the_agent_that_called() {
    let dir = Scratch::new("workspace-files-caller");
    let (_project, folder) =
        project("workspace-files-caller-project", &[("a.md", "a")]);
    let host = working_in(&dir, &folder, Arc::default()).await;
    let asked = read_file(&folder.join("a.md"));
    let other = ask(&host, &agent(AgentNumber(8)), &asked).await;
    let from_realm = ask(&host, &realm(), &asked).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        other,
        FileAnswer::Refused(CallRefusal::Caller(
            CallerRefusal::NotTheCaller {
                named: AGENT,
                caller: AgentNumber(8),
            }
        ))
    );
    assert_eq!(
        from_realm,
        FileAnswer::Refused(CallRefusal::Caller(
            CallerRefusal::NotAnAgent
        ))
    );
}

/// A read, a search and a find change nothing, so they leave no row.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_read_leaves_no_row() {
    let dir = Scratch::new("workspace-files-rows");
    let (_project, folder) =
        project("workspace-files-rows-project", &[("a.md", "a")]);
    let host = working_in(&dir, &folder, Arc::default()).await;
    let me = agent(AGENT);
    text(ask(&host, &me, &read_file(&folder.join("a.md"))).await);
    host.shutdown().await.unwrap();
    let before = rows_in_file(&dir);
    let host = working_in(&dir, &folder, Arc::default()).await;
    text(ask(&host, &me, &read_file(&folder.join("a.md"))).await);
    text(ask(&host, &me, &search(&folder, "a")).await);
    text(ask(&host, &me, &find(&folder)).await);
    host.shutdown().await.unwrap();

    assert_eq!(rows_in_file(&dir), before);
}

/// W3: what a file tool hands an agent has the realm's secrets taken
/// out, also on the first call after a start, which fetches them.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn what_a_read_hands_back_is_scrubbed() {
    let dir = Scratch::new("workspace-files-scrub");
    let named = format!("{SECRET}.md");
    let (_project, folder) = project(
        "workspace-files-scrub-project",
        &[(&named, &format!("key = {SECRET}\n"))],
    );
    let peers = Arc::new(Peers::default());
    let host = working_in(&dir, &folder, peers.clone()).await;
    let me = agent(AGENT);
    let read =
        text(ask(&host, &me, &read_file(&folder.join(&named))).await);
    let found = text(ask(&host, &me, &search(&folder, "key")).await);
    let listed = text(ask(&host, &me, &find(&folder)).await);
    let missing = ask(
        &host,
        &me,
        &read_file(&folder.join(format!("{SECRET}/x"))),
    )
    .await;
    host.shutdown().await.unwrap();

    assert_eq!(peers.fetched.load(Ordering::SeqCst), 1);
    assert_eq!(read.text, "key = [REDACTED]\n");
    assert_eq!(found.text, "[REDACTED].md:1: key = [REDACTED]");
    assert_eq!(listed.text, "[REDACTED].md");
    assert!(
        matches!(&missing, FileAnswer::Failed(words) if words.contains("[REDACTED]/x")),
        "{missing:?}"
    );
}
