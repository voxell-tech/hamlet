//! Requests of the engine, each giving a [`Ticket`], and what comes
//! back as Bevy messages carrying it.

use bevy_app::App;
use bevy_ecs::prelude::*;
use hamlet_engine_api::{
    AgentsEvent, AgentsReply, AgentsRequest, AssetId, DesktopEvent,
    DesktopReply, DesktopRequest, EventId, Failure, FetchRequest,
    Message as Mail, PluginId, Reply, Ticket, encode,
};

use crate::bindings::hamlet::engine::host as wit;

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<Replied>()
        .add_message::<AgentsEventReceived>()
        .add_message::<DesktopEventReceived>()
        .add_message::<AssetChanged>()
        .add_message::<AssetGone>()
        .add_message::<EventReceived>();
}

/// What became of the request given `ticket`, but for the events of
/// a watch.
#[derive(Message, Clone, Debug)]
pub struct Replied {
    pub ticket: Ticket,
    pub reply: Result<Reply, Failure>,
}

/// A change the agents service tells of, to the watch of `ticket`.
#[derive(Message, Clone, Debug)]
pub struct AgentsEventReceived {
    pub ticket: Ticket,
    pub event: AgentsEvent,
}

/// A change the desktop service tells of, to the watch of `ticket`.
#[derive(Message, Clone, Debug)]
pub struct DesktopEventReceived {
    pub ticket: Ticket,
    pub event: DesktopEvent,
}

/// An asset the plugin holds is at a new version.
#[derive(Message, Clone, Copy, Debug)]
pub struct AssetChanged {
    pub asset: AssetId,
    pub version: u64,
}

/// An asset the plugin held is taken away.
#[derive(Message, Clone, Copy, Debug)]
pub struct AssetGone {
    pub asset: AssetId,
}

/// An event of another plugin the plugin subscribed to.
#[derive(Message, Clone, Debug)]
pub struct EventReceived {
    pub from: PluginId,
    pub event: EventId,
    pub payload: Vec<u8>,
}

/// Requests of the engine. Each gives a ticket, and what it came to
/// is a [`Replied`] with it on a later frame. A ticket of 0 is a
/// request not taken.
pub trait HostExt {
    /// Makes an asset of the type `kind`, one the plugin makes:
    /// [`Reply::Created`].
    fn create_asset(
        &mut self,
        kind: &str,
        name: &str,
        bytes: &[u8],
    ) -> Ticket {
        wit::create_asset(kind, name, bytes)
    }

    /// [`Reply::Read`].
    fn read_asset(&mut self, id: AssetId) -> Ticket {
        wit::read_asset(id.0)
    }

    /// Gives one of the plugin's own assets new bytes:
    /// [`Reply::Updated`].
    fn update_asset(&mut self, id: AssetId, bytes: &[u8]) -> Ticket {
        wit::update_asset(id.0, bytes)
    }

    fn delete_asset(&mut self, id: AssetId) -> Ticket {
        wit::delete_asset(id.0)
    }

    /// The assets of the type `kind`: [`Reply::Listed`].
    fn list_assets(&mut self, kind: &str) -> Ticket {
        wit::list_assets(kind)
    }

    /// Holds a reference to the asset `id`: [`Reply::Held`], then an
    /// [`AssetChanged`] at each new version and [`AssetGone`] when it
    /// goes.
    fn hold_asset(&mut self, id: AssetId) -> Ticket {
        wit::hold_asset(id.0)
    }

    fn release_asset(&mut self, id: AssetId) -> Ticket {
        wit::release_asset(id.0)
    }

    /// Calls the service `service` (`plugin.id/name`) of another
    /// plugin with `argument`: [`Reply::Answered`].
    fn call(&mut self, service: &str, argument: &[u8]) -> Ticket {
        wit::call(service, argument)
    }

    /// Subscribes to the event `event` (`plugin.id/name`) of another
    /// plugin: [`Reply::Subscribed`], then each as an
    /// [`EventReceived`].
    fn subscribe(&mut self, event: &str) -> Ticket {
        wit::subscribe(event)
    }

    /// Sends the plugin's own event `event` with `payload`. A reply
    /// comes only when it is refused.
    fn emit(&mut self, event: &str, payload: &[u8]) -> Ticket {
        wit::emit(event, payload)
    }

    /// Fetches `request` from a host the manifest asks `net.fetch`
    /// for: [`Reply::Fetched`].
    fn fetch(&mut self, request: &FetchRequest) -> Ticket {
        encode(request).map_or(0, |bytes| wit::fetch(&bytes))
    }

    /// Asks the agents service `request`: [`Reply::Agents`], or an
    /// [`AgentsEventReceived`] for each change a watch tells of.
    fn agents(&mut self, request: &AgentsRequest) -> Ticket {
        encode(request).map_or(0, |bytes| wit::agents(&bytes))
    }

    /// Asks the desktop service `request`: [`Reply::Desktop`], or a
    /// [`DesktopEventReceived`] for each change a watch tells of.
    fn desktop(&mut self, request: &DesktopRequest) -> Ticket {
        encode(request).map_or(0, |bytes| wit::desktop(&bytes))
    }
}

impl HostExt for Commands<'_, '_> {}

impl HostExt for World {}

/// Writes `mail` into the world as the message it is.
pub(crate) fn deliver(world: &mut World, mail: Mail) {
    match mail {
        Mail::Reply {
            ticket,
            reply: Ok(Reply::Agents(AgentsReply::Event(event))),
        } => {
            world
                .write_message(AgentsEventReceived { ticket, event });
        }
        Mail::Reply {
            ticket,
            reply: Ok(Reply::Desktop(DesktopReply::Event(event))),
        } => {
            world.write_message(DesktopEventReceived {
                ticket,
                event,
            });
        }
        Mail::Reply { ticket, reply } => {
            world.write_message(Replied { ticket, reply });
        }
        Mail::Changed { asset, version } => {
            world.write_message(AssetChanged { asset, version });
        }
        Mail::Gone(asset) => {
            world.write_message(AssetGone { asset });
        }
        Mail::Event {
            from,
            event,
            payload,
        } => {
            world.write_message(EventReceived {
                from,
                event,
                payload,
            });
        }
    }
}
