//! What a WASM plugin shows outside its panels and settings pages,
//! as it changes it in a call: kept to be made once the call ends.

use crate::instance::ui_bindings::hamlet::engine::ui::StatusItem;

/// A change to something a plugin added, named by its name within
/// the plugin.
#[derive(Clone, Debug)]
pub enum SurfaceChange {
    /// What its status item shows.
    Status { name: String, item: StatusItem },
    /// Its overlay shown or hidden.
    Overlay { name: String, shown: bool },
    /// Its panel or settings page brought to the front, or the panel
    /// of a full id, any plugin's.
    Open(String),
}
