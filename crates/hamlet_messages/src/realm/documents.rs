//! The chief's documents, and the copies of them an agent asks for.

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use super::{ChiefRequest, Done};
use crate::Message;
use crate::address::{Agent, Realm};
use crate::agent::copies::Copies;
use crate::sender::Client;

/// Sets the realm's charter; absent text gives back the built-in one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditCharter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl ChiefRequest for EditCharter {
    const NAME: &'static str = "chief_edit_charter";
    type Answer = Done<EditRefusal>;
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
pub enum EditRefusal {
    #[error("there is no such hamlet")]
    NoSuchHamlet,
    #[error("no model is named `{model}`")]
    NoSuchModel { model: String },
    #[error("a profile's documents are the profile's to edit")]
    AProfiles,
}

/// Reads the charter agents are given.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadCharter;

impl Action for ReadCharter {
    type Output = CharterRead;
    const NAME: &'static str = "read_charter";
}

impl Message for ReadCharter {
    type To = Realm;
    type From = Client;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharterRead {
    /// Whether a chief wrote it; else it is the built-in one.
    pub own: bool,
    pub text: String,
}

/// Asks for the parts of the copies newer than the versions the
/// agent holds, once after each wake. A part's version is absent
/// when the agent holds none of it.
#[derive(
    Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize,
)]
pub struct ReadCopies {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charter: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<u32>,
}

impl Action for ReadCopies {
    type Output = Copies;
    const NAME: &'static str = "read_copies";
}

impl Message for ReadCopies {
    type To = Realm;
    type From = Agent;
}
