//! What the chief brings home from the Agent Bazaar: a skill taken
//! into the library, a hamlet founded from a template, and packages
//! bought at the bazaar's server settled; and what the chief packs
//! to sell there.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{Address, RequestId};

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

/// The most bytes of a package a `settle-part` or a `package` read
/// carries, before base64.
pub const PACKAGE_PART: usize = 512 * 1024;

/// The most bytes a package of the bazaar's server may be.
pub const PACKAGE_BYTES: u64 = 10 * 1024 * 1024;

/// Begins settling a package bought at the bazaar's server into the
/// home: an agent moved into a hamlet, a hamlet founded, or tools
/// added. Its bytes follow in `settle-part`s; the last part's answer
/// says what came of it.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Settle {
    pub id: RequestId,
    /// The hamlet an agent moves into; nothing else names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub into: Option<String>,
    /// The listing's id at the bazaar's server.
    #[schemars(length(min = 1, max = 128))]
    pub listing: String,
    /// The SHA-256 of the package's bytes, in lowercase hex.
    #[schemars(length(min = 64, max = 64))]
    pub sha256: String,
    /// The package's bytes.
    #[schemars(range(min = 1, max = 10_485_760))]
    pub size: u64,
    /// The listing's title.
    #[schemars(length(max = 200))]
    pub title: String,
    /// The version bought.
    #[schemars(length(min = 1, max = 64))]
    pub version: String,
}

/// A part of the package a `settle` began, in order.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SettlePart {
    /// At most [`PACKAGE_PART`] bytes, in base64.
    #[schemars(length(max = 699_052))]
    pub data: String,
    pub id: RequestId,
    /// Where the part begins in the package.
    #[schemars(range(max = 10_485_760))]
    pub offset: u64,
    /// The `upload` of the answer to the `settle`.
    pub upload: String,
}

/// What a package of the bazaar's server holds.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum PackageKind {
    #[serde(rename = "agent")]
    Agent,
    #[serde(rename = "hamlet")]
    Hamlet,
    /// Tools, which the bazaar's server calls a plugin.
    #[serde(rename = "tools")]
    Tools,
}

/// A listing bought and settled into the home.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Bought {
    /// The hamlet an agent moved into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub hamlet: Option<String>,
    pub kind: PackageKind,
    /// The listing's id at the bazaar's server.
    pub listing: String,
    /// What it became: the agent's name, the hamlet's or the tools'.
    pub names: Vec<String>,
    pub title: String,
    /// The version settled.
    pub version: String,
}

/// A tool added by hand that can be packed for sale.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct PackableTool {
    pub description: String,
    pub name: String,
}

/// What to pack for sale at the bazaar's server.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "what", deny_unknown_fields)]
pub enum Packing {
    /// An agent with its character and skills, its notes too with
    /// `notes`.
    #[serde(rename = "agent")]
    Agent { agent: Address, notes: bool },
    /// A hamlet whole, `kind` saying in a few words what kind of
    /// hamlet it is.
    #[serde(rename = "hamlet")]
    Hamlet {
        hamlet: String,
        #[schemars(length(min = 1, max = 60))]
        kind: String,
        notes: bool,
    },
    /// These tools added by hand, together.
    #[serde(rename = "tools")]
    Tools {
        #[schemars(length(min = 1, max = 16))]
        tools: Vec<String>,
    },
}

/// A part of a package packed for sale. The parts come in order,
/// the last ending at `size`.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct PackagePart {
    /// At most [`PACKAGE_PART`] bytes, in base64.
    pub data: String,
    pub offset: u64,
    /// The whole package's bytes.
    pub size: u64,
}
