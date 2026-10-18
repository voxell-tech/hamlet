//! What an event does to the notes and to what the panel shows.

use hamlet_guest::{Incoming, RequestResult};

use crate::model::{Index, MAX_NOTE, Session, cut};
use crate::storage;

/// The capability the copy of an agent's notes needs.
const PAPERS: &str = "read.papers";

const REASON: &str =
    "To copy the selected agent's notes into a note of your own.";

/// Carries out one event.
pub fn handle(
    incoming: Incoming,
    session: &mut Session,
    index: &mut Index,
) {
    match incoming {
        Incoming::Click(press) => click(&press.id, session, index),
        Incoming::Toggled(toggle) if toggle.id == "preview" => {
            session.preview = toggle.on;
        }
        Incoming::TextChanged(typed) => {
            text(&typed.id, &typed.text, session, index);
        }
        Incoming::Granted(name) if name == PAPERS => {
            if std::mem::take(&mut session.waiting) {
                session.hint.clear();
                copy(session, index);
            }
        }
        Incoming::Denied(name) if name == PAPERS => {
            session.waiting = false;
            session.hint = "Not allowed to read the agent's notes, \
                            so none were copied."
                .into();
        }
        _ => {}
    }
}

fn click(id: &str, session: &mut Session, index: &mut Index) {
    let sure = session.sure.take();
    session.hint.clear();
    match id {
        "new" => create(session, index, ""),
        "from-agent" => ask(session, index),
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
            if let Err(why) = storage::save_body(id, body)
                .and_then(|()| storage::save_index(index))
            {
                session.hint = why;
            }
        }
        _ => {}
    }
}

/// Adds a note with these words and selects it.
fn create(session: &mut Session, index: &mut Index, body: &str) {
    let body = cut(body, MAX_NOTE);
    let id = index.add(body);
    let saved = storage::save_body(id, body)
        .and_then(|()| storage::save_index(index));
    match saved {
        Ok(()) => {
            session.selected = Some(id);
            session.preview = false;
        }
        Err(why) => {
            index.remove(id);
            storage::delete(id);
            session.hint = why;
        }
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
fn ask(session: &mut Session, index: &mut Index) {
    match hamlet_guest::request(PAPERS, REASON) {
        RequestResult::Granted => copy(session, index),
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

fn copy(session: &mut Session, index: &mut Index) {
    match hamlet_guest::papers() {
        Ok(papers) if papers.notes.trim().is_empty() => {
            session.hint = "The selected agent has no notes. Select \
                            an agent in the world first."
                .into();
        }
        Ok(papers) => create(session, index, &papers.notes),
        Err(why) => session.hint = why,
    }
}
