//! The notes on disk: an index under one key, and the words of each
//! note under a key of its own.

use hamlet_guest::storage;

use crate::model::{Index, Prefs};

const INDEX: &str = "index";
const PREFS: &str = "prefs";

fn key(id: u64) -> String {
    format!("note.{id}")
}

/// The index, empty where there is none or it cannot be read.
pub fn index() -> Index {
    storage::load(INDEX).unwrap_or_default()
}

pub fn save_index(index: &Index) -> Result<(), String> {
    storage::save(INDEX, index)
}

/// What the chief chose, the defaults where nothing was chosen.
pub fn prefs() -> Prefs {
    storage::load(PREFS).unwrap_or_default()
}

pub fn save_prefs(prefs: &Prefs) -> Result<(), String> {
    storage::save(PREFS, prefs)
}

/// The words of a note, empty where there are none.
pub fn body(id: u64) -> String {
    storage::load(&key(id)).unwrap_or_default()
}

pub fn save_body(id: u64, body: &str) -> Result<(), String> {
    storage::save(&key(id), &body)
}

pub fn delete(id: u64) {
    storage::delete(&key(id));
}
