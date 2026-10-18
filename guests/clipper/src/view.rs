//! The panel: the button, the reply, the notes in the collection and
//! the count of notes Notes saved.

use hamlet_guest::collection;
use hamlet_guest::ui::{
    Node, button, column, divider, heading, icon, label, note, row,
};

use crate::model::Model;
use crate::update::NOTES;

/// The most titles the panel lists.
const SHOWN: u32 = 8;

/// The most bytes of a title.
const TITLE: usize = 60;

pub fn panel(model: &Model) -> Node {
    let selected = match model.selected.is_empty() {
        true => "No agent is selected.".to_string(),
        false => format!("Selected: {}", model.selected),
    };
    let mut rows = vec![
        row([icon("notes"), heading("CLIP TO NOTES")]),
        note(&selected).id("selected"),
        row([button("Send the selected agent's name to Notes")
            .id("send")]),
    ];
    if !model.line.is_empty() {
        rows.push(note(&model.line).id("reply"));
    }
    rows.push(divider());
    rows.push(heading("IN NOTES"));
    rows.push(titles());
    rows.push(divider());
    rows.push(
        label(&format!("Notes saved: {}", model.saved)).id("saved"),
    );
    column(rows)
}

/// The titles of the notes read from the collection, or why they
/// are not shown.
fn titles() -> Node {
    match collection::list(NOTES, "notes", None, SHOWN) {
        Ok(page) if page.items.is_empty() => {
            note("Notes has no notes yet.").id("none")
        }
        Ok(page) => {
            let mut rows = page
                .items
                .iter()
                .map(|item| {
                    let body = collection::value::<String>(item)
                        .unwrap_or_default();
                    label(&title(&body))
                        .id(&format!("note.{}", item.key))
                })
                .collect::<Vec<_>>();
            if page.next.is_some() {
                rows.push(note("More notes than are listed."));
            }
            column(rows)
        }
        Err(why) => note(&format!("Not shown: {why}.")).id("denied"),
    }
}

/// The first line of a note, cut to a title.
fn title(body: &str) -> String {
    let line = body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Untitled");
    line[..line.floor_char_boundary(TITLE)].to_string()
}
