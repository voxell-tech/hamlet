//! What an agent waits on a chief for, and the chief's answers.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{AgentId, Change, ChatRef, ProfileId, RequestId};

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Wait {
    pub about: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub agent: Option<AgentId>,
    pub answers: Vec<Choice>,
    pub asked: u64,
    pub by: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub change: Option<Change>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub chat: Option<ChatRef>,
    pub detail: WaitDetail,
    pub id: String,
    pub since: u64,
    pub subject: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum WaitDanger {
    #[serde(rename = "network")]
    Network,
    #[serde(rename = "unconfined")]
    Unconfined,
    #[serde(rename = "destructive")]
    Destructive,
    #[serde(rename = "reserved")]
    Reserved,
    #[serde(rename = "edit")]
    Edit,
    #[serde(rename = "command")]
    Command,
    /// Files an agent sends that are over what it may send alone.
    #[serde(rename = "send")]
    Send,
    /// A call of an MCP tool that may change something outside the
    /// realm.
    #[serde(rename = "act")]
    Act,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "kind", deny_unknown_fields)]
#[expect(
    clippy::large_enum_variant,
    reason = "a wire type; a box would show in every pattern"
)]
pub enum WaitDetail {
    #[serde(rename = "approval")]
    Approval {
        #[serde(deserialize_with = "Option::deserialize")]
        always: Option<Lasting>,
        danger: WaitDanger,
        says: String,
    },
    #[serde(rename = "fault")]
    Fault {
        /// The cause, when a chief sets it up once for every agent.
        #[serde(deserialize_with = "Option::deserialize")]
        setup: Option<Setup>,
    },
    #[serde(rename = "question")]
    Question { questions: Questions },
    #[serde(rename = "objection")]
    Objection {},
    /// A profile an agent made, waiting at the harbor to be let into
    /// the city.
    #[serde(rename = "newcomer")]
    Newcomer { profile: ProfileId },
    /// An MCP server an agent asks a chief to add, as its entry would
    /// be; `allow` adds it.
    #[serde(rename = "mcp-server")]
    McpServer {
        name: String,
        entry: crate::client::McpEntry,
    },
}

/// A fault's cause a chief sets up once for every agent.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    pub cause: SetupCause,
    /// The model the agent runs, as `provider::id`; empty when none
    /// is chosen.
    pub model: String,
}

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
pub enum SetupCause {
    /// No model is set up, or the one chosen is not available.
    #[serde(rename = "model")]
    Model,
    /// The model's provider refused the credentials, or has none.
    #[serde(rename = "credentials")]
    Credentials,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum Lasting {
    /// The rest of the agent's conversation.
    #[serde(rename = "conversation")]
    Conversation,
    /// A grant, until a chief revokes it.
    #[serde(rename = "grant")]
    Grant,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub choice: Choice,
    pub id: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "Vec<Pick>", length(min = 1))]
    pub picks: Option<Vec<Pick>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub text: Option<String>,
    pub wait: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum Choice {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "always")]
    Always,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "go")]
    Go,
    #[serde(rename = "withdraw")]
    Withdraw,
    #[serde(rename = "reply")]
    Reply,
    #[serde(rename = "dismiss")]
    Dismiss,
    /// Lets a newcomer into the city.
    #[serde(rename = "welcome")]
    Welcome,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Pick {
    #[serde(rename = "offer")]
    Offer { index: u64 },
    #[serde(rename = "words")]
    Words { text: String },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub description: String,
    pub title: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub description: String,
    pub options: Vec<Offer>,
    pub question: String,
    pub text_input: bool,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Questions {
    pub message: String,
    #[schemars(length(min = 1))]
    pub questions: Vec<Question>,
}
