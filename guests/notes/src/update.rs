//! What an event does to the notes and to what the panel shows, and
//! the answers and events it sends other plugins.

use hamlet_guest::{
    Incoming, Intent, Level, RequestResult, calls, events, log,
};

use crate::model::{Index, MAX_NOTE, Session, cut};
use crate::{PANEL, storage};

/// The capability the copy of an agent's notes needs.
const PAPERS: &str = "read.papers";

const REASON: &str =
    "To copy the selected agent's notes into a note of your own.";

/// The command that adds a note with the words it is given.
pub const ADD: &str = "add";

/// The command that adds an empty note and opens it. The chief runs
/// it from the menu and the keyboard.
pub const NEW: &str = "new";

/// The event of a note that was saved. Its payload is the note's
/// number.
pub const SAVED: &str = "note-saved";

/// Carries out one event. `out` gets what the plugin sends to
/// others.
pub fn handle(
    incoming: Incoming,
    session: &mut Session,
    index: &mut Index,
    out: &mut Vec<Intent>,
) {
    match incoming {
        Incoming::Click(press) => {
            click(&press.id, session, index, out)
        }
        Incoming::Toggled(toggle) if toggle.id == "preview" => {
            session.preview = toggle.on;
        }
        Incoming::TextChanged(typed) => {
            text(&typed.id, &typed.text, session, index, out);
        }
        Incoming::Granted(name) if name == PAPERS => {
            if std::mem::take(&mut session.waiting) {
                session.hint.clear();
                copy(session, index, out);
            }
        }
        Incoming::Denied(name) if name == PAPERS => {
            session.waiting = false;
            session.hint = "Not allowed to read the agent's notes, \
                            so none were copied."
                .into();
        }
        Incoming::Command(call) => {
            let added =
                match call.command.as_str() {
                    ADD => calls::args::<String>(&call)
                        .and_then(|words| add(index, &words))
                        .inspect(|id| saved(*id, out)),
                    NEW => open_new(session, index, "", out).inspect(
                        |_| out.push(hamlet_guest::open_panel(PANEL)),
                    ),
                    other => Err(format!("no command `{other}`")),
                };
            out.push(calls::reply(
                &call,
                added.as_ref().map_err(String::clone),
            ));
        }
        _ => {}
    }
}

fn click(
    id: &str,
    session: &mut Session,
    index: &mut Index,
    out: &mut Vec<Intent>,
) {
    let sure = session.sure.take();
    session.hint.clear();
    match id {
        "new" => create(session, index, "", out),
        "from-agent" => ask(session, index, out),
        "delete" => {
            if let Some(selected) = session.selected
                && sure == Some(selected)
            {
                remove(selected, session, index);
            } else {
                session.sure = session.selected;
            }
        }
        other => {
            let chosen = other
                .strip_prefix("note.")
                .and_then(|id| id.parse::<u64>().ok());
            if let Some(id) = chosen.filter(|id| index.has(*id)) {
                session.selected = Some(id);
            }
        }
    }
}

fn text(
    id: &str,
    text: &str,
    session: &mut Session,
    index: &mut Index,
    out: &mut Vec<Intent>,
) {
    match id {
        "search" => session.search = text.to_string(),
        "body" => {
            let Some(id) =
                session.selected.filter(|id| index.has(*id))
            else {
                return;
            };
            let body = cut(text, MAX_NOTE);
            if body.len() < text.len() {
                session.hint = format!(
                    "A note keeps {MAX_NOTE} bytes at most. The \
                     rest was cut."
                );
            }
            index.touch(id, body);
            match storage::save_body(id, body)
                .and_then(|()| storage::save_index(index))
            {
                Ok(()) => saved(id, out),
                Err(why) => session.hint = why,
            }
        }
        _ => {}
    }
}

/// Adds a note with these words, and returns its number.
fn add(index: &mut Index, body: &str) -> Result<u64, String> {
    let body = cut(body, MAX_NOTE);
    let id = index.add(body);
    storage::save_body(id, body)
        .and_then(|()| storage::save_index(index))
        .inspect_err(|_| {
            index.remove(id);
            storage::delete(id);
        })?;
    Ok(id)
}

/// Adds a note with these words and selects it, or says why not.
fn create(
    session: &mut Session,
    index: &mut Index,
    body: &str,
    out: &mut Vec<Intent>,
) {
    if let Err(why) = open_new(session, index, body, out) {
        session.hint = why;
    }
}

/// Adds a note with these words, selects it and returns its number.
fn open_new(
    session: &mut Session,
    index: &mut Index,
    body: &str,
    out: &mut Vec<Intent>,
) -> Result<u64, String> {
    let id = add(index, body)?;
    session.selected = Some(id);
    session.preview = false;
    saved(id, out);
    Ok(id)
}

/// Tells the plugins that hear of it that the note `id` was saved.
fn saved(id: u64, out: &mut Vec<Intent>) {
    match events::publish(SAVED, &id) {
        Ok(intent) => out.push(intent),
        Err(why) => log(Level::Warn, &why),
    }
}

fn remove(id: u64, session: &mut Session, index: &mut Index) {
    index.remove(id);
    storage::delete(id);
    if let Err(why) = storage::save_index(index) {
        session.hint = why;
    }
    session.selected = None;
}

/// Asks to use the agent's notes, and copies them if the chief
/// allows it.
fn ask(
    session: &mut Session,
    index: &mut Index,
    out: &mut Vec<Intent>,
) {
    match hamlet_guest::request(PAPERS, REASON) {
        RequestResult::Granted => copy(session, index, out),
        RequestResult::Pending => {
            session.waiting = true;
            session.hint = "Waiting for you to answer, to read the \
                            agent's notes."
                .into();
        }
        RequestResult::Denied => {
            session.hint = "Reading the agent's notes is not \
                            allowed. Allow read.papers in Settings."
                .into();
        }
    }
}

fn copy(
    session: &mut Session,
    index: &mut Index,
    out: &mut Vec<Intent>,
) {
    match hamlet_guest::papers() {
        Ok(papers) if papers.notes.trim().is_empty() => {
            session.hint = "The selected agent has no notes. Select \
                            an agent in the world first."
                .into();
        }
        Ok(papers) => create(session, index, &papers.notes, out),
        Err(why) => session.hint = why,
    }
}
