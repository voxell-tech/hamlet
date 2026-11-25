//! The saves the window waits for before it closes.

use bevy::ecs::event::Event;
use hamlet_client::outbox::RequestId;

/// A save on its way to the server, as the request that carries it,
/// which the window waits for before it closes.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct Saving(pub RequestId);
