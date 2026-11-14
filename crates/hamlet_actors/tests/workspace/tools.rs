//! Tools a chief adds by hand (C17): listed for an agent to offer its
//! model, run as a command is, with the reach their manifest grants,
//! and in a folder no agent writes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use hamlet_messages::agent::Outcome;
use hamlet_messages::workspace::files::ChangeAnswer;
use hamlet_messages::workspace::reach::{Need, Refusal};
use hamlet_messages::workspace::{
    AddedTool, Answer, AnswerTaken, CallId, CallRefusal,
    ConversationId, Exit, ListTools, Program, Run, RunAnswer, Scope,
    ToolList,
};
use hamlet_testing::Scratch;

use super::changes::{read, write};
use super::files::ask;
use super::waits::chief;
use super::{
    AGENT, Peers, SECRET, agent, agent_runs, answering_within, done,
    filesystem, host, host_with, reach, realm_sets, run, running,
    told, told_within,
};

const MANIFEST: &str = r#"{
    "name": "echo_word",
    "description": "Prints its arguments",
    "exec": ["cat"],
    "parameters": {
        "type": "object",
        "properties": {"word": {"type": "string"}},
        "required": ["word"]
    }
}"#;

/// Adds the tool `folder` with `manifest` to the realm at `dir`.
fn add_tool(dir: &Path, folder: &str, manifest: &str) -> PathBuf {
    let path = dir.join("tools").join(folder).join("manifest.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, manifest).unwrap();
    path
}

fn tool(call: &str, name: &str, arguments: &str) -> Run {
    Run {
        agent: AGENT,
        call: CallId(call.to_owned()),
        conversation: ConversationId("c1".to_owned()),
        realm_task: None,
        program: Program::Tool {
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        },
    }
}

/// The tools that load are listed with what a model is offered; a
/// folder whose manifest does not load, or takes a built-in tool's
/// name, is left out with its reason.
#[tokio::test(flavor = "multi_thread")]
async fn the_tools_added_by_hand_are_listed() {
    let dir = Scratch::new("workspace-tools-listed");
    add_tool(&dir, "a-echo", MANIFEST);
    add_tool(&dir, "broken", "not json");
    add_tool(&dir, "shell", r#"{"name": "bash", "exec": ["cat"]}"#);
    std::fs::write(dir.join("tools/notes.txt"), "not a tool")
        .unwrap();
    let host = host(&dir).await;
    let list: ToolList = ask(&host, &agent(AGENT), &ListTools).await;
    host.shutdown().await.unwrap();
    assert_eq!(
        list.tools,
        vec![AddedTool {
            name: "echo_word".into(),
            description: "Prints its arguments".into(),
            parameters: r#"{"properties":{"word":{"type":"string"}},"required":["word"],"type":"object"}"#.into(),
        }]
    );
    assert_eq!(list.faults.len(), 2, "{:?}", list.faults);
    assert!(
        list.faults[0]
            .ends_with("broken: manifest.json is not valid JSON"),
        "{:?}",
        list.faults
    );
    assert!(
        list.faults[1].ends_with("shell: name 'bash' is reserved"),
        "{:?}",
        list.faults
    );
}

/// A tool runs as a command does: its arguments on its standard
/// input, its output with the realm's secrets taken out, and a repeat
/// of its call answered from its row.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_tool_added_by_hand_runs_as_a_command() {
    let dir = Scratch::new("workspace-tools-run");
    filesystem(&dir);
    add_tool(&dir, "echo", MANIFEST);
    let host = host(&dir).await;
    realm_sets(&host, 1, reach(None)).await;
    let arguments = format!(r#"{{"word":"hi {SECRET}"}}"#);
    let call = tool("t1", "echo_word", &arguments);
    let first = agent_runs(&host, &call).await;
    let again = agent_runs(&host, &call).await;
    host.shutdown().await.unwrap();
    let end = done(first.clone());
    assert_eq!(end.exit, Exit::Code(0), "{end:?}");
    assert!(end.output.starts_with(r#"{"word":"hi "#), "{end:?}");
    assert!(!end.output.contains(SECRET), "{end:?}");
    assert_eq!(again, first);
}

/// A call of a tool that is not there, or with arguments its
/// manifest does not take, is refused and runs nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_tool_call_is_checked_against_its_manifest() {
    let dir = Scratch::new("workspace-tools-checked");
    add_tool(&dir, "echo", MANIFEST);
    let host = host(&dir).await;
    realm_sets(&host, 1, reach(None)).await;
    let missing = agent_runs(&host, &tool("t1", "nope", "{}")).await;
    let unnamed =
        agent_runs(&host, &tool("t2", "echo_word", "{}")).await;
    let not_json =
        agent_runs(&host, &tool("t3", "echo_word", "word")).await;
    host.shutdown().await.unwrap();
    assert_eq!(
        missing,
        RunAnswer::Refused(CallRefusal::Invalid(
            "Tool \"nope\" not found".into()
        ))
    );
    let invalid = RunAnswer::Refused(CallRefusal::Invalid(
        "Validation failed for tool echo_word".into(),
    ));
    assert_eq!(unnamed, invalid);
    assert_eq!(not_json, invalid);
}

/// A tool that runs past the answer's wait answers `Running`, and its
/// agent is called once with its end.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_long_tool_answers_running_then_finished() {
    let dir = Scratch::new("workspace-tools-running");
    filesystem(&dir);
    add_tool(
        &dir,
        "slow",
        r#"{"name": "slow", "exec": ["sh", "-c", "sleep 1; echo late"]}"#,
    );
    let peers = answering_within(200);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let answer = agent_runs(&host, &tool("t1", "slow", "{}")).await;
    told_within(&peers, 1, Duration::from_secs(5)).await;
    host.shutdown().await.unwrap();
    running(&answer);
    let told = told(&peers);
    assert_eq!(told.len(), 1, "{told:?}");
    let Outcome::Ran(end) = &told[0].1.outcome else {
        panic!("the agent was told {told:?}");
    };
    assert_eq!(
        (&end.exit, end.output.as_str()),
        (&Exit::Code(0), "late\n")
    );
}

/// A tool whose manifest reaches the network waits for a chief when
/// the agent's reach does not give it, as a command does. Once a
/// chief allows it, it runs, its manifest read again: a tool removed
/// meanwhile does not run.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_tool_that_reaches_the_network_waits_for_a_chief() {
    let dir = Scratch::new("workspace-tools-network");
    filesystem(&dir);
    let fetch = r#"{"name": "fetch", "exec": ["cat"], "sandbox": {"net": true}}"#;
    add_tool(&dir, "fetch", fetch);
    let peers = Arc::new(Peers::default());
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let first =
        agent_runs(&host, &tool("t1", "fetch", r#"{"url":"x"}"#))
            .await;
    let second =
        agent_runs(&host, &tool("t2", "fetch", r#"{"url":"y"}"#))
            .await;
    let allowed =
        chief(&host, "t1", Answer::Allow(Scope::Once)).await;
    told_within(&peers, 1, Duration::from_secs(5)).await;
    std::fs::remove_dir_all(dir.join("tools/fetch")).unwrap();
    let gone = chief(&host, "t2", Answer::Allow(Scope::Once)).await;
    host.shutdown().await.unwrap();
    let waits = RunAnswer::WaitsForChief(vec![Need::Network]);
    assert_eq!((first, second), (waits.clone(), waits));
    let asked = peers.asked.lock().unwrap().clone();
    assert_eq!(asked[0].subject, r#"fetch {"url":"x"}"#);
    assert_eq!(allowed, AnswerTaken::Runs);
    let told = told(&peers);
    let Outcome::Ran(end) = &told[0].1.outcome else {
        panic!("the agent was told {told:?}");
    };
    assert_eq!(
        (&end.exit, end.output.as_str()),
        (&Exit::Code(0), r#"{"url":"x"}"#)
    );
    assert_eq!(
        gone,
        AnswerTaken::NotRun(CallRefusal::Invalid(
            "Tool \"fetch\" not found".into()
        ))
    );
}

/// No agent writes the tools folder: not a command, not a write, and
/// not through a project folder that holds the realm folder or a link
/// in it, under the real sandbox.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_never_writes_the_tools_folder() {
    for with_project in [false, true] {
        let scratch = Scratch::new("workspace-tools-locked");
        let dir = scratch.join("realm");
        filesystem(&dir);
        let manifest = add_tool(&dir, "echo", MANIFEST);
        let link = scratch.join("tools-link");
        std::os::unix::fs::symlink(dir.join("tools"), &link).unwrap();
        let host = host(&dir).await;
        let project = with_project.then(|| scratch.to_str().unwrap());
        realm_sets(&host, 1, reach(project)).await;
        let shown = |path: &Path| path.to_str().unwrap().to_owned();
        let commands = [
            format!("printf x > '{}'", shown(&manifest)),
            format!("mkdir '{}'", shown(&dir.join("tools/new"))),
            format!(
                "printf x > '{}'",
                shown(&link.join("echo/manifest.json"))
            ),
        ];
        let mut ran = Vec::new();
        for (n, script) in commands.iter().enumerate() {
            let answer =
                agent_runs(&host, &run(&format!("c{n}"), script))
                    .await;
            ran.push((script.clone(), done(answer).exit));
        }
        let mut changes = Vec::new();
        for (n, path) in
            [manifest.clone(), link.join("echo/manifest.json")]
                .iter()
                .enumerate()
        {
            let asked = write(&format!("w{n}"), path, "x");
            let answer: ChangeAnswer =
                ask(&host, &agent(AGENT), &asked).await;
            changes.push((path.clone(), answer));
        }
        host.shutdown().await.unwrap();
        for (script, exit) in &ran {
            assert_ne!(
                exit,
                &Exit::Code(0),
                "{with_project}: {script}"
            );
        }
        for (path, answer) in &changes {
            assert!(
                matches!(
                    answer,
                    ChangeAnswer::Refused(CallRefusal::Reach(
                        Refusal::OutsideReach { .. }
                    ))
                ),
                "{with_project}: {path:?} {answer:?}"
            );
        }
        assert_eq!(read(&manifest), MANIFEST, "{with_project}");
        assert!(!dir.join("tools/new").exists(), "{with_project}");
    }
}

/// A tool's manifest cannot grant it its own folder or the tools
/// folder to write: both stay read-only.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_tool_never_writes_the_tools_folder() {
    let dir = Scratch::new("workspace-tools-own-folder");
    filesystem(&dir);
    let manifest = r#"{
        "name": "rewrite",
        "exec": ["sh", "-c", "printf x > manifest.json; printf x > ../new"],
        "sandbox": {"fs_rw": [".."]}
    }"#;
    let path = add_tool(&dir, "rewrite", manifest);
    let host = host(&dir).await;
    realm_sets(&host, 1, reach(None)).await;
    let end =
        done(agent_runs(&host, &tool("t1", "rewrite", "{}")).await);
    host.shutdown().await.unwrap();
    assert_ne!(end.exit, Exit::Code(0), "{end:?}");
    assert_eq!(read(&path), manifest);
    assert!(!dir.join("tools/new").exists());
}
