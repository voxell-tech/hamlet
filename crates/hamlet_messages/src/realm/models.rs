//! The agent's calls to the realm about its model: a provider's
//! credential, and a turn the model or the budget ended.

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use crate::address::{Agent, Realm};
use crate::workspace::{ConversationId, RealmTask};
use crate::{Message, worded};

/// Asks for a provider's credential, held in memory for one request
/// and never written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credential {
    pub provider: String,
}

impl Action for Credential {
    type Output = CredentialAnswer;
    const NAME: &'static str = "credential";
}

impl Message for Credential {
    type To = Realm;
    type From = Agent;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CredentialAnswer {
    Key {
        key: String,
    },
    Oauth {
        login: OauthLogin,
    },
    #[serde(serialize_with = "worded")]
    Refused {
        why: CredentialRefusal,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OauthLogin {
    pub access: String,
    pub refresh: String,
    /// Whole seconds since the epoch, so it crosses both ways:
    /// cbor-x writes a larger integer as a float.
    pub expires: u32,
    /// The provider's own fields, as pi-ai keeps them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<String>,
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
pub enum CredentialRefusal {
    #[error("no one has signed in to {provider}")]
    NotSignedIn { provider: String },
}

/// Keeps a credential a sign-in or a refresh made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeepCredential {
    pub provider: String,
    pub value: Kept,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Kept {
    Key { key: String },
    Oauth { login: OauthLogin },
}

impl Action for KeepCredential {
    type Output = ();
    const NAME: &'static str = "keep_credential";
}

impl Message for KeepCredential {
    type To = Realm;
    type From = Agent;
}

/// Tells the realm that a turn in a chief's chat ended with no
/// answer, so it opens a wait for the chief (C29). A turn
/// `may_spend` refused with a [`Hold`](super::oversight::Hold) sends
/// none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnFailed {
    pub task: RealmTask,
    pub conversation: ConversationId,
    pub words: String,
    pub cause: Cause,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    Model,
    Budget,
}

impl Action for TurnFailed {
    type Output = ();
    const NAME: &'static str = "turn_failed";
}

impl Message for TurnFailed {
    type To = Realm;
    type From = Agent;
}
