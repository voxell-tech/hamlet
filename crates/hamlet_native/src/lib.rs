//! Native plugins: a dynamic library built against this Hamlet's
//! toolchain and engine, loaded when the app starts, and run as the
//! app with no limits. The contract a library keeps is here for the
//! plugin to build against; the host that loads one is behind the
//! `host` feature.

#[cfg(feature = "host")]
use bevy::prelude::*;
#[cfg(feature = "host")]
use hamlet_sdk::capability::open_store;

pub mod abi;
pub mod gate;
#[cfg(feature = "host")]
pub mod install;
#[cfg(feature = "host")]
pub mod library;
#[cfg(feature = "host")]
pub mod load;
#[cfg(feature = "host")]
pub mod manifest;
#[cfg(feature = "host")]
pub mod marker;
#[cfg(feature = "host")]
pub mod remove;

pub use bevy::app::App;

pub use self::abi::{ABI, Abi, declare};

/// Loads the installed native plugins and the one the chief lets in.
/// Add it after the core and the SDK.
#[cfg(feature = "host")]
pub fn plugin(app: &mut App) {
    app.add_observer(load::admit)
        .add_observer(remove::removed)
        .add_systems(Startup, load::installed.after(open_store));
}
