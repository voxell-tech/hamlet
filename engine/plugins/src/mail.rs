//! What plugins ask of the engine's services, and what the services
//! send back, whatever host runs the plugin.

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::Message;
use hamlet_engine_api::{Failure, Reply, Request, Ticket};

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<Asked>().add_message::<Deliver>();
}

/// A request of the plugin `plugin`, made `depth` plugins deep in a
/// chain of calls and events: 1 for one made on its own.
#[derive(Message, Clone, Debug)]
pub struct Asked {
    pub plugin: Entity,
    pub ticket: Ticket,
    pub request: Request,
    pub depth: u32,
}

impl Asked {
    /// The reply to this request.
    pub fn reply(&self, reply: Result<Reply, Failure>) -> Deliver {
        Deliver {
            plugin: self.plugin,
            message: hamlet_engine_api::Message::Reply {
                ticket: self.ticket,
                reply,
            },
            depth: self.depth,
        }
    }
}

/// A message for the plugin `plugin`, at `depth` in the chain it
/// came from.
#[derive(Message, Clone, Debug)]
pub struct Deliver {
    pub plugin: Entity,
    pub message: hamlet_engine_api::Message,
    pub depth: u32,
}
