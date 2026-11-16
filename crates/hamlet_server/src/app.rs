//! The server's app shell: the inputs and their channel, the runner,
//! the first update (`startup`), stopping (`shutdown`), what follows
//! each input (`follow_up`), timers, tasks off the loop, the options,
//! the clock and the fault points.

pub(crate) mod clock;
pub(crate) mod failure;
pub(crate) mod fault;
pub(crate) mod follow_up;
pub(crate) mod input;
pub mod options;
pub(crate) mod runner;
pub(crate) mod shutdown;
pub(crate) mod startup;
pub(crate) mod tasks;
pub(crate) mod timers;

use bevy_app::App;

/// Adds the shell's plugins: the inputs, what follows them, the
/// first update, tasks off the loop, timers and stopping.
pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        input::plugin,
        follow_up::plugin,
        startup::plugin,
        tasks::plugin,
        timers::plugin,
        shutdown::plugin,
    ));
}
