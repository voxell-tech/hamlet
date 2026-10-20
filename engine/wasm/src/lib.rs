//! The host of WASM plugins: wasmtime with the component model, a
//! store for each plugin, and each call into a plugin held to its
//! budgets. The only engine crate that knows wasmtime.

mod add;
mod budget;
mod folder;
mod host;
mod instance;
mod mail;
mod panel;
mod reload;
mod run;

#[cfg(test)]
mod tests;

use bevy::app::App;

pub use self::add::AddFrom;
pub use self::folder::{MANIFEST, Package, WASM};
pub use self::host::{Host, WasmTuning};
pub use self::instance::{Caps, Fault, Instance, Spent};
pub use self::panel::{Drawn, Input, Shows};

pub fn plugin(app: &mut App) {
    app.add_plugins((
        host::plugin,
        folder::plugin,
        add::plugin,
        run::plugin,
        panel::plugin,
        reload::plugin,
    ));
}
