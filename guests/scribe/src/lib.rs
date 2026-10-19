//! A plugin that works in a folder the chief chooses: it lists the
//! folder, reads its notes, asks the model to summarise them and
//! keeps the summary there. It also steers the agents of Home: a
//! short reminder before each model call, and a command that deletes
//! a folder held back. Each is behind a grant, and an answer or a
//! refusal is shown as a line in the panel.

use hamlet_guest::folder::{self, FolderResult};
use hamlet_guest::ui::{
    View, button, column, divider, heading, note, row,
};
use hamlet_guest::{
    Incoming, Intent, Level, Plugin, State, model, steer,
};
use serde::{Deserialize, Serialize};

const PANEL: &str = "scribe";

/// The notes it reads, and the summary it keeps, in the folder.
const NOTES: &str = "tides.md";
const SUMMARY: &str = "summary.txt";

/// The numbers its calls go by.
const PICK: u32 = 1;
const LIST: u32 = 2;
const READ: u32 = 3;
const WRITE: u32 = 4;
const ASK: u32 = 5;

/// What it reminds the agents of before each model call.
const REMINDER: &str = "Keep replies short.";

/// What the panel shows and what it read, which the host keeps
/// across a reload.
#[derive(Default, Serialize, Deserialize)]
struct Kept {
    /// The notes it read last.
    notes: String,
    /// The model's last summary of them.
    summary: String,
    /// What came of the last thing done.
    lines: Vec<String>,
}

impl Kept {
    fn say(&mut self, line: impl Into<String>) {
        self.lines.insert(0, line.into());
        self.lines.truncate(6);
    }
}

struct Scribe;

impl Plugin for Scribe {
    fn entry() {
        hamlet_guest::panel(PANEL, "SCRIBE", "book");
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let mut kept = State::<Kept>::load();
        let mut out = Vec::new();
        let mut heard = false;
        for incoming in inbox {
            // A point of a turn is answered and not shown.
            if let Some(point) = steer::point(&incoming) {
                out.extend(steered(point));
                continue;
            }
            heard = true;
            handle(incoming, &mut kept, &mut out);
        }
        if let Err(why) = State::save(&kept) {
            hamlet_guest::log(Level::Warn, &why);
        }
        if heard {
            out.push(Intent::Redraw(PANEL.into()));
        }
        out
    }

    fn panel_view(_panel: &str) -> View {
        let kept = State::<Kept>::load();
        let mut rows = vec![
            heading("SCRIBE"),
            row([
                button("Choose a folder").id("pick"),
                button("List it").id("list"),
                button("Read the notes").id("read"),
            ]),
            row([
                button("Summarise").id("ask"),
                button("Keep the summary").id("write"),
                button("Escape the folder").id("escape"),
            ]),
            divider(),
        ];
        rows.extend(
            kept.lines.iter().enumerate().map(|(at, line)| {
                note(line).id(&format!("line-{at}"))
            }),
        );
        column(rows).into()
    }
}

/// What it makes of a point of an agent's turn.
fn steered(point: &steer::SteerPoint) -> Option<Intent> {
    match &point.tool {
        None => Some(steer::note(point, REMINDER)),
        Some(tool) if tool.args.contains("rm -rf") => {
            Some(steer::hold(
                point,
                "Scribe does not let a folder be deleted",
            ))
        }
        Some(_) => None,
    }
}

fn handle(
    incoming: Incoming,
    kept: &mut Kept,
    out: &mut Vec<Intent>,
) {
    if let Some((_, outcome)) = folder::picked(&incoming) {
        kept.say(match outcome {
            Ok(name) => format!("Works in {name}"),
            Err(why) => format!("No folder: {why}"),
        });
        return;
    }
    if let Some((id, outcome)) = folder::answer(&incoming) {
        let line = match (id, outcome) {
            (_, Err(why)) => format!("Refused: {why}"),
            (_, Ok(FolderResult::Listed(names))) => {
                format!("Holds {}", names.join(", "))
            }
            (READ, Ok(FolderResult::Read(bytes))) => {
                kept.notes =
                    String::from_utf8_lossy(bytes).into_owned();
                format!("Read {} bytes of notes", bytes.len())
            }
            (_, Ok(FolderResult::Read(bytes))) => {
                format!("Read {} bytes", bytes.len())
            }
            (_, Ok(FolderResult::Written)) => {
                format!("Kept {SUMMARY}")
            }
        };
        kept.say(line);
        return;
    }
    if let Some((_, outcome)) = model::answer(&incoming) {
        let line = match outcome {
            Ok(text) => {
                kept.summary = text.into();
                format!("Summary: {text}")
            }
            Err(why) => format!("No summary: {why}"),
        };
        kept.say(line);
        return;
    }
    match incoming {
        Incoming::Click(press) => click(&press.id, kept, out),
        Incoming::Granted(name) => {
            kept.say(format!("The chief allowed {name}"))
        }
        Incoming::Denied(name) => {
            kept.say(format!("The chief refused {name}"))
        }
        _ => {}
    }
}

fn click(id: &str, kept: &mut Kept, out: &mut Vec<Intent>) {
    match id {
        "pick" => {
            out.push(folder::pick(
                PICK,
                "a folder of notes to summarise",
            ));
            kept.say("Asked the chief for a folder");
        }
        "list" => out.push(folder::list(LIST, "")),
        "read" => out.push(folder::read(READ, NOTES)),
        "escape" => out.push(folder::read(READ, "../manifest.toml")),
        "ask" => out.push(model::ask(
            ASK,
            "You summarise notes in one short sentence.",
            &kept.notes,
            Some(64),
        )),
        "write" => {
            out.push(folder::write(
                WRITE,
                SUMMARY,
                kept.summary.as_bytes(),
            ));
            kept.say(format!("Asked to keep {SUMMARY}"));
        }
        _ => {}
    }
}

hamlet_guest::export!(Scribe);
