//! The Hamlet server: the realm, its society, and its agents'
//! conversations and work.

mod after_input;
mod attached;
mod background;
mod calls;
mod change;
mod claim;
mod clients;
mod clock;
mod commands;
mod core;
mod credentials;
mod diagnostic;
mod dreaming;
mod fault;
mod files;
mod images;
mod inbox;
mod jobs;
mod knowledge;
mod layout;
mod oversight;
mod providers;
mod realm;
pub mod serve;
mod server;
mod settings;
mod society;
mod spend;
mod startup;
mod talk;
#[cfg(test)]
mod testing;
mod text;
mod timers;
mod work;
mod workspace;

use bevy_app::App;

/// Every area of the server: the schedules the runner runs, and the
/// resources, systems and timers of each area.
pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        core::plugin,
        after_input::plugin,
        startup::plugin,
        background::plugin,
        calls::plugin,
        talk::sync::plugin,
        clients::plugin,
        commands::plugin,
        talk::runtimes::plugin,
        providers::plugin,
        dreaming::plugin,
        images::plugin,
        files::plugin,
    ));
}
