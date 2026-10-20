//! The vocabulary the engine and its plugins share. Plain data with
//! serde, no Bevy and no host, so both sides of a plugin build it.

pub mod asset;
pub mod capability;
pub mod contribution;
pub mod id;
pub mod layout;
pub mod manifest;
pub mod widget;
pub mod wire;

pub use asset::{AssetId, TypeName};
pub use capability::{Ask, Capability};
pub use contribution::Contribution;
pub use id::{CommandId, IdError, PanelId, PluginId};
pub use layout::{Axis, Layout};
pub use manifest::{Manifest, ManifestError};
pub use widget::{
    Action, TreeError, TreeLimits, Widget, WidgetEvent, WidgetId,
    WidgetNode, WidgetTree,
};
pub use wire::{WireError, decode, encode};

/// The version of this API. A plugin's manifest names the one it was
/// built against.
pub const API_VERSION: &str = "0.1.0";
