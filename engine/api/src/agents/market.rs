//! What the Agent Bazaar asks of the agents service: a model an
//! endpoint lists added to the catalogue, a skill taken into the
//! library, a hamlet founded from a template with its agents, and
//! what is bought from and sold at the bazaar's server written into
//! the home and packed from it.

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

/// What a package of the bazaar's server holds.
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize,
)]
pub enum PackageKind {
    Agent,
    Hamlet,
    /// An agent's tools.
    Tools,
}

/// Where a package bought is written.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Destination {
    /// A hamlet founded in the realm.
    Realm,
    /// An agent moved into the hamlet of this name.
    Hamlet(String),
    /// Tools added to the house of `agent` of `hamlet`.
    House { hamlet: String, agent: String },
}

/// One version of a listing bought, to be written into the home.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Delivery {
    /// The listing's id at the bazaar's server.
    pub listing: String,
    pub title: String,
    pub version: String,
    /// One part of the package as the server sends it, its JSON
    /// untouched: at most [`PACKAGE_PART`] bytes, the parts sent in
    /// order.
    pub package: Vec<u8>,
    /// This part's place, from 0, and how many there are. The
    /// package is settled as its last part comes.
    pub part: u32,
    pub parts: u32,
    pub to: Destination,
}

/// The most bytes of a package one request or reply carries, well
/// within what one call of a plugin hands over.
pub const PACKAGE_PART: usize = 384 * 1024;

/// A listing bought and written into the home, as `bought.json`
/// records it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Bought {
    pub kind: PackageKind,
    pub title: String,
    pub version: String,
    /// The agents, hamlet or tools it became.
    pub names: Vec<String>,
    /// The hamlet it went into, for an agent or tools.
    pub hamlet: Option<String>,
    /// The agent whose house took the tools.
    pub house: Option<String>,
}

/// Tools in an agent's house that can be packed for sale.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct HousedTool {
    pub hamlet: String,
    pub agent: String,
    /// The name of its folder.
    pub name: String,
}

/// What to pack for sale.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Packing {
    /// The agent `name` of `hamlet` with its character and skills,
    /// its notes too when `notes`.
    Agent {
        hamlet: String,
        name: String,
        notes: bool,
    },
    /// The hamlet `name` whole, `kind` saying what kind of hamlet it
    /// is.
    Hamlet {
        name: String,
        kind: String,
        notes: bool,
    },
    /// These tools together.
    Tools(Vec<HousedTool>),
}
