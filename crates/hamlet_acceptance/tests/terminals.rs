//! Terminals: the chief's own, typed into and unsandboxed, the
//! read-only commands of an agent's shell tool, and what a restart
//! keeps of them.

use std::path::Path;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use serde_json::json;

/// The prompt of the chief's shells, short enough that a line typed
/// after it does not wrap.
const PROMPT: &str = "ready> ";

/// [`QUIET`] with the chief's terminals starting zsh, else `sh`,
/// showing [`PROMPT`].
fn known_shell(home: &Home) -> Vec<(&'static str, String)> {
    let shell = ["/bin/zsh", "/usr/bin/zsh"]
        .into_iter()
        .find(|zsh| Path::new(zsh).is_file())
        .unwrap_or("/bin/sh");
    // zsh reads the user's startup files from here, after the
    // system's, which set a prompt of their own.
    let startup = home.side().join("zsh");
    std::fs::create_dir_all(&startup).unwrap();
    let rc = startup.join(".zshrc");
    std::fs::write(&rc, format!("PS1='{PROMPT}'\n")).unwrap();
    let mut env = QUIET
        .iter()
        .map(|&(name, value)| (name, value.to_owned()))
        .collect::<Vec<_>>();
    env.push(("SHELL", shell.into()));
    env.push(("ZDOTDIR", startup.display().to_string()));
    if shell == "/bin/sh" {
        // `sh` reads the startup files of `HOME` and `ENV`, and sets
        // its prompt from `ENV`'s alone.
        env.retain(|(name, _)| !matches!(*name, "HOME" | "ENV"));
        env.push(("HOME", startup.display().to_string()));
        env.push(("ENV", rc.display().to_string()));
    }
    env
}

async fn serve(home: &mut Home, faux: &Faux) -> Server {
    let env = known_shell(home);
    let env = env
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect::<Vec<_>>();
    home.serve(faux, &env).await
}

async fn session(faux: Faux) -> Session {
    let mut home = Home::new();
    let server = serve(&mut home, &faux).await;
    let chief = Client::join(&server).await;
    Session {
        home,
        server,
        chief,
    }
}

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

/// Opens a terminal and waits for its prompt: keys typed before it
/// shows are not read as following it.
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
    let id = match answer {
        ServerMessage::Terminal(output) => output.terminal,
        other => panic!("an open got {other:?}"),
    };
    shows(chief, &id, PROMPT).await;
    id
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
    let mut s = session(Faux::new()).await;
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
async fn a_program_in_a_terminal_gets_its_queries_answered() {
    let mut s = session(Faux::new()).await;
    let id = open(&mut s.chief).await;
    // Each query's reply is read back from the terminal's input and
    // shown byte by byte, as a line editor waiting for it would.
    let ask = |query: &str, reply: usize| {
        format!(
            "printf '{query}'; dd bs=1 count={reply} 2>/dev/null | \
             od -An -c | tr -d ' '; "
        )
    };
    let command = format!(
        "stty -icanon -echo min 1; {}{}{}stty sane\r",
        ask("\\033[5n", 4),
        ask("\\033[H\\033[6n", 6),
        ask("\\033[c", 9),
    );
    type_in(&mut s.chief, &id, &command).await;
    shows(&mut s.chief, &id, "033[0n").await;
    shows(&mut s.chief, &id, "033[1;1R").await;
    shows(&mut s.chief, &id, "033[?62;22c").await;
}

#[tokio::test]
async fn a_terminal_keeps_its_last_command_and_the_name_the_chief_gives_it()
 {
    let mut s = session(Faux::new()).await;
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
    let mut s = session(faux).await;
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
    assert_eq!(commands.name, format!("agent#{}'s commands", ada.0));
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
    let mut s = session(faux).await;
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
    let mut s = session(faux).await;
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
    let mut server = serve(&mut home, &Faux::new()).await;
    let mut chief = Client::join(&server).await;
    let id = open(&mut chief).await;
    type_in(&mut chief, &id, "echo before-$((3 * 3))\r").await;
    shows(&mut chief, &id, "before-9").await;
    chief.close().await;
    server.stop().await;
    let server = serve(&mut home, &Faux::new()).await;
    let mut chief = Client::join(&server).await;
    assert_eq!(view(&mut chief, &id).await.exit, None);
    watch(&mut chief, &id).await;
    shows(&mut chief, &id, "before-9").await;
    shows(&mut chief, &id, "restarted").await;
    type_in(&mut chief, &id, "echo after-$((4 * 4))\r").await;
    shows(&mut chief, &id, "after-16").await;
    chief.settle(Duration::from_millis(100)).await;
}
