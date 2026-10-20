//! The host of WASM plugins: wasmtime with the component model, a
//! store for each plugin, and each call into a plugin held to its
//! budgets. The only engine crate that knows wasmtime.

mod budget;
mod folder;
mod host;
mod instance;
mod panel;
mod run;

#[cfg(test)]
mod tests;

use bevy::app::App;

pub use self::folder::{MANIFEST, Package, WASM};
pub use self::host::{Host, WasmTuning};
pub use self::instance::{Caps, Fault, Instance, Spent};
pub use self::panel::{Drawn, Shows};

pub fn plugin(app: &mut App) {
    app.add_plugins((
        host::plugin,
        folder::plugin,
        run::plugin,
        panel::plugin,
    ));
}
