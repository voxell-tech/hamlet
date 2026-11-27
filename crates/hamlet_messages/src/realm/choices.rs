//! The choices a lead or a chief makes for an agent: the nickname it
//! is shown by, and the model it runs.

use hamlet_protocol::Effort;
use serde::{Deserialize, Serialize};

use super::{
    AgentName, ChiefRequest, Done, DoneTo, HamletId, LeadRefusal,
    Left, Named, OnlyAHire, SocietyTool, ToolCall, ToolName,
};

/// Sets the name a hire is shown by; absent gives it back its own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetNickname {
    pub hire: Named,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Why, for the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for SetNickname {
    const NAME: &'static str = "chief_set_nickname";
    type Answer = Done<ChoiceRefusal>;
}

impl ToolCall for SetNickname {
    const TOOL: SocietyTool = SocietyTool::SetNickname;
    type Answer = DoneTo<LeadRefusal<ChoiceRefusal>>;
}

/// Sets the model of an agent, or of a hamlet's agents that choose
/// none. An absent part takes the next choice up: the hamlet's, then
/// the realm's (H29).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetModel {
    pub of: ModelOf,
    /// `provider::model`, as the catalogue names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast: Option<bool>,
    /// Why, for the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for SetModel {
    const NAME: &'static str = "chief_set_model";
    type Answer = Done<ChoiceRefusal>;
}

impl ToolCall for SetModel {
    const TOOL: SocietyTool = SocietyTool::SetModel;
    type Answer = DoneTo<LeadRefusal<ChoiceRefusal>>;
}

/// The hire or hamlet whose model a [`SetModel`] sets. Only a chief
/// names a hamlet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelOf {
    Hire { hire: Named },
    Hamlet { hamlet: HamletId },
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChoiceRefusal {
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("no hamlet has the id `{}`", id.0)]
    NoSuchHamlet { id: HamletId },
    #[error("`{hire}` has left: {left}")]
    Left { hire: AgentName, left: Left },
    #[error("A nickname is one line of at most 24 letters.")]
    BadNickname,
    #[error("`{name}` is not a hire")]
    NotAHire { name: String },
    #[error("no model is named `{model}`")]
    NoSuchModel { model: String },
    #[error(
        "a chief chose `{hire}`'s model; only a chief may change it"
    )]
    ChiefChose { hire: AgentName },
    #[error("only a chief sets a hamlet's model")]
    HamletFromAnAgent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetTools {
    pub hire: Named,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub off: Vec<ToolName>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<ToolName>,
    /// Why, for the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for SetTools {
    const NAME: &'static str = "chief_set_tools";
    type Answer = Done<OnlyAHire>;
}

impl ToolCall for SetTools {
    const TOOL: SocietyTool = SocietyTool::SetTools;
    type Answer = DoneTo<LeadRefusal<OnlyAHire>>;
}
