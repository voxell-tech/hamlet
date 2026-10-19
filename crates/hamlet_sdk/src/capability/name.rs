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

/// The capabilities only a native plugin has, as it is given the
/// whole app. A manifest that asks for one is turned down at the
/// scan, with no petition.
pub const NATIVE_ONLY: [&str; 1] = ["render.compute"];

/// A thing a plugin can be allowed to do. A manifest names the most
/// it may ask for; a host function checks the matching verdict on
/// each call.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Capability {
    ReadAgents,
    ReadChat,
    ReadActivity,
    ReadPapers,
    ReadWorld,
    ChatSend,
    CommandsRun,
    WorldAct,
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
    AgentTool,
    AgentHook,
    AgentSteer,
    UiCanvas,
    WorldOverlay,
    RenderScene,
    RenderShader,
}

impl Capability {
    pub const ALL: [Capability; 30] = [
        Capability::ReadAgents,
        Capability::ReadChat,
        Capability::ReadActivity,
        Capability::ReadPapers,
        Capability::ReadWorld,
        Capability::ChatSend,
        Capability::CommandsRun,
        Capability::WorldAct,
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
        Capability::AgentTool,
        Capability::AgentHook,
        Capability::AgentSteer,
        Capability::UiCanvas,
        Capability::WorldOverlay,
        Capability::RenderScene,
        Capability::RenderShader,
    ];

    /// What a plugin that holds a capability that [faces the
    /// agents](Capability::agent_facing) may not also ask for, since
    /// together they would hand the chief's authority to whatever an
    /// agent does: a tool an agent calls, or a turn it takes, with a
    /// way to act as the chief or to work through another plugin that
    /// does.
    pub const AGENT_FACING: [Capability; 4] = [
        Capability::ChatSend,
        Capability::CommandsRun,
        Capability::WorldAct,
        Capability::PluginUse,
    ];

    /// Whether what the plugin does is set off by an agent.
    pub fn agent_facing(self) -> bool {
        matches!(
            self,
            Capability::AgentTool
                | Capability::AgentHook
                | Capability::AgentSteer
        )
    }

