//! An agent's place pushed while its Node host cannot take it: the
//! agent holds the realm's place by its next request all the same.

use std::path::{Path, PathBuf};

use hamlet_acceptance::{Client, chat};
use hamlet_protocol::client::{AgentId, ChatRef, ItemKind};
use hamlet_testing::{Scratch, Server};
use serde_json::json;

use super::commands::found;
use super::lines::Frozen;
use super::{HeldNode, kill_node, serve, until};

/// What `notes.txt` in the project holds.
const NOTES: &str = "The project's own notes.";

#[tokio::test(flavor = "multi_thread")]
async fn a_place_pushed_while_the_node_host_is_down_holds_once_it_is_back()
 {
    let held = HeldNode::new();
    let (mut server, home, _side) = held.serve(json!([])).await;
    let project = project();
    let mut chief = Client::join(&server).await;
    let (mayor, watched) = up(&server, &mut chief).await;
    kill_node(&home).await;

    moved(&mut chief, mayor, &project).await;
    held.release();

    holds(&server, &mut chief, mayor, &watched, &project).await;
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_place_pushed_to_a_node_host_killed_with_it_in_flight_holds_after_the_restart()
 {
    let (mut server, home, _side) = serve(json!([])).await;
    let project = project();
    let mut chief = Client::join(&server).await;
    let (mayor, watched) = up(&server, &mut chief).await;
    let frozen = Frozen::node(&home);

    moved(&mut chief, mayor, &project).await;
    frozen.killed(&home).await;

    holds(&server, &mut chief, mayor, &watched, &project).await;
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// The mayor of a new hamlet and its watched chat, once it has
/// answered a line under the place it was made with.
async fn up(
    server: &Server,
    chief: &mut Client,
) -> (AgentId, ChatRef) {
    let mayor = found(server, chief, "Docs").await;
    let watched = chat(mayor);
    chief.open(&watched).await;
    asked(chief, mayor, &watched, "Are you up?").await;
    (mayor, watched)
}

/// A project folder with `notes.txt`, as its real path.
fn project() -> (Scratch, PathBuf) {
    let project = Scratch::new("front-project");
    std::fs::write(project.join("notes.txt"), NOTES).unwrap();
    let real = std::fs::canonicalize(&*project).unwrap();
    (project, real)
}

/// Moves `mayor`'s hamlet to `folder` in manual mode, and turns the
/// mayor's `write` off.
async fn moved(
    chief: &mut Client,
    mayor: AgentId,
    (_, folder): &(Scratch, PathBuf),
) {
    chief
        .command(mayor, &format!("/folder {}", folder.display()))
        .await;
    chief.command(mayor, "/mode manual").await;
    chief.command(mayor, "/tools mayor -write").await;
}

/// Asserts that `mayor`'s next request offers the tools [`moved`]
/// left on, that its prompt names the folder and the manual mode,
/// and that its next tool and command work in the folder.
async fn holds(
    server: &Server,
    chief: &mut Client,
    mayor: AgentId,
    watched: &ChatRef,
    (_, folder): &(Scratch, PathBuf),
) {
    let tools = asked(chief, mayor, watched, "tools").await;
    let offered = |name: &str| {
        tools
            .lines()
            .any(|line| line.starts_with(&format!("{name}:")))
    };
    assert!(offered("read") && !offered("write"), "{tools}");

    let prompt = chief.prompt(mayor).await;
    assert!(
        prompt.contains(&format!(
            "Your hamlet works on the folder {}.",
            folder.display()
        )),
        "{prompt}"
    );
    assert!(
        prompt.contains("A chief approves each edit"),
        "{prompt}"
    );

    let read = asked(chief, mayor, watched, "read notes.txt").await;
    assert!(read.contains(NOTES), "{read}");

    ran_in(server, chief, mayor, watched, folder).await;
}

/// The model's last answer to `text` said to `mayor`, once its turn
/// has ended.
async fn asked(
    chief: &mut Client,
    mayor: AgentId,
    watched: &ChatRef,
    text: &str,
) -> String {
    let seen = answers(chief, watched).len();
    chief.say(mayor, text).await;
    next_answer(chief, watched, seen).await
}

/// Waits until a command of `mayor`'s waits for a chief, and once
/// allowed shows its output: the folder it ran in.
async fn ran_in(
    server: &Server,
    chief: &mut Client,
    mayor: AgentId,
    watched: &ChatRef,
    folder: &Path,
) {
    let seen = answers(chief, watched).len();
    chief.say(mayor, "run: pwd").await;
    until(server, "the command's wait", |realm| {
        !realm.waits.is_empty()
    })
    .await;
    chief.command(mayor, "/allow").await;
    let folder = folder.display().to_string();
    chief
        .until("the command's output", |c| {
            answers(c, watched)
                .iter()
                .skip(seen)
                .any(|answer| answer.contains(&folder))
        })
        .await;
}

/// The model's answers that `watched` shows.
fn answers(chief: &Client, watched: &ChatRef) -> Vec<String> {
    chief
        .items(watched)
        .into_iter()
        .filter(|item| item.kind == ItemKind::Assistant)
        .map(|item| item.text)
        .collect()
}

/// The model's last answer in `watched` once there are more than
/// `seen` and its turn has ended.
async fn next_answer(
    chief: &mut Client,
    watched: &ChatRef,
    seen: usize,
) -> String {
    chief
        .until("the model's answer", |c| {
            answers(c, watched).len() > seen && !c.is_working(watched)
        })
        .await;
    answers(chief, watched).pop().unwrap_or_default()
}
