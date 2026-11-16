//! The Hamlet server: the realm, its society, and its agents'
//! conversations and work, as a Bevy app with no frame (ADR 0015).
//!
//! Read it in the order it runs:
//!
//! 1. `main` adds [`ServerPlugin`] to an app and runs it.
//! 2. [`ServerPlugin`] adds every area's plugin, the order of the
//!    [`ServerSystems`], and the runner.
//! 3. The runner, `serve::runner`, updates the app a first time:
//!    `PreStartup` opens what the server serves, from the sandbox and
//!    the realm to the listener, and `Startup` starts it (`startup`).
//!    It then runs each input until the app exits: a stop signal is
//!    an input that takes the server from `Open` through `Closing` to
//!    `Stopping` (`closing`).
//! 4. `server::Server::run` writes one input as a message and updates
//!    the app once, in one transaction:
//!    - `PreUpdate` takes the input in and reads it into a message of
//!      its kind: a runtime's message in `talk::heard`, a client's
//!      request in `clients::asked`;
//!    - `Update` runs the system for that kind;
//!    - `PostUpdate` keeps the input, or rolls it back when a system
//!      for it failed, then follows it and runs its jobs;
//!    - `Last` sends the runtimes and the clients what it queued.

use bevy_app::{App, Last, Plugin, PostUpdate};
use bevy_ecs::schedule::{IntoScheduleConfigs, SystemSet};

use crate::options::Options;

// The app, its runner and what follows each input.
mod background;
mod closing;
mod core;
mod follow_up;
pub mod options;
mod serve;
mod server;
mod startup;
mod timers;

// The realm on disk: its folder, database, jobs and knowledge.
mod change;
mod claim;
mod credentials;
mod jobs;
mod knowledge;
mod layout;
mod realm;
mod settings;
mod workspace;

// The society, and what oversees and limits it.
mod oversight;
mod society;
mod spend;

// The agents' conversations, what they ask, and their tools.
mod calls;
mod dreaming;
mod providers;
mod talk;
mod work;

// The clients, and what a chief sends and types.
mod attached;
mod clients;
mod commands;
mod files;
mod images;
mod inbox;
mod terminals;

// What every area shares.
mod clock;
mod diagnostic;
mod fault;
#[cfg(test)]
mod testing;

/// The server: every area, the order of its systems, and the runner
/// that serves the realm `options` name.
pub struct ServerPlugin {
    pub options: Options,
}

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            PostUpdate,
            (
                ServerSystems::KeepOrRollBackInput,
                ServerSystems::FollowEndedInputs,
                ServerSystems::NarrowToolsToRoles,
                ServerSystems::CommitFollowUps,
                ServerSystems::RunKeptJobs,
                ServerSystems::StopCancelledToolRuns,
                ServerSystems::StartToolRunsAndBackgroundWork,
            )
                .chain(),
        )
        .configure_sets(
            Last,
            (
                ServerSystems::SendRuntimesWhatTheyAreOwed,
                ServerSystems::PublishState,
                ServerSystems::SendClientMessages,
                ServerSystems::WriteRuntimeLines,
                ServerSystems::PushOwedLodges,
                ServerSystems::ForgetInput,
            )
                .chain(),
        )
        // The runner's side: inputs, what follows them, and what runs
        // off the loop.
        .add_plugins((
            server::plugin,
            core::plugin,
            follow_up::plugin,
            startup::plugin,
            background::plugin,
            timers::plugin,
            knowledge::runner::plugin,
            closing::plugin,
        ))
        // The areas, each with the systems for the inputs it takes.
        .add_plugins((
            calls::plugin,
            talk::heard::plugin,
            talk::sync::plugin,
            talk::runtimes::plugin,
            clients::plugin,
            commands::plugin,
            providers::plugin,
            dreaming::plugin,
            images::plugin,
            files::plugin,
            terminals::plugin,
            work::added::plugin,
        ))
        .insert_resource(self.options.clone())
        .set_runner(serve::runner);
    }
}

/// What follows each input, in its order: in `PostUpdate` the input
/// kept and followed, in `Last` what it queued sent.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ServerSystems {
    /// The input's changes are kept, or rolled back when a system for
    /// it failed, and whoever sent it is told.
    KeepOrRollBackInput,
    /// The inputs that ended are followed.
    FollowEndedInputs,
    /// Each agent's tools are narrowed to its role.
    NarrowToolsToRoles,
    /// The changes of the follows and the narrowing are kept.
    CommitFollowUps,
    /// The jobs the kept changes left are run.
    RunKeptJobs,
    /// The tool runs of the stopped turns end.
    StopCancelledToolRuns,
    /// The tool runs and the tasks the input asked for start.
    StartToolRunsAndBackgroundWork,
    /// Each ready runtime is sent what it is owed, and told of the
    /// held calls whose waits were answered.
    SendRuntimesWhatTheyAreOwed,
    /// The clients are sent the realm's state.
    PublishState,
    /// The clients are sent what the input queued for them.
    SendClientMessages,
    /// The runtimes are sent what the input queued for them.
    WriteRuntimeLines,
    /// The lodges the input owes the remote workspace are pushed.
    PushOwedLodges,
    /// What the input left is cleared for the next.
    ForgetInput,
}
