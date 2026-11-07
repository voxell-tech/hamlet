//! Terminals: the chief's own, one in a hamlet's folder under the
//! sandbox, those an agent or a worker opens and types into, the
//! commands of its shell tool, and what a restart keeps of them.

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

async fn open(chief: &mut Client, hamlet: Option<&str>) -> String {
    match try_open(chief, hamlet).await {
        ServerMessage::Terminal(output) => output.terminal,
        other => panic!("an open got {other:?}"),
    }
}

async fn try_open(
    chief: &mut Client,
    hamlet: Option<&str>,
) -> ServerMessage {
    chief
        .ask(|id| {
            ClientMessage::Terminal(Terminal::Open {
                cols: 80,
                hamlet: hamlet.map(String::from),
                id,
                rows: 24,
            })
        })
        .await
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

/// A session with the hamlet `docs` working on a folder of its own,
/// and that folder.
async fn with_folder(faux: Faux) -> (Session, std::path::PathBuf) {
    let home = Home::new();
    let work = home.side().join("work");
    std::fs::create_dir(&work).unwrap();
    let mut s = Session::start_in(home, faux, QUIET).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    s.chief
        .command(ada, &format!("/folder {}", work.display()))
        .await;
    s.chief.command(ada, "/mode auto").await;
    (s, work)
}

#[tokio::test]
async fn the_chief_types_into_an_unbounded_terminal_resizes_and_closes_it()
 {
    let mut s = Session::start(Faux::new()).await;
    let id = open(&mut s.chief, None).await;
    let shown = view(&mut s.chief, &id).await;
    assert_eq!(shown.hamlet, None);
    assert_eq!(shown.agent, None);
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
async fn a_hamlet_terminal_writes_its_folder_and_no_lodge() {
    let (mut s, work) = with_folder(Faux::new()).await;
    let lodge = s.home.file("hamlets/docs/0");
    let id = open(&mut s.chief, Some("docs")).await;
    let shown = view(&mut s.chief, &id).await;
    assert_eq!(shown.hamlet.as_deref(), Some("docs"));
    assert_eq!(shown.agent, None);
    assert!(shown.sandboxed);
    type_in(
        &mut s.chief,
        &id,
        &format!(
            "pwd -P; echo made > here; touch '{}/mine' 2>/dev/null; \
             echo rc=$((0 + $?))\r",
            lodge.display()
        ),
    )
    .await;
    shows(&mut s.chief, &id, "rc=1").await;
    let folder = work.canonicalize().unwrap();
    assert!(
        printed(&s.chief, &id).contains(folder.to_str().unwrap()),
        "{}",
        printed(&s.chief, &id)
    );
    assert_eq!(
        std::fs::read_to_string(work.join("here")).unwrap(),
        "made\n"
    );
    assert!(!lodge.join("mine").exists());
}

#[tokio::test]
async fn a_hamlet_with_no_folder_has_no_terminal_of_the_chief_s() {
    let mut s = Session::start(Faux::new()).await;
    s.found("docs", "ada", "keep the docs").await;
    let answer = try_open(&mut s.chief, Some("docs")).await;
    assert!(
        matches!(
            answer,
            ServerMessage::Reply(ref reply)
                if reply.kind == ReplyKind::Error
                    && reply.text.contains("no folder")
        ),
        "{answer:?}"
    );
}

#[tokio::test]
async fn a_terminal_keeps_its_last_command_and_the_name_the_chief_gives_it()
 {
    let (mut s, _work) = with_folder(Faux::new()).await;
    let id = open(&mut s.chief, Some("docs")).await;
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
async fn an_agent_opens_and_types_into_a_terminal_and_its_bash_commands_show()
 {
    let faux = Faux::new().agent(
        2,
        [
            call("terminal_open", json!({"name": "dev"})),
            call(
                "terminal_write",
                json!({"terminal": "dev", "text": "echo agent-$((6 * 7))"}),
            ),
            call("bash", json!({"command": "echo ran-$((2 + 3))"})),
            words("Done."),
        ],
    );
    let mut s = Session::start(faux).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let now = chat(ada);
    s.chief.open(&now).await;
    s.chief.tell(ada, "Work in a terminal.").await;
    s.chief.shows(&now, "Done.").await;
    let terminals = s.chief.realm().terminals;
    let dev = terminals
        .iter()
        .find(|t| t.kind == TerminalKind::Shell)
        .expect("the agent's terminal");
    assert_eq!(dev.name, "dev");
    assert_eq!(dev.agent, Some(ada));
    assert_eq!(dev.hamlet.as_deref(), Some("docs"));
    assert!(dev.sandboxed);
    let commands = terminals
        .iter()
        .find(|t| t.kind == TerminalKind::Commands)
        .expect("the agent's commands");
    assert_eq!(commands.agent, Some(ada));
    assert_eq!(commands.name, "docs#0's commands");
    let tools = s
        .chief
        .items(&now)
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
    let (dev, commands) = (dev.id.clone(), commands.id.clone());
    watch(&mut s.chief, &dev).await;
    shows(&mut s.chief, &dev, "agent-42").await;
    watch(&mut s.chief, &commands).await;
    shows(&mut s.chief, &commands, "echo ran-$((2 + 3))").await;
    shows(&mut s.chief, &commands, "ran-5").await;
    // The chief types into the agent's terminal too.
    type_in(&mut s.chief, &dev, "echo chief-$((1 + 1))\r").await;
    shows(&mut s.chief, &dev, "chief-2").await;
}

#[tokio::test]
async fn the_hand_opens_a_terminal_of_no_hamlet_under_the_sandbox() {
    let faux = Faux::new().agent(
        1,
        [
            call("terminal_open", json!({"name": "errands"})),
            call(
                "terminal_write",
                json!({"terminal": "errands", "text": "pwd -P"}),
            ),
            words("Opened."),
        ],
    );
    let mut s = Session::start(faux).await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "Open a terminal.").await;
    s.chief.shows(&now, "Opened.").await;
    let shown = s
        .chief
        .realm()
        .terminals
        .into_iter()
        .find(|t| t.kind == TerminalKind::Shell)
        .expect("the Hand's terminal");
    assert_eq!(shown.agent, Some(hand));
    assert_eq!(shown.hamlet, None);
    assert!(shown.sandboxed);
    watch(&mut s.chief, &shown.id).await;
    let lodge = s.home.file("hand").canonicalize().unwrap();
    shows(&mut s.chief, &shown.id, lodge.to_str().unwrap()).await;
}

#[tokio::test]
async fn a_worker_s_terminals_close_when_it_answers() {
    let faux = Faux::new()
        .agent(
            2,
            [
                call(
                    "start_worker",
                    json!({"task": "use a terminal", "wait": true}),
                ),
                words("The worker said 42."),
            ],
        )
        .kind(
            "worker",
            [
                call("terminal_open", json!({"name": "scratch"})),
                call("bash", json!({"command": "echo w"})),
                words("42"),
            ],
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
    assert!(opened.contains(&TerminalKind::Shell), "{opened:?}");
    assert!(opened.contains(&TerminalKind::Commands), "{opened:?}");
    s.chief
        .until("the worker's terminals gone", |c| {
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
    let id = open(&mut chief, None).await;
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
