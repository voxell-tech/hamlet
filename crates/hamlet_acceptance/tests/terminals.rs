//! Terminals: the chief's own, a hamlet's under the sandbox, those an
//! agent opens and types into, the runs of its shell tool, and what a
//! restart keeps of them.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::{Home, Script, call, words};
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

async fn open(chief: &mut Client, hamlet: Option<&str>) -> String {
    let answer = chief
        .ask(|id| {
            ClientMessage::Terminal(Terminal::Open {
                cols: 80,
                hamlet: hamlet.map(String::from),
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
    let mut s = Session::start(Script::new()).await;
    let id = open(&mut s.chief, None).await;
    let shown = view(&mut s.chief, &id).await;
    assert_eq!(shown.hamlet, None);
    assert_eq!(shown.kind, TerminalKind::Shell);
    assert!(!shown.sandboxed);
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
async fn a_hamlet_terminal_runs_in_the_house_under_the_sandbox() {
    let mut s = Session::start(Script::new()).await;
    let outside = s.home.side().join("outside");
    let id = open(&mut s.chief, Some("home")).await;
    let shown = view(&mut s.chief, &id).await;
    assert_eq!(shown.hamlet.as_deref(), Some("home"));
    assert!(shown.sandboxed);
    type_in(
        &mut s.chief,
        &id,
        &format!(
            "pwd -P; touch '{}' 2>/dev/null; echo rc=$((0 + $?))\r",
            outside.display()
        ),
    )
    .await;
    shows(&mut s.chief, &id, "rc=1").await;
    let house = s.home.file("houses/home/0").canonicalize().unwrap();
    assert!(
        printed(&s.chief, &id).contains(house.to_str().unwrap()),
        "{}",
        printed(&s.chief, &id)
    );
    assert!(!outside.exists());
}

#[tokio::test]
async fn a_terminal_keeps_its_last_command_and_the_name_the_chief_gives_it()
 {
    let mut s = Session::start(Script::new()).await;
    let id = open(&mut s.chief, Some("home")).await;
    // The prompt, its word dimmed.
    shows(&mut s.chief, &id, "sandboxed\x1b[0m $ ").await;
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
async fn an_agent_opens_and_types_into_a_terminal_and_its_bash_runs_show()
 {
    let script = Script::new().responses([
        call("terminal_open", json!({"name": "dev"})),
        call(
            "terminal_write",
            json!({"terminal": "dev", "text": "echo agent-$((6 * 7))"}),
        ),
        call("bash", json!({"command": "echo ran-$((2 + 3))"})),
        words("Done."),
    ]);
    let mut s = Session::start(script).await;
    s.chief.open(&chat(mayor())).await;
    s.chief.say(mayor(), "Work in a terminal.").await;
    s.chief.shows(&chat(mayor()), "Done.").await;
    let terminals = s.chief.realm().terminals;
    let me = AgentRef {
        hamlet: "home".into(),
        number: 0,
    };
    let dev = terminals
        .iter()
        .find(|t| t.kind == TerminalKind::Shell)
        .expect("the agent's terminal");
    assert_eq!(dev.name, "dev");
    assert_eq!(dev.agent.as_ref(), Some(&me));
    assert!(dev.sandboxed);
    let runs = terminals
        .iter()
        .find(|t| t.kind == TerminalKind::Runs)
        .expect("the agent's runs");
    assert_eq!(runs.agent.as_ref(), Some(&me));
    let tools = s
        .chief
        .items(&chat(mayor()))
        .into_iter()
        .filter_map(|item| item.tool)
        .map(|tool| (tool.name, tool.exit, tool.failure))
        .collect::<Vec<_>>();
    assert_eq!(
        tools,
        ["terminal_open", "terminal_write", "bash"].map(|name| (
            name.to_owned(),
            Some(0),
            None
        )),
    );
    let (dev, runs) = (dev.id.clone(), runs.id.clone());
    watch(&mut s.chief, &dev).await;
    shows(&mut s.chief, &dev, "agent-42").await;
    watch(&mut s.chief, &runs).await;
    shows(&mut s.chief, &runs, "echo ran-$((2 + 3))").await;
    shows(&mut s.chief, &runs, "ran-5").await;
    // The chief types into the agent's terminal too.
    type_in(&mut s.chief, &dev, "echo chief-$((1 + 1))\r").await;
    shows(&mut s.chief, &dev, "chief-2").await;
}

#[tokio::test]
async fn a_restart_keeps_a_terminal_s_output_below_a_new_shell() {
    let mut home = Home::new();
    let mut server = home.serve(&Script::new(), QUIET).await;
    let mut chief = Client::join(&server).await;
    let id = open(&mut chief, None).await;
    type_in(&mut chief, &id, "echo before-$((3 * 3))\r").await;
    shows(&mut chief, &id, "before-9").await;
    chief.close().await;
    server.stop().await;
    let server = home.serve(&Script::new(), QUIET).await;
    let mut chief = Client::join(&server).await;
    assert_eq!(view(&mut chief, &id).await.exit, None);
    watch(&mut chief, &id).await;
    shows(&mut chief, &id, "before-9").await;
    shows(&mut chief, &id, "restarted").await;
    type_in(&mut chief, &id, "echo after-$((4 * 4))\r").await;
    shows(&mut chief, &id, "after-16").await;
    chief.settle(Duration::from_millis(100)).await;
}
