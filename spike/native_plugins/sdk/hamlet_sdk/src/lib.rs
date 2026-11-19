//! Bevy and Hamlet's plugin API as one shared library, linked by the
//! app and by every plugin.

use std::sync::atomic::AtomicUsize;

pub use bevy;

/// The function a plugin exports as [`ENTRY`].
pub type Entry = fn(&mut bevy::app::App);

/// The name of a plugin's [`Entry`].
pub const ENTRY: &[u8] = b"hamlet_plugin";

/// A static the app and every plugin count on, to show whether they
/// see one copy of it.
pub static LOADS: AtomicUsize = AtomicUsize::new(0);

/// What a plugin reports to the app, to check that types, statics and
/// the allocator are shared.
#[derive(bevy::prelude::Resource, Default)]
pub struct Probes(pub Vec<Probe>);

/// One plugin's view of the process.
pub struct Probe {
    pub plugin: String,
    pub loads: usize,
    pub loads_address: usize,
    pub transform_type: std::any::TypeId,
    pub compute_pool_address: usize,
}
