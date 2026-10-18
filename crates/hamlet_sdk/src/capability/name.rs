//! The names of the capabilities, and of the ones that no plugin can
//! be given.

use std::fmt;

/// The capabilities that can never be granted. A manifest that asks
/// for one is turned down at the scan, with no petition.
pub const NEVER: [&str; 6] = [
    "keys",
    "petitions.answer",
    "home.raw",
    "process.run",
    "sandbox.off",
    "storage.other",
];

/// A thing a plugin can be allowed to do. A manifest names the most
/// it may ask for; a host function checks the matching verdict on
/// each call.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Capability {
    ReadAgents,
    ReadChat,
    ReadActivity,
    ReadPapers,
    ChatSend,
    CommandsRun,
    UiMenu,
    Net,
    FsPick,
    FsFolder,
    ModelCall,
    PluginUse,
}

impl Capability {
    pub const ALL: [Capability; 12] = [
        Capability::ReadAgents,
        Capability::ReadChat,
        Capability::ReadActivity,
        Capability::ReadPapers,
        Capability::ChatSend,
        Capability::CommandsRun,
        Capability::UiMenu,
        Capability::Net,
        Capability::FsPick,
        Capability::FsFolder,
        Capability::ModelCall,
        Capability::PluginUse,
    ];

    /// The name a manifest and the grants file know it by.
    pub fn name(self) -> &'static str {
        match self {
            Capability::ReadAgents => "read.agents",
            Capability::ReadChat => "read.chat",
            Capability::ReadActivity => "read.activity",
            Capability::ReadPapers => "read.papers",
            Capability::ChatSend => "chat.send",
            Capability::CommandsRun => "commands.run",
            Capability::UiMenu => "ui.menu",
            Capability::Net => "net",
            Capability::FsPick => "fs.pick",
            Capability::FsFolder => "fs.folder",
            Capability::ModelCall => "model.call",
            Capability::PluginUse => "plugin.use",
        }
    }

    /// What holding it lets a plugin do, in a few words.
    pub fn words(self) -> &'static str {
        match self {
            Capability::ReadAgents => {
                "See the agents and the hamlets they live in"
            }
            Capability::ReadChat => "Read the chat",
            Capability::ReadActivity => {
                "See what the agents are doing"
            }
            Capability::ReadPapers => {
                "Read an agent's notes, character and skills"
            }
            Capability::ChatSend => "Send messages as the chief",
            Capability::CommandsRun => "Run commands as the chief",
            Capability::UiMenu => "Add entries to the menus",
            Capability::Net => "Reach the network",
            Capability::FsPick => {
                "Ask for a file with the file dialog"
            }
            Capability::FsFolder => "Work in a hamlet's folder",
            Capability::ModelCall => "Call a model, on the budget",
            Capability::PluginUse => "Use another plugin",
        }
    }

    /// The capability of a manifest's `name`.
    pub fn parse(name: &str) -> Result<Self, Refusal> {
        if NEVER.contains(&name) {
            return Err(Refusal::Never(name.into()));
        }
        Self::ALL
            .into_iter()
            .find(|capability| capability.name() == name)
            .ok_or_else(|| Refusal::Unknown(name.into()))
    }
}

/// Why a manifest's ask is turned down.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// The capability is among [`NEVER`].
    Never(String),
    Unknown(String),
    /// The ask gives no reason.
    NoReason(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Never(name) => {
                write!(
                    f,
                    "it asks for `{name}`, which is never granted"
                )
            }
            Refusal::Unknown(name) => {
                write!(
                    f,
                    "it asks for `{name}`, which is not a capability"
                )
            }
            Refusal::NoReason(name) => {
                write!(f, "its ask for `{name}` gives no reason")
            }
        }
    }
}
