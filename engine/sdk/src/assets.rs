//! The asset store. Each request gives a [`Ticket`], and its reply
//! comes to [`Plugin::receive`](crate::Plugin::receive) on a later
//! frame as a [`Message::Reply`](crate::Message::Reply) with it.

use hamlet_engine_api::{AssetId, Ticket};

use crate::bindings::hamlet::engine::host;

/// Makes an asset of the type `kind`, one the plugin makes:
/// [`Reply::Created`](crate::Reply::Created).
pub fn create(kind: &str, name: &str, bytes: &[u8]) -> Ticket {
    host::create_asset(kind, name, bytes)
}

/// [`Reply::Read`](crate::Reply::Read).
pub fn read(id: AssetId) -> Ticket {
    host::read_asset(id.0)
}

/// Gives one of the plugin's own assets new bytes:
/// [`Reply::Updated`](crate::Reply::Updated) with its version.
pub fn update(id: AssetId, bytes: &[u8]) -> Ticket {
    host::update_asset(id.0, bytes)
}

/// Takes one of the plugin's own assets away.
pub fn delete(id: AssetId) -> Ticket {
    host::delete_asset(id.0)
}

/// The assets of the type `kind`:
/// [`Reply::Listed`](crate::Reply::Listed).
pub fn list(kind: &str) -> Ticket {
    host::list_assets(kind)
}

/// Holds a reference to the asset `id`:
/// [`Reply::Held`](crate::Reply::Held) with its version. A
/// [`Message::Changed`](crate::Message::Changed) comes each time it
/// is at a new version, and [`Message::Gone`](crate::Message::Gone)
/// when it goes.
pub fn hold(id: AssetId) -> Ticket {
    host::hold_asset(id.0)
}

/// Lets go of the asset `id`.
pub fn release(id: AssetId) -> Ticket {
    host::release_asset(id.0)
}
