//! The chief's documents: the charter, the realm's settings and the
//! catalogue of models, and the copies of them an agent asks for.

use hamlet_actor::Action;
use hamlet_protocol::Effort;
use serde::{Deserialize, Serialize};

use super::{ChiefRequest, Done};
use crate::Message;
use crate::address::{Agent, Realm};
use crate::agent::copies::{Copies, CopyVersion, Price};
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

/// Changes the realm's settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditSettings {
    /// Set in order, so the last of one kind holds.
    pub set: Vec<Setting>,
}

impl ChiefRequest for EditSettings {
    const NAME: &'static str = "chief_edit_settings";
    type Answer = Done<SettingRefusal>;
}

/// One of the realm's settings, as a chief sets it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Setting {
    /// The model an agent runs when nothing closer chooses one,
    /// `provider::model` as the catalogue names it.
    Model {
        model: String,
    },
    /// Absent sets none.
    Effort {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<Effort>,
    },
    /// The answered turns after which an agent reflects; never for 0
    /// (C5).
    ReflectEvery {
        turns: u32,
    },
    AllowNet {
        allow: bool,
    },
    Newcomers {
        newcomers: Newcomers,
    },
    /// The character a blank agent starts from.
    Character {
        text: String,
    },
    /// Every model server the chief added; one left out is removed.
    Endpoints {
        endpoints: Vec<Endpoint>,
    },
    /// A workspace on another machine; absent takes it away.
    Workspace {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace: Option<RemoteWorkspace>,
    },
}

/// Who lets a newcomer into the city.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Newcomers {
    /// Nobody: it moves in at once.
    Open,
    /// A chief: it waits at the harbor until a chief welcomes it.
    Chief,
    /// The Hand, for the chiefs, who can undo its verdict.
    Hand,
}

/// A model server the chief added, by the name its models are listed
/// under.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub name: String,
    pub base_url: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteWorkspace {
    pub url: String,
    pub token: String,
}

/// Leaves out the token.
impl std::fmt::Debug for RemoteWorkspace {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.debug_struct("RemoteWorkspace")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
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
pub enum SettingRefusal {
    #[error("a blank scholar needs a character to start from")]
    BlankCharacter,
    #[error("a character may have at most 4000 characters")]
    LongCharacter,
    #[error("an endpoint has a name without `::`")]
    EndpointName,
    #[error("two endpoints are named `{name}`")]
    EndpointTwice { name: String },
    #[error("`{address}` is not an address")]
    NotAnAddress { address: String },
    #[error("`{address}` is not an http or https address")]
    NotHttp { address: String },
    #[error(
        "an address may not hold a name or a key; sign in to the \
         endpoint with its key instead"
    )]
    AddressHoldsKey,
}

/// Changes what the chief says of one model of the catalogue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditCatalogue {
    /// `provider::model`, as the catalogue names it.
    pub model: String,
    /// Set in order, so the last of one kind holds.
    pub set: Vec<CardSetting>,
}

impl ChiefRequest for EditCatalogue {
    const NAME: &'static str = "chief_edit_catalogue";
    type Answer = Done<CatalogueRefusal>;
}

/// A part of a model's card, as a chief sets it. An absent value
/// gives back what the model's listing says.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CardSetting {
    Images {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<ImageLimits>,
    },
    Note {
        note: String,
    },
    Price {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        price: Option<Price>,
    },
}

/// What a model takes of images; a limit left out takes its default.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct ImageLimits {
    /// The most bytes of one image; larger ones are made smaller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<u32>,
    /// The most images one line carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_line: Option<u32>,
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
pub enum CatalogueRefusal {
    #[error("no model is named `{model}`")]
    NoSuchModel { model: String },
    #[error("a cost is a number of zero or more")]
    NegativeCost,
    #[error("an image limit is a number above zero")]
    ZeroImageLimit,
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
    pub charter: Option<CopyVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<CopyVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<CopyVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<CopyVersion>,
}

impl Action for ReadCopies {
    type Output = Copies;
    const NAME: &'static str = "read_copies";
}

impl Message for ReadCopies {
    type To = Realm;
    type From = Agent;
}
