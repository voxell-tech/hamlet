//! What an agent waits on a chief for, and the chief's answers.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{AgentRef, Change, ChatRef, RequestId};

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Wait {
    pub about: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub agent: Option<AgentRef>,
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
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum WaitDetail {
    #[serde(rename = "approval")]
    Approval {
        #[serde(deserialize_with = "Option::deserialize")]
        always: Option<Lasting>,
        danger: WaitDanger,
        says: String,
    },
    #[serde(rename = "fault")]
    Fault {},
    #[serde(rename = "question")]
    Question { questions: Questions },
    #[serde(rename = "objection")]
    Objection {},
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum Lasting {
    #[serde(rename = "session")]
    Session,
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
