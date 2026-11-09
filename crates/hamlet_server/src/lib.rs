//! The Hamlet server: the realm, its society, and its agents'
//! conversations and work, as a Bevy app with no frame (ADR 0015).
//!
//! Read it in the order it runs:
//!
//! 1. `main` reads the [`Options`] and runs the app [`create_app`]
//!    makes of them.
//! 2. [`create_app`] adds the log, every area's plugin, the order of
//!    the [`FollowUpSystems`] and the [`SendSystems`], and the
//!    runner.
//! 3. The runner, `app::runner::runner`, updates the app a first
//!    time: `PreStartup` opens what the server serves, from the
//!    sandbox and the realm to the listener, and `Startup` starts it
//!    (`app::startup`). It then runs each input until the app exits:
//!    a stop signal is an input that takes the server from `Open`
//!    through `Closing` to `Stopping` (`app::shutdown`).
//! 4. `app::runner::Server::run` writes one input as a message and
//!    updates the app once, in one transaction:
//!    - `PreUpdate` takes the input in and reads it into a message of
//!      its kind: a runtime's message in `runtimes::handlers`, a
//!      client's request in `clients::handlers`;
//!    - `Update` runs the system for that kind;
//!    - `PostUpdate` keeps the input, or rolls it back when a system
//!      for it failed, then follows it and runs its jobs;
//!    - `Last` sends the runtimes and the clients what it queued.

use std::io::IsTerminal;

use bevy_app::{App, Last, PostUpdate};
use bevy_ecs::schedule::{
    IntoScheduleConfigs, Schedules, SingleThreadedExecutor, SystemSet,
};
use bevy_log::{BoxedFmtLayer, LogPlugin};

use crate::app::options::Options;

// The app shell: inputs, runner, startup, shutdown, follow-ups,
// timers, tasks, options, the clock and the fault points.
pub mod app;
mod core;

// The realm on disk: its folder, database, jobs, settings and
// workspace, and its knowledge.
mod knowledge;
mod realm;

// The society, what it spends and what oversees it.
mod society;

// The agents' conversations, the questions they put, and their tools.
mod calls;
mod conversations;
mod providers;
mod runtimes;
mod tools;

// The clients, and what a chief sends and types.
mod attachments;
mod clients;
mod terminals;
mod web;

// What every area shares.
#[cfg(test)]
mod testing;

/// Create the server's app.
pub fn create_app(options: Options) -> App {
    let mut app = App::new();

    app.set_runner(app::runner::runner).insert_resource(options);

    app.configure_sets(
        PostUpdate,
        (
            FollowUpSystems::KeepOrRollBackInput,
            FollowUpSystems::FollowEndedInputs,
            FollowUpSystems::NarrowToolsToRoles,
            FollowUpSystems::CommitFollowUps,
            FollowUpSystems::RunKeptJobs,
            FollowUpSystems::StopCancelledToolRuns,
            FollowUpSystems::StartToolRunsAndBackgroundWork,
        )
            .chain(),
    )
    .configure_sets(
        Last,
        (
            SendSystems::SendRuntimesWhatTheyAreOwed,
            SendSystems::PublishState,
            SendSystems::SendClientMessages,
            SendSystems::WriteRuntimeLines,
            SendSystems::PushOwedWorktrees,
            SendSystems::ForgetInput,
        )
            .chain(),
    );

    // Plugins
    app.add_plugins(LogPlugin {
        fmt_layer: log_lines,
        ..Default::default()
    })
    // The runner's side: inputs, what follows them, and what runs
    // off the loop.
    .add_plugins((app::plugin, core::plugin, knowledge::plugin))
    // The areas, each with the systems for the inputs it takes.
    .add_plugins((
        calls::plugin,
        runtimes::plugin,
        clients::plugin,
        providers::plugin,
        conversations::plugin,
        attachments::plugin,
        terminals::plugin,
        tools::plugin,
        web::plugin,
    ));

    // Every schedule runs its systems one at a time on the runner's
    // thread, inside its tokio runtime, whatever executor Bevy's
    // features choose: the systems spawn tasks, and an input's
    // update is one transaction in order.
    for (_, schedule) in
        app.world_mut().resource_mut::<Schedules>().iter_mut()
    {
        schedule.set_executor(SingleThreadedExecutor::new());
    }

    /// Bevy's readable lines in a terminal, else one JSON object per
    /// line, so words with a newline or a terminal escape cannot
    /// break or forge a line of `server.log`.
    fn log_lines(_: &mut App) -> Option<BoxedFmtLayer> {
        if std::io::stderr().is_terminal() {
            return None;
        }
        Some(Box::new(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(std::io::stderr),
        ))
    }

    app
}

/// What follows each input in `PostUpdate`, in its order: the input
/// kept or rolled back, then followed. Only for `PostUpdate`.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FollowUpSystems {
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
    /// The tool runs and the tasks the input set off start.
    StartToolRunsAndBackgroundWork,
}

/// What a kept input queued, sent in `Last`, in its order. Only for
/// `Last`.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SendSystems {
    /// Each ready runtime is sent what it is owed, and told of the
    /// held calls whose waits were answered.
    SendRuntimesWhatTheyAreOwed,
    /// The clients are sent the realm's state.
    PublishState,
    /// The clients are sent what the input queued for them.
    SendClientMessages,
    /// The runtimes are sent what the input queued for them.
    WriteRuntimeLines,
    /// The worktrees the input owes the remote workspace are pushed.
    PushOwedWorktrees,
    /// What the input left is cleared for the next.
    ForgetInput,
}
