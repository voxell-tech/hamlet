//! The window's state and requests over the client protocol.

pub mod chats;
pub mod connection;
pub mod gifts;
mod index;
pub mod messages;
pub mod outbox;
pub mod realm;
pub mod terminals;
pub mod waits;

use std::path::PathBuf;

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
pub use connection::{Connection, Server};
use hamlet_protocol::client as protocol;
use hamlet_protocol::client::discovery::ServerAddress;
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
            app.insert_resource(Server(server.clone()));
        }
        app.init_resource::<realm::Chief>()
            .init_resource::<realm::Confinement>()
            .init_resource::<realm::Settings>()
            .init_resource::<realm::Providers>()
            .init_resource::<realm::Grants>()
            .init_resource::<realm::Deeds>()
            .init_resource::<realm::Skills>()
            .init_resource::<realm::Closed>()
            .init_resource::<realm::Starters>()
            .add_plugins((
                index::plugin,
                messages::plugin,
                chats::plugin,
                connection::plugin,
                gifts::plugin,
                terminals::plugin,
            ));
    }
}

/// The async runtime shared with the window's connection.
#[derive(Resource, Clone)]
pub struct Tokio(pub Handle);

/// The folder for this window's layout, size and look.
#[derive(Resource, Clone, Debug)]
pub struct ViewFolder(pub PathBuf);

/// One frame of the server's state or a request's answer.
pub fn receive(world: &mut World, message: protocol::ServerMessage) {
    messages::receive(world, &message);
    match message {
        protocol::ServerMessage::State(state) => {
            realm::state(world, state);
        }
        protocol::ServerMessage::Change(changes) => {
            realm::changes(world, changes);
        }
        protocol::ServerMessage::Terminal(output) => {
            terminals::receive(world, output);
        }
        message @ (protocol::ServerMessage::Transcript(_)
        | protocol::ServerMessage::Turn(_)) => {
            chats::receive(world, message);
        }
        _ => {}
    }
}
