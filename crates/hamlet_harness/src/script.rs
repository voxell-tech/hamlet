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
    /// The chief allows a capability the plugin asked for.
    Grant(String),
    /// The chief refuses one.
    Deny(String),
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
