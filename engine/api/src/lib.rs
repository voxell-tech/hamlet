//! The vocabulary the engine and its plugins share. Plain data with
//! serde, no Bevy and no host, so both sides of a plugin build it.

pub mod agents;
pub mod asset;
pub mod audio;
pub mod capability;
pub mod contribution;
pub mod desktop;
pub mod event;
pub mod id;
pub mod layout;
pub mod limits;
pub mod mail;
pub mod manifest;
pub mod net;
pub mod service;
pub mod wire;

pub use crate::agents::{AgentsEvent, AgentsReply, AgentsRequest};
pub use crate::asset::{
    Asset, AssetId, AssetInfo, AssetType, AssetTypes, TypeName,
};
pub use crate::capability::{Ask, Capability};
pub use crate::contribution::{Contribution, PanelPadding};
pub use crate::desktop::{
    DesktopEvent, DesktopReply, DesktopRequest,
};
pub use crate::event::{EventHeard, EventSent, Events};
pub use crate::id::{
    CommandId, EventId, IdError, PanelId, PluginId, ServiceId,
};
pub use crate::layout::{Axis, Layout};
pub use crate::limits::{
    Limit, LimitKind, LimitUnit, Limits, PluginLimits,
};
pub use crate::mail::{Failure, Message, Reply, Request, Ticket};
pub use crate::manifest::{Dependency, Manifest, ManifestError};
pub use crate::net::{FetchReply, FetchRequest, Method};
pub use crate::service::{ServiceExport, ServiceImport, Services};
pub use crate::wire::{WireError, decode, encode};

/// The version of this API. A plugin's manifest names the one it was
/// built against.
pub const API_VERSION: &str = "0.1.0";
