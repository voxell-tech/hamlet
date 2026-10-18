//! The names of the capabilities, and of the ones that no plugin can
//! be given.

use std::fmt;

/// The capabilities that can never be granted. A manifest that asks
/// for one is turned down at the scan, with no petition. `keys` is
/// credentials; keyboard shortcuts are [`Capability::UiKeys`].
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
    UiKeys,
    UiContext,
    UiInspector,
    UiSettings,
    Net,
    FsPick,
    FsFolder,
    ModelCall,
    PluginUse,
    CollectionsRead,
    CollectionsWrite,
    FilesRead,
    FilesWrite,
    Db,
}

impl Capability {
    pub const ALL: [Capability; 21] = [
        Capability::ReadAgents,
        Capability::ReadChat,
        Capability::ReadActivity,
        Capability::ReadPapers,
        Capability::ChatSend,
        Capability::CommandsRun,
        Capability::UiMenu,
        Capability::UiKeys,
        Capability::UiContext,
        Capability::UiInspector,
        Capability::UiSettings,
        Capability::Net,
        Capability::FsPick,
        Capability::FsFolder,
        Capability::ModelCall,
        Capability::PluginUse,
        Capability::CollectionsRead,
        Capability::CollectionsWrite,
        Capability::FilesRead,
        Capability::FilesWrite,
        Capability::Db,
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
            Capability::UiKeys => "ui.keys",
            Capability::UiContext => "ui.context",
            Capability::UiInspector => "ui.inspector",
            Capability::UiSettings => "ui.settings",
            Capability::Net => "net",
            Capability::FsPick => "fs.pick",
            Capability::FsFolder => "fs.folder",
            Capability::ModelCall => "model.call",
            Capability::PluginUse => "plugin.use",
            Capability::CollectionsRead => "collections.read",
            Capability::CollectionsWrite => "collections.write",
            Capability::FilesRead => "files.read",
            Capability::FilesWrite => "files.write",
            Capability::Db => "db",
        }
    }

    /// Whether the chief chooses for it once for each thing it names,
    /// such as each plugin, and not once for all.
    pub fn scoped(self) -> bool {
        matches!(
            self,
            Capability::PluginUse
                | Capability::CollectionsRead
                | Capability::CollectionsWrite
        )
    }

    /// Whether letting a plugin in leaves the choice at Ask next
    /// time, so that the chief is asked at its first use: the ones
    /// that are scoped, and the acts done as the chief.
    pub fn asked_at_use(self) -> bool {
        self.scoped()
            || matches!(
                self,
                Capability::ChatSend | Capability::CommandsRun
            )
    }

    /// Whether it reaches only what its ask's scope lists, so that an
    /// ask with no scope reaches nothing.
    pub fn needs_scope(self) -> bool {
        matches!(
            self,
            Capability::Net
                | Capability::ChatSend
                | Capability::CommandsRun
        )
    }

    /// What `scope` limits it to, as the chief reads it, if it is
    /// limited at all.
    pub fn limit(self, scope: &[String]) -> Option<String> {
        match (scope.is_empty(), self.needs_scope()) {
            (false, _) => Some(scope.join(", ")),
            (true, true) => {
                Some("nothing, the ask names no scope".into())
            }
            (true, false) => None,
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
            Capability::ChatSend => {
                "Send messages to agents as the chief"
            }
            Capability::CommandsRun => "Run commands as the chief",
            Capability::UiMenu => "Add entries to the menus",
            Capability::UiKeys => "Add keyboard shortcuts",
            Capability::UiContext => {
                "Add items to the menus of things in the world"
            }
            Capability::UiInspector => {
                "Add tabs to the Inspector's page of an agent"
            }
            Capability::UiSettings => "Add pages to the Settings",
            Capability::Net => "Fetch from the web hosts it lists",
            Capability::FsPick => {
                "Ask for a file with the file dialog"
            }
            Capability::FsFolder => "Work in a hamlet's folder",
            Capability::ModelCall => "Call a model, on the budget",
            Capability::PluginUse => "Use another plugin",
            Capability::CollectionsRead => {
                "Read a collection another plugin shares"
            }
            Capability::CollectionsWrite => {
                "Change a collection another plugin shares"
            }
            Capability::FilesRead => {
                "Read the files in its own folder"
            }
            Capability::FilesWrite => {
                "Write the files in its own folder"
            }
            Capability::Db => "Keep records in a database of its own",
        }
    }

    /// A line for the chief on what makes the capability weigh more
    /// than its name says, where it does.
    pub fn caution(self) -> Option<&'static str> {
        match self {
            Capability::ChatSend | Capability::CommandsRun => Some(
                "Acts as the chief. Each use is marked with the \
                 plugin's name and the number of uses is limited.",
            ),
            Capability::Net => Some(
                "Reaches only the hosts listed, over https, within \
                 a size and time limit.",
            ),
            _ => None,
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
