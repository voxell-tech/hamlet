//! The desktop service: the machine the engine runs on. It keeps the
//! machine awake while an agent works, opens a web address or a file
//! for a plugin, and tells of the address under the pointer. A plugin
//! asks with a [`DesktopRequest`] and is answered with a
//! [`DesktopReply`].

use serde::{Deserialize, Serialize};

use crate::capability::Capability;

/// The target of `desktop.open` for any `https` address. Every other
/// target is a folder, written whole or from `~/`, whose files and
/// folders may be opened.
pub const WEB: &str = "https";

/// The target of `desktop.drop`: the files the person drops.
pub const FILES: &str = "files";

/// What a plugin asks of the desktop service.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DesktopRequest {
    /// Sends each [`DesktopEvent`] from now on, as
    /// [`DesktopReply::Event`] with this request's ticket.
    Watch,
    /// The settings: [`DesktopReply::Settings`].
    Settings,
    /// Keeps the machine awake while an agent works, or not:
    /// [`DesktopReply::Settings`].
    SetKeepAwake(bool),
    /// Opens an `https` address in the browser, or a file or folder
    /// under a folder the plugin asked for in the program the system
    /// keeps for it. A file the system would run is shown in its
    /// folder instead.
    Open(String),
    /// The address under the pointer, or none once it left it.
    Hover(Option<String>),
}

impl DesktopRequest {
    /// The capability it needs: `desktop.open` on [`WEB`] or a
    /// folder, or `desktop.settings` on
    /// [`LOCAL`](crate::agents::LOCAL).
    pub fn capability(&self) -> Option<Capability> {
        match self {
            Self::Open(_) => Some(Capability::DesktopOpen),
            Self::SetKeepAwake(_) => {
                Some(Capability::DesktopSettings)
            }
            Self::Watch | Self::Settings | Self::Hover(_) => None,
        }
    }
}

/// What a request of the desktop service came to.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DesktopReply {
    Done,
    Watching,
    Settings(DesktopSettings),
    Event(DesktopEvent),
}

/// A change, sent to each plugin that watches.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DesktopEvent {
    Settings(DesktopSettings),
    /// The address under the pointer, or none.
    Hovered(Option<String>),
}

/// The desktop's settings, kept in the engine's home.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct DesktopSettings {
    /// The machine is kept awake while an agent works.
    pub keep_awake: bool,
}
