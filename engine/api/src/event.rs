//! Events: what a plugin sends to every plugin that subscribed to it.
//! The payload is bytes of a type both sides name, and an event goes
//! only to a plugin that names the same.

use serde::{Deserialize, Serialize};

use crate::asset::TypeName;
use crate::id::EventId;

/// An event the plugin sends.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventSent {
    /// Its name within the plugin.
    pub name: String,
    pub payload: TypeName,
}

/// An event of another plugin that the plugin may subscribe to.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventHeard {
    pub event: EventId,
    pub payload: TypeName,
}

/// The events a plugin sends and those it hears, as its manifest's
/// `[events]` names them.
#[derive(
    Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct Events {
    #[serde(default)]
    pub sends: Vec<EventSent>,
    #[serde(default)]
    pub hears: Vec<EventHeard>,
}
