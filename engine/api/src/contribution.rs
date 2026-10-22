//! What a plugin adds to the engine, as its entry point declares it.

use serde::{Deserialize, Serialize};

use crate::layout::Layout;

/// One thing a plugin adds. A name is the plugin's own, and its full
/// id is the plugin's id and the name. A command is named by its
/// name, for one of the plugin's own, or by its full id.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Contribution {
    /// A panel for the dock, its tab showing the icon `icon` by its
    /// name, if it has one.
    Panel {
        name: String,
        title: String,
        icon: Option<String>,
    },
    /// A command a menu calls `label`.
    Command { name: String, label: String },
    /// The command `command` in the bar's menu titled `menu`.
    MenuEntry { menu: String, command: String },
    /// Keys bound to the command `command`, written as
    /// `Primary+Shift+H`.
    Shortcut { keys: String, command: String },
    /// A page of settings, its link row showing the icon `icon` by
    /// its name and what it holds, `about`.
    SettingsPage {
        name: String,
        title: String,
        about: String,
        icon: String,
    },
    /// What Settings shows under the link rows to its pages, built as
    /// a page is.
    SettingsFooter { name: String },
    /// A workspace the person can take.
    Workspace {
        name: String,
        title: String,
        layout: Layout,
    },
    /// An item of the status line, after those that came before it,
    /// hidden until the plugin gives it words. One that grows takes
    /// the room left on the line, words or none, and clips its words.
    StatusItem { name: String, grow: bool },
    /// A card over the dock, on a backdrop that takes every press,
    /// shown while the plugin says so. Its view is built as a
    /// panel's is.
    Overlay { name: String },
    /// A section of the engine's panel `panel` (`engine/name`), one
    /// that takes sections, headed `title`, and an installed plugin's
    /// under its name too. It is shown while the plugin says it holds
    /// something, and its view is built as a panel's is.
    PanelSection {
        panel: String,
        name: String,
        title: String,
    },
}
