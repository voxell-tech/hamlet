//! The panel: a search, the list of notes and the open note.

use hamlet_guest::ui::{
    Node, button, checkbox, column, divider, heading, icon, markdown,
    note, row, text_area, text_field,
};

use crate::model::{Entry, Index, MAX_SHOWN, Session};
use crate::storage;

/// The panel for these notes and this session.
pub fn panel(session: &Session, index: &Index) -> Node {
    let shown = matching(session, index);
    let mut rows = vec![
        row([
            text_field(&session.search).id("search"),
            button("New").id("new"),
        ]),
        row([button("Add from the agent's notes").id("from-agent")]),
    ];
    if !session.hint.is_empty() {
        rows.push(note(&session.hint).id("hint"));
    }
    rows.push(divider());
    rows.push(list(session, index, &shown));
    rows.push(divider());
    rows.push(open(session, index));
    column(rows)
}

/// The notes whose title or words hold the search, newest first.
fn matching<'a>(
    session: &Session,
    index: &'a Index,
) -> Vec<&'a Entry> {
    let wanted = session.search.trim().to_lowercase();
    index
        .notes
        .iter()
        .filter(|entry| {
            wanted.is_empty()
                || entry.title.to_lowercase().contains(&wanted)
                || storage::body(entry.id)
                    .to_lowercase()
                    .contains(&wanted)
        })
        .collect()
}

fn list(session: &Session, index: &Index, shown: &[&Entry]) -> Node {
    if shown.is_empty() {
        let words = match index.notes.is_empty() {
            true => "No notes yet. Press New.",
            false => "No note matches.",
        };
        return column([note(words)]);
    }
    let mut rows = shown
        .iter()
        .take(MAX_SHOWN)
        .map(|entry| match session.selected == Some(entry.id) {
            true => row([icon("notes"), heading(&entry.title)]),
            false => row([button(&entry.title)
                .id(&format!("note.{}", entry.id))]),
        })
        .collect::<Vec<_>>();
    if shown.len() > MAX_SHOWN {
        rows.push(row([note(&format!(
            "{} more. Search to narrow them.",
            shown.len() - MAX_SHOWN
        ))]));
    }
    column(rows)
}

/// The selected note: its words to edit, or drawn as markdown.
fn open(session: &Session, index: &Index) -> Node {
    let Some(id) = session.selected.filter(|id| index.has(*id))
    else {
        return column([note("Select a note, or press New.")]);
    };
    let body = storage::body(id);
    let delete = match session.sure == Some(id) {
        true => "Sure? Delete it",
        false => "Delete",
    };
    let words = match session.preview {
        true => markdown(&body),
        false => text_area(&body).id("body"),
    };
    column([
        row([
            checkbox("Preview", session.preview).id("preview"),
            button(delete).id("delete"),
        ]),
        words,
    ])
}
