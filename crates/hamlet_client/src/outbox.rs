//! Requests waiting for the connection to send them.

use bevy_ecs::prelude::*;
use hamlet_protocol::v0;
use uuid::Uuid;
pub use v0::RequestId;

/// Requests queued for the connection to send.
#[derive(Resource, Default)]
pub struct Outbox(Vec<v0::ClientMessage>);

impl Outbox {
    /// A queued request with a fresh UUID, returned to its caller.
    pub fn send(
        &mut self,
        message: impl FnOnce(RequestId) -> v0::ClientMessage,
    ) -> RequestId {
        let id = Uuid::new_v4().to_string();
        self.0.push(message(id.clone()));
        id
    }

    /// The queued requests in sending order.
    pub fn iter(&self) -> impl Iterator<Item = &v0::ClientMessage> {
        self.0.iter()
    }

    /// The queued requests removed in sending order.
    pub fn drain(
        &mut self,
    ) -> impl Iterator<Item = v0::ClientMessage> + '_ {
        self.0.drain(..)
    }
}
