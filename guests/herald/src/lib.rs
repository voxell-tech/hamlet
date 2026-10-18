//! A plugin that acts for the chief: it sends a message to an agent,
//! runs a command, keeps a draft in its own folder of files and
//! fetches a page. Each is behind a grant, and an answer or a refusal
//! is shown as a line in the panel.

use hamlet_guest::net::{self, FetchResponse};
use hamlet_guest::ui::{
    View, button, column, divider, heading, label, note, row,
    text_field,
};
use hamlet_guest::{Incoming, Intent, Level, Plugin, State, files};
use serde::{Deserialize, Serialize};

const PANEL: &str = "herald";

/// The file the draft is kept in.
const DRAFT: &str = "drafts/hello.txt";

/// The number the page fetch goes by.
const PAGE: u32 = 1;

/// The page the manifest lists a host for.
const ADDRESS: &str = "https://api.example.com/hello";

/// The command the herald runs, which its ask is limited to.
const COMMAND: &str = "notes/new";

/// What the panel shows, which the host keeps across a reload.
#[derive(Default, Serialize, Deserialize)]
struct Model {
    /// The name of the agent to send to.
    agent: String,
    /// What came of the last thing done.
    lines: Vec<String>,
}

impl Model {
    fn say(&mut self, line: impl Into<String>) {
        self.lines.insert(0, line.into());
        self.lines.truncate(6);
    }
}

struct Herald;

impl Plugin for Herald {
    fn entry() {
        hamlet_guest::panel(PANEL, "HERALD", "robot");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let mut model = State::<Model>::load();
        let mut out = Vec::new();
        let mut heard = false;
        for incoming in inbox {
            heard |= !matches!(incoming, Incoming::Tick(_));
            handle(incoming, &mut model, &mut out);
        }
        if let Err(why) = State::save(&model) {
            hamlet_guest::log(Level::Warn, &why);
        }
        if heard {
            // An answer or a grant is not an event of the panel.
            out.push(Intent::Redraw(PANEL.into()));
        }
        out
    }

    fn panel_view(_panel: &str) -> View {
        let model = State::<Model>::load();
        let mut rows = vec![
            heading("HERALD"),
            row([
                label("Agent"),
                text_field(&model.agent).id("agent"),
            ]),
            row([
                button("Say hello").id("hello"),
                button("Run Notes: new").id("run"),
            ]),
            row([
                button("Save a draft").id("save"),
                button("Read the draft").id("read"),
                button("Escape the folder").id("escape"),
            ]),
            row([
                button("Fetch a page").id("fetch"),
                button("Fetch elsewhere").id("elsewhere"),
            ]),
            divider(),
        ];
        rows.extend(
            model.lines.iter().enumerate().map(|(at, line)| {
                note(line).id(&format!("line-{at}"))
            }),
        );
        column(rows).into()
    }
}

fn handle(
    incoming: Incoming,
    model: &mut Model,
    out: &mut Vec<Intent>,
) {
    if let Some((id, outcome)) = net::answer(&incoming) {
        model.say(match outcome {
            Ok(FetchResponse { status, body, .. }) => {
                format!("Fetch {id}: {status}, {} bytes", body.len())
            }
            Err(why) => format!("Fetch {id}: {why}"),
        });
        return;
    }
    match incoming {
        Incoming::TextChanged(typed) if typed.id == "agent" => {
            model.agent = typed.text;
        }
        Incoming::Click(press) => click(&press.id, model, out),
        Incoming::Granted(name) => {
            model.say(format!("The chief allowed {name}"))
        }
        Incoming::Denied(name) => {
            model.say(format!("The chief refused {name}"))
        }
        _ => {}
    }
}

fn click(id: &str, model: &mut Model, out: &mut Vec<Intent>) {
    match id {
        "hello" => {
            out.push(hamlet_guest::send_chat(
                &model.agent,
                "Hello from the Herald.",
            ));
            model.say(format!(
                "Asked to say hello to {}",
                model.agent
            ));
        }
        "run" => {
            out.push(hamlet_guest::run_as_chief(COMMAND));
            model.say(format!("Asked to run {COMMAND}"));
        }
        "save" => {
            model.say(match files::write(DRAFT, b"hello draft") {
                Ok(()) => format!("Saved {DRAFT}"),
                Err(why) => format!("Not saved: {why}"),
            })
        }
        "read" => model.say(match files::read(DRAFT) {
            Ok(bytes) => {
                format!("Draft: {}", String::from_utf8_lossy(&bytes))
            }
            Err(why) => format!("Not read: {why}"),
        }),
        "escape" => {
            model.say(match files::read("../manifest.toml") {
                Ok(_) => "Read outside the folder".to_string(),
                Err(why) => format!("Refused: {why}"),
            })
        }
        "fetch" => {
            out.push(net::get(PAGE, ADDRESS));
            model.say(format!("Asked for {ADDRESS}"));
        }
        "elsewhere" => {
            out.push(net::get(PAGE + 1, "https://evil.example.org/"));
        }
        _ => {}
    }
}

hamlet_guest::export!(Herald);
