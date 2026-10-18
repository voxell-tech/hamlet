//! What the plugin knows: the notes it keeps and what its panel is
//! showing.

use serde::{Deserialize, Serialize};

/// The most bytes of a note, kept under the host's limit on one text
/// of a panel.
pub const MAX_NOTE: usize = 4000;

/// The most bytes of a title.
const MAX_TITLE: usize = 60;

/// The most notes the list shows at once.
pub const MAX_SHOWN: usize = 12;

/// One note in the index: its number and the title the list shows.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub title: String,
}

/// The notes, newest first, without their words. It is kept in
/// storage under one key, and each note's words under a key of their
/// own.
#[derive(Default, Serialize, Deserialize)]
pub struct Index {
    /// The number the next note gets.
    pub next: u64,
    pub notes: Vec<Entry>,
}

impl Index {
    /// Adds a note with these words at the head, and returns its
    /// number.
    pub fn add(&mut self, body: &str) -> u64 {
        let id = self.next;
        self.next += 1;
        self.notes.insert(
            0,
            Entry {
                id,
                title: title_of(body),
            },
        );
        id
    }

    /// Makes the note `id` the newest, with a title for these words.
    pub fn touch(&mut self, id: u64, body: &str) {
        let Some(at) = self.notes.iter().position(|e| e.id == id)
        else {
            return;
        };
        let mut entry = self.notes.remove(at);
        entry.title = title_of(body);
        self.notes.insert(0, entry);
    }

    pub fn remove(&mut self, id: u64) {
        self.notes.retain(|entry| entry.id != id);
    }

    pub fn has(&self, id: u64) -> bool {
        self.notes.iter().any(|entry| entry.id == id)
    }
}

/// The first line of `body`, cut to a title, or a name for a note
/// with no words.
pub fn title_of(body: &str) -> String {
    let line = body.lines().find(|line| !line.trim().is_empty());
    match line.map(str::trim) {
        Some(line) => cut(line, MAX_TITLE).to_string(),
        None => "Untitled".to_string(),
    }
}

/// `text` cut to at most `bytes`, at a character.
pub fn cut(text: &str, bytes: usize) -> &str {
    &text[..text.floor_char_boundary(bytes)]
}

/// What the panel is showing, which the host keeps across a reload.
#[derive(Default, Serialize, Deserialize)]
pub struct Session {
    pub selected: Option<u64>,
    pub search: String,
    /// Whether the selected note is shown as drawn markdown.
    pub preview: bool,
    /// The note whose Delete was pressed once and waits for a
    /// second.
    pub sure: Option<u64>,
    /// A line under the buttons, such as why a copy was not made.
    pub hint: String,
    /// A copy of the agent's notes waits for the chief to answer.
    pub waiting: bool,
}
