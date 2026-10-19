//! A script: the steps a plugin is put through, read from TOML.
//!
//! ```toml
//! [[step]]
//! tick = 2
//!
//! [[step]]
//! click = { panel = "main", node = "go" }
//! ```
//!
//! A script for several plugins puts one in front with `plugin =
//! "id"` before the steps that are for it.

use serde::Deserialize;

/// The steps, in order.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Script {
    #[serde(default, rename = "step")]
    pub steps: Vec<Step>,
}

/// One thing done to the plugin, or asked of it.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum Step {
    /// Puts the plugin of this id in front, where there are several:
    /// the steps after it are done to that one.
    Plugin(String),
    /// Runs this many frames.
    Tick(u32),
    /// Presses a button.
    Click(Target),
    /// Commits a text in a text field.
    Type(Typing),
    /// Flips a checkbox.
    Toggle(Target),
    /// Chooses the menu line of this label.
    RunMenu(String),
    /// Presses this chord, such as `Cmd+Shift+N`.
    Press(String),
    /// Replaces the agents the plugin may read.
    SetAgents(Vec<Agent>),
    /// Replaces the papers of the selected agent, which a plugin that
    /// may read them sees.
    SetPapers(Papering),
    /// Selects the agent of this name in the world, or none for an
    /// empty name.
    SetSelected(String),
    /// Replaces the realm a plugin that may read it sees.
    SetWorld(Surveying),
    /// Closes the file dialog a plugin opened with the file at this
    /// path chosen, or with nothing for an empty path.
    ChooseFile(String),
    /// Closes the folder dialog a plugin opened with the folder at
    /// this path chosen, or with nothing for an empty path.
    ChooseFolder(String),
    /// Makes these the replies of the model, in order, to the model
    /// calls of the plugins. No server is reached; until this step
    /// there is no model.
    ModelReplies(Vec<String>),
    /// The chief answers the question in Needs You whose words hold
    /// the text, such as a write a plugin asked to make.
    Answer(Answering),
    /// The chief allows a capability the plugin asked for.
    Grant(String),
    /// The chief refuses one.
    Deny(String),
    /// Quits the app and starts it again on the same home, as
    /// `restart = true`. What the plugins kept on disk is there; what
    /// they held in memory is not.
    Restart(bool),
    /// Draws a panel to a picture.
    Render(Picture),
    /// Chooses an item of the menu of a thing of the world.
    Context(Choosing),
    /// Draws the Inspector's page of the selected agent, turned to a
    /// tab the plugin added.
    Section(Hosted),
    /// Draws a page the plugin added to the Settings.
    Page(Hosted),
    /// Fails unless a node of the panel shows the text.
    Expect(Expected),
    /// Fails unless a notice the chief was given in the chat holds
    /// the text, as an act done as the chief is noted.
    ExpectTold(String),
    /// Fails unless a mark the plugin draws over the world, as the
    /// transcript writes it, holds the text.
    ExpectOverlay(String),
    /// Fails unless a line of what the plugin builds in the world, as
    /// the transcript writes it (its site, a node, a mesh or a
    /// shader), holds the text.
    ExpectScene(String),
    /// Has an agent call a tool the plugin gave, as the app carries
    /// a call of an agent's, and checks what comes back.
    CallTool(ToolCall),
    /// Tells the plugins that an agent's turn began or ended, as the
    /// app does for each turn, to those that hold `agent.hook` and
    /// import `host/before-turn` or `host/after-turn`.
    Turn(Turned),
    /// Puts a point of an agent's turn to the plugins that hold
    /// `agent.steer`, as the app does before a model call or a tool
    /// call, and checks what the plugin in front made of it.
    Steer(Steering),
}

/// A point of an agent's turn, and what the plugin in front is to
/// make of it.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Steering {
    #[serde(default = "an_agent")]
    pub agent: String,
    #[serde(default = "a_hamlet")]
    pub hamlet: String,
    /// The tool about to be called, or none for a point before a
    /// model call.
    #[serde(default)]
    pub tool: String,
    /// Its arguments as a model writes them, in JSON.
    #[serde(default = "no_arguments")]
    pub args: String,
    /// `note`, `hold`, `failed` or `none`.
    pub expect: String,
    /// Fails unless the note, the reason or why it failed holds
    /// this.
    #[serde(default)]
    pub shows: String,
}

