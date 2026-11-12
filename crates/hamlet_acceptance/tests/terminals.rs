//! Terminals: the chief's own, typed into and unsandboxed, the
//! read-only commands of an agent's shell tool, and what a restart
//! keeps of them.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use serde_json::json;

/// What terminal `id` printed since its last redraw, as text.
fn printed(chief: &Client, id: &str) -> String {
    let mut bytes = Vec::new();
    for message in chief.messages() {
        if let ServerMessage::Terminal(output) = message
            && output.terminal == id
        {
            if output.reset {
                bytes.clear();
            }
            bytes.extend(STANDARD.decode(&output.data).unwrap());
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

async fn shows(chief: &mut Client, id: &str, text: &str) {
    chief
        .until(&format!("terminal {id} to print {text}"), |c| {
            printed(c, id).contains(text)
        })
        .await;
}

async fn open(chief: &mut Client) -> String {
    let answer = chief
        .ask(|id| {
            ClientMessage::Terminal(Terminal::Open {
                cols: 80,
                id,
                rows: 24,
            })
        })
        .await;
    match answer {
        ServerMessage::Terminal(output) => output.terminal,
        other => panic!("an open got {other:?}"),
    }
}

async fn type_in(chief: &mut Client, id: &str, text: &str) {
    chief
        .send(ClientMessage::Terminal(Terminal::Input {
            data: text.into(),
            terminal: id.into(),
        }))
        .await;
}

async fn watch(chief: &mut Client, id: &str) {
    let answer = chief
        .ask(|request| {
            ClientMessage::Terminal(Terminal::Watch {
                id: request,
                terminal: id.into(),
            })
        })
        .await;
    assert!(
        matches!(answer, ServerMessage::Terminal(ref o) if o.reset),
        "{answer:?}"
    );
}

async fn view(chief: &mut Client, id: &str) -> TerminalView {
    chief
        .until(&format!("terminal {id} in the state"), |c| {
            c.realm().terminals.iter().any(|t| t.id == id)
        })
        .await;
    chief
        .realm()
        .terminals
        .into_iter()
        .find(|t| t.id == id)
        .unwrap_or_else(|| panic!("no terminal {id}"))
}

#[tokio::test]
async fn the_chief_types_into_an_unbounded_terminal_resizes_and_closes_it()
 {
    let mut s = Session::start(Faux::new()).await;
    let id = open(&mut s.chief).await;
    let shown = view(&mut s.chief, &id).await;
    assert_eq!(shown.hamlet, None);
    assert_eq!(shown.agent, None);
    assert_eq!(shown.kind, TerminalKind::Shell);
    assert!(!shown.running);
    type_in(&mut s.chief, &id, "echo hi-$((40 + 2))\r").await;
    shows(&mut s.chief, &id, "hi-42").await;
    s.chief
        .send(ClientMessage::Terminal(Terminal::Resize {
            cols: 100,
            rows: 20,
            terminal: id.clone(),
        }))
        .await;
    type_in(&mut s.chief, &id, "stty size\r").await;
    shows(&mut s.chief, &id, "20 100").await;
    let closed = s
        .chief
        .request(|request| {
            ClientMessage::Terminal(Terminal::Close {
                id: request,
                terminal: id.clone(),
            })
        })
        .await;
    assert_eq!(closed.kind, ReplyKind::Notice, "{closed:?}");
    s.chief
        .until("the terminal gone", |c| {
            c.realm().terminals.is_empty()
        })
        .await;
}

#[tokio::test]
async fn a_terminal_keeps_its_last_command_and_the_name_the_chief_gives_it()
 {
    let mut s = Session::start(Faux::new()).await;
    let id = open(&mut s.chief).await;
    type_in(&mut s.chief, &id, "echo last-$((1 + 1))\r").await;
    shows(&mut s.chief, &id, "last-2").await;
    s.chief
        .until("the last command in the state", |c| {
            c.realm().terminals.iter().any(|t| {
                t.id == id
                    && t.last.as_deref()
                        == Some("echo last-$((1 + 1))")
            })
        })
        .await;
    let named = s
        .chief
        .request(|request| {
            ClientMessage::Terminal(Terminal::Rename {
                id: request,
                name: " builds ".into(),
                terminal: id.clone(),
            })
        })
        .await;
    assert_eq!(named.kind, ReplyKind::Notice, "{named:?}");
    s.chief
        .until("the name in the state", |c| {
            c.realm()
                .terminals
                .iter()
                .any(|t| t.id == id && t.named && t.name == "builds")
        })
        .await;
}

#[tokio::test]
async fn an_agent_has_no_terminal_tools_and_its_bash_commands_show_read_only()
 {
    let faux = Faux::new().agent(
        2,
        [
            call("bash", json!({"command": "echo ran-$((2 + 3))"})),
            words("Done."),
        ],
    );
    let mut s = Session::start(faux).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.tell(ada, "Run something.").await;
    s.chief.shows(&now, "Done.").await;
    let terminals = s.chief.realm().terminals;
    assert!(
        terminals.iter().all(|t| t.kind == TerminalKind::Commands),
        "an agent opened a shell: {terminals:?}"
    );
    let commands = terminals
        .iter()
        .find(|t| t.agent == Some(ada))
        .expect("the agent's commands");
    let docs = s.chief.hamlet_id("docs");
    assert_eq!(commands.hamlet.as_deref(), Some(docs.as_str()));
    assert_eq!(commands.name, format!("hire#{}'s commands", ada.0));
    assert_eq!(commands.last.as_deref(), Some("echo ran-$((2 + 3))"));
    assert!(!commands.running);
    let id = commands.id.clone();
    watch(&mut s.chief, &id).await;
    shows(&mut s.chief, &id, "echo ran-$((2 + 3))").await;
    shows(&mut s.chief, &id, "ran-5").await;
    // The commands are read-only: typing into them reaches no shell.
    type_in(&mut s.chief, &id, "echo typed-$((1 + 1))\r").await;
    s.chief.settle(Duration::from_millis(300)).await;
    assert!(!printed(&s.chief, &id).contains("typed-2"));
}

#[tokio::test]
async fn a_running_command_shows_as_running_until_it_ends() {
    let faux = Faux::new().agent(
        2,
        [
            call("bash", json!({"command": "sleep 2; echo slept"})),
            words("Done."),
        ],
    );
    let mut s = Session::start(faux).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.tell(ada, "Wait a bit.").await;
    s.chief
        .until("the command running", |c| {
            c.realm()
                .terminals
                .iter()
                .any(|t| t.agent == Some(ada) && t.running)
        })
        .await;
    s.chief.shows(&now, "Done.").await;
    s.chief
        .until("the command ended", |c| {
            c.realm()
                .terminals
                .iter()
                .any(|t| t.agent == Some(ada) && !t.running)
        })
        .await;
}

#[tokio::test]
async fn a_worker_s_commands_close_when_it_answers() {
    let faux = Faux::new()
        .agent(
            2,
            [
                call(
                    "start_worker",
                    json!({"task": "run a command", "wait": true}),
                ),
                words("The worker said 42."),
            ],
        )
        .kind(
            "worker",
            [call("bash", json!({"command": "echo w"})), words("42")],
        );
    let mut s = Session::start(faux).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.tell(ada, "Count.").await;
    s.chief.shows(&now, "The worker said 42.").await;
    let worker = s.chief.id("worker#3");
    let opened = s
        .chief
        .changes()
        .into_iter()
        .filter_map(|update| match update {
            Update::Terminal { terminal } => Some(terminal),
            _ => None,
        })
        .filter(|terminal| terminal.agent == Some(worker))
        .map(|terminal| terminal.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        opened.first(),
        Some(&TerminalKind::Commands),
        "{opened:?}"
    );
    assert!(!opened.contains(&TerminalKind::Shell), "{opened:?}");
    s.chief
        .until("the worker's commands gone", |c| {
            !c.realm()
                .terminals
                .iter()
                .any(|t| t.agent == Some(worker))
        })
        .await;
}

#[tokio::test]
async fn a_restart_keeps_a_terminal_s_output_below_a_new_shell() {
    let mut home = Home::new();
    let mut server = home.serve(&Faux::new(), QUIET).await;
    let mut chief = Client::join(&server).await;
    let id = open(&mut chief).await;
    type_in(&mut chief, &id, "echo before-$((3 * 3))\r").await;
    shows(&mut chief, &id, "before-9").await;
    chief.close().await;
    server.stop().await;
    let server = home.serve(&Faux::new(), QUIET).await;
    let mut chief = Client::join(&server).await;
    assert_eq!(view(&mut chief, &id).await.exit, None);
    watch(&mut chief, &id).await;
    shows(&mut chief, &id, "before-9").await;
    shows(&mut chief, &id, "restarted").await;
    type_in(&mut chief, &id, "echo after-$((4 * 4))\r").await;
    shows(&mut chief, &id, "after-16").await;
    chief.settle(Duration::from_millis(100)).await;
}
