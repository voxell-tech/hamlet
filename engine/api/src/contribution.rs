//! What a plugin adds to the engine, as its entry point declares it.

use serde::{Deserialize, Serialize};

use crate::layout::Layout;

/// One thing a plugin adds. A name is the plugin's own, and its full
/// id is the plugin's id and the name. A command is named by its
/// name, for one of the plugin's own, or by its full id.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Contribution {
    /// A panel for the dock.
    Panel { name: String, title: String },
    /// A command a menu calls `label`.
    Command { name: String, label: String },
    /// The command `command` in the bar's menu titled `menu`.
    MenuEntry { menu: String, command: String },
    /// Keys bound to the command `command`, written as
    /// `Primary+Shift+H`.
    Shortcut { keys: String, command: String },
    /// A page of settings.
    SettingsPage { name: String, title: String },
    /// A workspace the person can take.
    Workspace {
        name: String,
        title: String,
        layout: Layout,
    },
}