/// A question of a plugin's put to the chief, by its words, and the
/// chief's answer to it.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Answering {
    pub holds: String,
    /// True allows it, and false refuses it.
    pub allow: bool,
}

fn an_agent() -> String {
    "Juniper".into()
}

impl Step {
    /// Whether the step draws a picture and does nothing else.
    pub fn is_picture(&self) -> bool {
        matches!(
            self,
            Step::Render(_) | Step::Section(_) | Step::Page(_)
        )
    }
}

/// A turn of an agent, as the plugins are told of it.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Turned {
    /// `before` or `after`.
    pub phase: String,
    pub agent: String,
    #[serde(default = "a_hamlet")]
    pub hamlet: String,
    #[serde(default = "first")]
    pub number: u64,
    /// The names of the tools the turn used, for `after`.
    #[serde(default)]
    pub tools: Vec<String>,
}

fn first() -> u64 {
    1
}

/// A call of a tool of the plugin in front, by an agent.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    /// The plugin's own name for the tool.
    pub tool: String,
    /// The arguments as a model writes them, in JSON.
    #[serde(default = "no_arguments")]
    pub args: String,
    /// The hamlet of the agent. A tool is offered to the agents of
    /// the hamlets the ask lists.
    #[serde(default = "a_hamlet")]
    pub hamlet: String,
    /// Fails unless the text the tool returned holds this.
    #[serde(default)]
    pub shows: String,
    /// The call is to fail, and `shows` is in why.
    #[serde(default)]
    pub fails: bool,
    /// False checks that the tool is not offered to the agent, and
    /// calls nothing.
    #[serde(default = "offered")]
    pub offered: bool,
}

fn no_arguments() -> String {
    "{}".into()
}

fn a_hamlet() -> String {
    "Home".into()
}

fn offered() -> bool {
    true
}

/// An item of the menu of a thing of the world.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Choosing {
    /// The thing, as `kind:name`, such as `agent:Juniper`.
    pub subject: String,
    /// The label of the item, or what it says of the thing.
    pub item: String,
    /// A picture of the menu, drawn before the item is chosen.
    pub file: Option<String>,
}

/// A page of the Inspector or of the Settings, by its title.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Hosted {
    pub title: String,
    /// The picture's file, under the output folder.
    pub file: String,
}

/// A node of a panel, by the ids the plugin gave them.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub panel: String,
    pub node: String,
}

#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Typing {
    pub panel: String,
    pub node: String,
    pub text: String,
}

/// An agent and its hamlet.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub name: String,
    pub hamlet: String,
}

/// The realm: its hamlets, its agents and where each stands, the
/// thing selected as `kind:name` (none when empty) and the hour.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Surveying {
    #[serde(default)]
    pub hamlets: Vec<Hamlet>,
    #[serde(default)]
    pub agents: Vec<Placed>,
    #[serde(default)]
    pub selected: String,
    #[serde(default)]
    pub hour: f32,
}

/// A hamlet by its id, its name (the id where it has none) and the
/// place of its common, as `[x, y, z]`.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Hamlet {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub at: Option<[f32; 3]>,
}

/// An agent, the id of its hamlet, the place of its robot and
/// whether it is at work.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Placed {
    pub name: String,
    pub hamlet: String,
    pub at: Option<[f32; 3]>,
    #[serde(default)]
    pub busy: bool,
}

/// The notes, the character and the skills of an agent.
#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Papering {
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub character: String,
    /// The names of the skills.
    #[serde(default)]
    pub skills: Vec<String>,
}

#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Picture {
    pub panel: String,
    /// The picture's file, under the output folder.
    pub file: String,
}

#[derive(Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    pub panel: String,
    pub text: String,
}

impl Script {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.message().to_string())
    }
}
