//! A script: the steps a plugin is put through, read from TOML.
//!
//! ```toml
//! [[step]]
//! tick = 2
//!
//! [[step]]
//! click = { panel = "main", node = "go" }
//! ```

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
    /// Runs this many frames.
    Tick(u32),
    /// Presses a button.
    Click(Target),
    /// Commits a text in a text field.
    Type(Typing),
    /// Flips a checkbox.
    Toggle(Target),
    /// Replaces the agents the plugin may read.
    SetAgents(Vec<Agent>),
    /// The chief allows a capability the plugin asked for.
    Grant(String),
    /// The chief refuses one.
    Deny(String),
    /// Draws a panel to a picture.
    Render(Picture),
    /// Fails unless a node of the panel shows the text.
    Expect(Expected),
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
