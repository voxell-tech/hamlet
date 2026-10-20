//! What a plugin asks of the engine, and what the engine sends it.
//! A request is answered later, by a [`Message::Reply`] with the
//! ticket the request was given.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::agents::{AgentsReply, AgentsRequest};
use crate::asset::{Asset, AssetId, AssetInfo};
use crate::desktop::{DesktopReply, DesktopRequest};
use crate::id::{EventId, PluginId};
use crate::net::{FetchReply, FetchRequest};

/// The number a request is given, which its reply comes back with.
/// Never 0: a request given 0 was not taken.
pub type Ticket = u64;

/// What a plugin asks of the engine. Names are as the plugin gave
/// them, checked by the part of the engine that answers.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Request {
    /// A new asset of a type the plugin makes.
    CreateAsset {
        kind: String,
        name: String,
        bytes: Vec<u8>,
    },
    ReadAsset(AssetId),
    /// New bytes for one of the plugin's own assets.
    UpdateAsset {
        id: AssetId,
        bytes: Vec<u8>,
    },
    /// Takes one of the plugin's own assets away.
    DeleteAsset(AssetId),
    /// The assets of a type.
    ListAssets(String),
    /// Holds a reference to an asset, so the plugin is told when it
    /// changes.
    HoldAsset(AssetId),
    ReleaseAsset(AssetId),
    /// Calls the service `service` of another plugin with `argument`,
    /// bytes of the type both name.
    Call {
        service: String,
        argument: Vec<u8>,
    },
    /// Subscribes to an event of another plugin (`plugin.id/name`).
    Subscribe(String),
    /// Sends the plugin's own event `event` to its subscribers. Only
    /// a refusal is replied to.
    Emit {
        event: String,
        payload: Vec<u8>,
    },
    Fetch(FetchRequest),
    Agents(AgentsRequest),
    Desktop(DesktopRequest),
}

/// What the engine sends a plugin, given to it in a batch on the
/// frame after it came.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Message {
    /// What became of the request given `ticket`.
    Reply {
        ticket: Ticket,
        reply: Result<Reply, Failure>,
    },
    /// An asset the plugin holds is at a new version.
    Changed { asset: AssetId, version: u64 },
    /// An asset the plugin held is taken away.
    Gone(AssetId),
    /// An event the plugin subscribed to, with its payload.
    Event {
        from: PluginId,
        event: EventId,
        payload: Vec<u8>,
    },
}

/// What a request came to.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Reply {
    Created(AssetId),
    Read(Asset),
    /// The asset's version now.
    Updated(u64),
    Deleted,
    Listed(Vec<AssetInfo>),
    /// Held, at this version.
    Held(u64),
    Released,
    /// A service's result, bytes of the type both name.
    Answered(Vec<u8>),
    Subscribed,
    Fetched(FetchReply),
    Agents(AgentsReply),
    Desktop(DesktopReply),
}

/// Why a request came to nothing.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Failure {
    /// There is no such asset.
    NotFound,
    /// No running plugin serves the service called.
    NoImplementer,
    /// It may not: why.
    Refused(String),
    /// Two sides name a type differently: how.
    Mismatch(String),
    /// Past a budget: which.
    OutOfBudget(String),
    /// It was taken and could not be done: why.
    Failed(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::NoImplementer => {
                write!(f, "no running plugin serves it")
            }
            Self::Refused(why) => write!(f, "refused: {why}"),
            Self::Mismatch(why) => write!(f, "mismatch: {why}"),
            Self::OutOfBudget(why) => {
                write!(f, "out of budget: {why}")
            }
            Self::Failed(why) => write!(f, "failed: {why}"),
        }
    }
}

impl std::error::Error for Failure {}
