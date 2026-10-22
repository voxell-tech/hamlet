//! What the Agent Bazaar asks of the agents service: a model an
//! endpoint lists added to the catalogue, a skill taken into the
//! library, and a hamlet founded from a template with its agents.

use serde::{Deserialize, Serialize};

/// A model an endpoint lists, as the bazaar shows it, to be added to
/// the catalogue.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub struct ListedModel {
    /// Its id at the endpoint, such as `anthropic/claude-sonnet-5`.
    pub id: String,
    /// Tokens it holds, where the endpoint says.
    pub context: Option<u64>,
    /// Dollars per million tokens in and out, where the endpoint
    /// says.
    pub input: Option<f64>,
    pub output: Option<f64>,
    /// What it is good for, in a line.
    pub note: String,
}

/// A file of a skill, by its path within the skill's folder.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SkillFile {
    /// Such as `SKILL.md` or `scripts/check.sh`, never leaving the
    /// folder.
    pub path: String,
    pub text: String,
}

/// A hamlet as it is founded from a template: its mayor and its
/// agents moved in with their characters, notes and skills.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Template {
    /// Its name on disk and in commands.
    pub name: String,
    /// The name it is shown by.
    pub title: String,
    /// What it is for, as its mayor is told.
    pub purpose: String,
    pub mayor: Settler,
    /// Its agents besides the mayor.
    pub agents: Vec<Settler>,
}

/// An agent of a hamlet founded from a template, with all it knows.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Settler {
    pub name: String,
    pub purpose: String,
    /// Its `CHARACTER.md`.
    pub character: String,
    /// Its `NOTES.md`.
    pub notes: String,
    pub skills: Vec<SettlerSkill>,
}

/// A skill a settler keeps in its house.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SettlerSkill {
    /// The name of its folder and of its front matter.
    pub name: String,
    pub description: String,
    /// What follows the front matter.
    pub body: String,
}
