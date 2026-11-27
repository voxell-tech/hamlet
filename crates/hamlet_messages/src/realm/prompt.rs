//! The sections of an agent's prompt that the realm writes.
use serde::{Deserialize, Serialize};

use crate::address::{Agent, Realm};
use crate::workspace::AgentNumber;

/// What a conversation of the agent is with.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind {
    /// A chief's chat.
    Chat,
    /// Letters with another agent.
    Letters,
    /// Questions with another agent.
    Questions,
    Other,
}

/// Asks the realm for the prompt's place, realm and profiles
/// sections, as they read in a conversation of `conversation`'s kind
/// with `peer`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadPlace {
    pub conversation: ConversationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer: Option<AgentNumber>,
}

/// The prompt's sections the realm writes; a section the agent is not
/// shown is absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceText {
    pub place: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles: Option<String>,
}

message!(ReadPlace, "read_place", Agent => Realm, PlaceText);
