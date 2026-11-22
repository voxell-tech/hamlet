//! What a part says to the chief in a toast over the world: something
//! that happened, or how a request it sent was answered.

use bevy::ecs::event::Event;
use hamlet_client::outbox::RequestId;
use hamlet_ui::theme::StatusKind;

/// A toast that says something happened, which nothing answers.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct Toast {
    pub kind: StatusKind,
    pub head: String,
    pub line: String,
}

/// Watches a request, so that its rejection is toasted, and its
/// response too under `head` when there is one.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct WatchRequest {
    pub request: RequestId,
    pub head: Option<String>,
}
