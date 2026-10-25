//! What the chief brings home from the Agent Bazaar: a skill taken
//! into the library, and a hamlet founded from a template.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::RequestId;

/// Takes a skill into the library, as the chief brought it from
/// `from`. Refused when the library has a skill of that name.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct TakeSkill {
    /// The skill's files, `SKILL.md` among them, by their paths in
    /// its folder.
    #[schemars(length(min = 1, max = 256))]
    pub files: Vec<SkillText>,
    /// Where the skill came from, such as its page.
    #[schemars(length(max = 512))]
    pub from: String,
    pub id: RequestId,
    /// The skill's folder in the library.
    #[schemars(length(min = 1, max = 64))]
    pub name: String,
}

/// A text file of a skill.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SkillText {
    /// Its path in the skill's folder, with `/` between folders.
    #[schemars(length(min = 1, max = 255))]
    pub path: String,
    pub text: String,
}

/// Founds a hamlet from a template, with its mayor and agents moved
/// in as the template has them. Nothing is founded when any of it is
/// refused.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct FoundFrom {
    pub id: RequestId,
    pub template: Template,
}

/// A hamlet as a template has it.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Template {
    /// Its agents besides the mayor.
    #[schemars(length(max = 64))]
    pub agents: Vec<Settler>,
    pub mayor: Settler,
    /// Its id, which commands name it by.
    pub name: String,
    /// What it is for, as its mayor is told.
    pub purpose: String,
    /// The name it is shown by.
    #[schemars(length(max = 32))]
    pub title: String,
}

/// An agent of a template, with what it knows.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Settler {
    /// Its `CHARACTER.md`.
    pub character: String,
    pub name: String,
    /// Its `NOTES.md`.
    pub notes: String,
    pub purpose: String,
    /// The skills kept in its house.
    #[schemars(length(max = 64))]
    pub skills: Vec<SettlerSkill>,
}

/// A skill an agent of a template keeps in its house.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SettlerSkill {
    /// What its `SKILL.md` says after its front matter.
    pub body: String,
    pub description: String,
    /// Its folder and the name in its front matter.
    pub name: String,
}
