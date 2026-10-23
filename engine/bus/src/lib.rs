//! Services and events between plugins. The bus takes what plugins
//! ask through [`Asked`](hamlet_engine_plugins::Asked), checks it and
//! passes it on; a host hands a plugin what is for it and says what
//! became of it. Nothing here knows how a plugin is run.

mod event;
mod service;

#[cfg(test)]
mod tests;

use bevy::app::App;

pub use crate::event::Subscriptions;
pub use crate::service::{Admitted, Call, Calls, Serve, Served};

pub fn plugin(app: &mut App) {
    app.add_plugins((service::plugin, event::plugin));
}
