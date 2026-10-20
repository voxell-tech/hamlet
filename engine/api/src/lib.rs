//! The vocabulary the engine and its plugins share. Plain data with
//! serde, no Bevy and no host, so both sides of a plugin build it.

pub mod asset;
pub mod capability;
pub mod contribution;
pub mod event;
pub mod id;
pub mod layout;
pub mod mail;
pub mod manifest;
pub mod service;
pub mod widget;
pub mod wire;

pub use asset::{
    Asset, AssetId, AssetInfo, AssetType, AssetTypes, TypeName,
};
pub use capability::{Ask, Capability};
pub use contribution::Contribution;
pub use event::{EventHeard, EventSent, Events};
pub use id::{
    CommandId, EventId, IdError, PanelId, PluginId, ServiceId,
};
pub use layout::{Axis, Layout};
pub use mail::{Failure, Message, Reply, Request, Ticket};
pub use manifest::{Manifest, ManifestError};
pub use service::{ServiceExport, ServiceImport, Services};
pub use widget::{
    Action, TreeError, TreeLimits, Widget, WidgetEvent, WidgetId,
    WidgetNode, WidgetTree,
};
pub use wire::{WireError, decode, encode};

/// The version of this API. A plugin's manifest names the one it was
/// built against.
pub const API_VERSION: &str = "0.1.0";