    /// The name a manifest and the grants file know it by.
    pub fn name(self) -> &'static str {
        match self {
            Capability::ReadAgents => "read.agents",
            Capability::ReadChat => "read.chat",
            Capability::ReadActivity => "read.activity",
            Capability::ReadPapers => "read.papers",
            Capability::ReadWorld => "read.world",
            Capability::ChatSend => "chat.send",
            Capability::CommandsRun => "commands.run",
            Capability::WorldAct => "world.act",
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
            Capability::AgentTool => "agent.tool",
            Capability::AgentHook => "agent.hook",
            Capability::AgentSteer => "agent.steer",
            Capability::UiCanvas => "ui.canvas",
            Capability::WorldOverlay => "world.overlay",
            Capability::RenderScene => "render.scene",
            Capability::RenderShader => "render.shader",
        }
    }

    /// Whether the chief chooses for it once for each thing it names,
    /// such as each plugin or folder, and not once for all.
    pub fn scoped(self) -> bool {
        matches!(
            self,
            Capability::PluginUse
                | Capability::CollectionsRead
                | Capability::CollectionsWrite
                | Capability::FsFolder
        )
    }

    /// Whether letting a plugin in leaves the choice at Ask next
    /// time, so that the chief is asked at its first use: the ones
    /// that are scoped, and the acts done as the chief.
    pub fn asked_at_use(self) -> bool {
        self.scoped()
            || matches!(
                self,
                Capability::ChatSend
                    | Capability::CommandsRun
                    | Capability::WorldAct
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
                | Capability::WorldAct
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
            (true, false) if self.agent_facing() => {
                Some("every hamlet, the ask names none".into())
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
            Capability::ReadWorld => {
                "See the hamlets, where the agents stand, what is \
                 selected and the hour"
            }
            Capability::ChatSend => {
                "Send messages to agents as the chief"
            }
            Capability::CommandsRun => "Run commands as the chief",
            Capability::WorldAct => {
                "Focus the camera on, follow or select a thing in \
                 the world, as the chief"
            }
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
            Capability::FsFolder => {
                "Read and write the files of a folder the chief \
                 chooses"
            }
            Capability::ModelCall => {
                "Ask the model the agents run, on the first hamlet's \
                 budget"
            }
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
            Capability::AgentTool => {
                "Give the agents tools that it carries out"
            }
            Capability::AgentHook => {
                "Be told when an agent's turn begins and ends"
            }
            Capability::AgentSteer => {
                "Add a note to an agent's turn, or hold back a tool \
                 call it is about to make"
            }
            Capability::UiCanvas => {
                "Draw 2D shapes in a canvas of its panels"
            }
            Capability::WorldOverlay => {
                "Draw labels and marks over the world"
            }
            Capability::RenderScene => {
                "Build small 3D scenes in the world beside a hamlet"
            }
            Capability::RenderShader => {
                "Colour its 3D scenes with shaders of its own"
            }
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
            Capability::WorldAct => Some(
                "Acts as the chief's clicks on the agents and \
                 hamlets of the hamlets listed, and no more. Each \
                 use is marked with the plugin's name and the \
                 number of uses is limited.",
            ),
            Capability::Net => Some(
                "Reaches only the hosts listed, over https, within \
                 a size and time limit.",
            ),
            Capability::AgentTool => Some(
                "The agents of the hamlets listed (every hamlet \
                 where none is) are offered its tools, each marked \
                 with the plugin's name, and what a tool returns is \
                 shown to them as text from a plugin. It cannot \
                 also act as the chief or use other plugins.",
            ),
            Capability::UiCanvas => Some(
                "Draws rectangles, lines, circles and text inside \
                 its own panels, within a size and a count. It \
                 cannot draw over the world or anywhere else.",
            ),
            Capability::FsPick => Some(
                "The chief picks each file in the system's dialog. \
                 The plugin gets a copy in its own folder, within \
                 its caps, and never a path outside it.",
            ),
            Capability::ModelCall => Some(
                "Spends money: each call is charged to the first \
                 hamlet's budget under the plugin's name and \
                 refused once the budget is spent, within a count a \
                 minute and a size. It never sees a key or an \
                 address.",
            ),
            Capability::FsFolder => Some(
                "The chief chooses the folder in the system's \
                 dialog, and the plugin reaches nothing outside it \
                 and never Hamlet's home. Reading and writing are \
                 chosen apart. Each write waits for the chief \
                 unless the folder is in the folder of a hamlet set \
                 to auto.",
            ),
            Capability::WorldOverlay => Some(
                "Hamlet draws its rectangles, lines, circles and \
                 text over the world, at agents, hamlets or places, \
                 within a count and a size, and names the plugin in \
                 a corner. It draws nothing itself: no shader, \
                 texture or model.",
            ),
            Capability::RenderScene => Some(
                "Hamlet builds its scene from Hamlet's own shapes, \
                 materials and lights, within a count and a small \
                 space about the place it names, and names the \
                 plugin over it. Nothing the plugin sends reaches \
                 the graphics card as code.",
            ),
            Capability::RenderShader => Some(
                "Hamlet checks each shader before it is used: one \
                 function that colours a point of a shape, within a \
                 size, with no resources of its own and only loops \
                 of a fixed count. It still runs on the graphics \
                 card for every point it colours.",
            ),
            Capability::AgentHook => Some(
                "Told of each turn of the agents of the hamlets \
                 listed (every hamlet where none is): the agent, \
                 the turn's number and the names of the tools it \
                 used, never what was said. It only watches, it \
                 cannot change or stop a turn, and it cannot also \
                 act as the chief or use other plugins.",
            ),
            Capability::AgentSteer => Some(
                "Asked before each model call and each tool call of \
                 the agents of the hamlets listed (every hamlet \
                 where none is), and sees the agent, the tool and \
                 its arguments. It may add a short note marked with \
                 its name, or hold a tool call back with a reason \
                 the agent is told. It only makes a turn stricter: \
                 it cannot approve anything, change what was said \
                 or what a tool returned, act as the chief or use \
                 other plugins. A hook that fails or runs out of \
                 time changes nothing.",
            ),
            _ => None,
        }
    }

    /// The capability of a manifest's `name`.
    pub fn parse(name: &str) -> Result<Self, Refusal> {
        if NEVER.contains(&name) {
            return Err(Refusal::Never(name.into()));
        }
        if NATIVE_ONLY.contains(&name) {
            return Err(Refusal::NativeOnly(name.into()));
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
    /// The capability is among [`NATIVE_ONLY`].
    NativeOnly(String),
    Unknown(String),
    /// The ask gives no reason.
    NoReason(String),
    /// Two asks that no plugin may make together: the first faces the
    /// agents, and the second would give what it does the chief's
    /// authority.
    Together(String, String),
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
            Refusal::NativeOnly(name) => write!(
                f,
                "it asks for `{name}`: render passes and compute \
                 are native-only, for a native plugin, which has \
                 the whole app"
            ),
            Refusal::Unknown(name) => {
                write!(
                    f,
                    "it asks for `{name}`, which is not a capability"
                )
            }
            Refusal::NoReason(name) => {
                write!(f, "its ask for `{name}` gives no reason")
            }
            Refusal::Together(facing, with) => write!(
                f,
                "it asks for `{facing}` and `{with}`, which no \
                 plugin may have together"
            ),
        }
    }
}
