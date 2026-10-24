//! What a WASM plugin shows outside its panels and settings pages,
//! as it changes it in a call: kept to be made once the call ends.

use crate::instance::ui_bindings::hamlet::engine::ui::{
    MenuItem, Note, StatusItem,
};

/// A change to something a plugin added, named by its name within
/// the plugin.
#[derive(Clone, Debug)]
pub enum SurfaceChange {
    /// What its status item shows.
    Status { name: String, item: StatusItem },
    /// Its overlay shown or hidden.
    Overlay { name: String, shown: bool },
    /// How many things its panel section holds.
    Section { name: String, holds: u32 },
    /// Its panel or settings page brought to the front, or the panel
    /// of a full id, any plugin's.
    Open(String),
    /// Its panel, or the panel of a full id, told `note`.
    Notify { name: String, note: Note },
    /// A menu of `items` opened at the pointer, its choice told to
    /// `key`.
    Menu { key: u32, items: Vec<MenuItem> },
}
