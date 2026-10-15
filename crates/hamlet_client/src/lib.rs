//! The window's state and requests over protocol v0.

pub mod chats;
pub mod connection;
pub mod messages;
pub mod outbox;
pub mod realm;
pub mod waits;

use std::path::PathBuf;

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
pub use connection::{Connection, ServerAddress};
use hamlet_protocol::v0;
use tokio::runtime::Handle;

/// The window's state, request routing and connection plugins.
pub struct ClientPlugin {
    pub server: Option<ServerAddress>,
}

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        realm::register(app.world_mut());
        app.world_mut().register_component::<waits::Waiting>();
        app.world_mut().register_component::<waits::RaisedBy>();
        app.world_mut().register_component::<waits::Needs>();
        if let Some(server) = &self.server {
            app.insert_resource(server.clone());
        }
        app.init_resource::<realm::Chief>()
            .init_resource::<realm::Sandbox>()
            .init_resource::<realm::Settings>()
            .init_resource::<realm::Providers>()
            .init_resource::<realm::Grants>()
            .init_resource::<realm::Deeds>()
            .init_resource::<realm::Skills>()
            .init_resource::<realm::Closed>()
            .init_resource::<realm::Starters>()
            .add_plugins((
                messages::plugin,
                chats::plugin,
                connection::plugin,
            ));
    }
}

/// The async runtime shared with the window's connection.
#[derive(Resource, Clone)]
pub struct Tokio(pub Handle);

/// The folder for this window's layout, size and look.
#[derive(Resource, Clone, Debug)]
pub struct ViewDir(pub PathBuf);

/// One frame of the server's state or a request's answer.
pub fn receive(world: &mut World, message: v0::ServerMessage) {
    messages::receive(world, &message);
    match message {
        v0::ServerMessage::State(state) => realm::state(world, state),
        v0::ServerMessage::Change(changes) => {
            realm::changes(world, changes)
        }
        message @ (v0::ServerMessage::Transcript(_)
        | v0::ServerMessage::Turn(_)) => {
            chats::receive(world, message)
        }
        _ => {}
    }
}
