//! The Hamlet server: the realm, its society, and its agents'
//! conversations and work, as a Bevy app with no frame (ADR 0015).
//!
//! Read it in the order it runs:
//!
//! 1. `main` adds [`ServerPlugin`] to an empty app and runs it.
//! 2. [`ServerPlugin`] adds the schedules [`ServerStartup`] and
//!    [`AfterInput`], every area's plugin, and the runner.
//! 3. The runner, `serve::runner`, opens the realm and starts
//!    `server::Server` on it, which runs [`ServerStartup`] once. It
//!    then listens for clients and runs each input until a signal
//!    comes, and closes in order.
//! 4. `server::Server::run` runs one input in one transaction, then
//!    [`AfterInput`], in the order of [`AfterInputStep`].
//! 5. Each input is handled by its area: a runtime's message in
//!    `talk::heard`, a client's request in `clients::requests`, a
//!    chief's command in `commands`.

use bevy_app::{App, Plugin};
use bevy_ecs::schedule::{
    IntoScheduleConfigs, Schedule, ScheduleLabel,
    SingleThreadedExecutor, SystemSet,
};

use crate::options::Options;

// The app, its runner and what runs around each input.
mod after_input;
mod background;
mod core;
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

// What every area shares.
mod clock;
mod diagnostic;
mod fault;
#[cfg(test)]
mod testing;
mod text;

/// The server: its schedules, every area, and the runner that serves
/// the realm `options` name.
pub struct ServerPlugin {
    pub options: Options,
}

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_schedule(single_threaded(ServerStartup))
            .add_schedule(single_threaded(AfterInput))
            .configure_sets(
                AfterInput,
                (
                    AfterInputStep::FollowEndedInputs,
                    AfterInputStep::NarrowToolsToRoles,
                    AfterInputStep::CommitInput,
                    AfterInputStep::RunKeptJobs,
                    AfterInputStep::StopCancelledToolRuns,
                    AfterInputStep::StartToolRunsAndBackgroundWork,
                    AfterInputStep::SendRuntimesWhatTheyAreOwed,
                    AfterInputStep::PublishState,
                    AfterInputStep::SendClientMessages,
                    AfterInputStep::WriteRuntimeLines,
                    AfterInputStep::PushOwedLodges,
                )
                    .chain(),
            )
            .add_plugins((
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
            ))
            .set_runner(serve::runner(self.options.clone()));
    }
}

/// A schedule run on one thread: SQLite has one writer, and the gain
/// is the order.
fn single_threaded(label: impl ScheduleLabel) -> Schedule {
    let mut schedule = Schedule::new(label);
    schedule.set_executor(SingleThreadedExecutor::new());
    schedule
}

/// The schedule the runner runs once, before the first input.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ServerStartup;

/// The schedule the runner runs after each input.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct AfterInput;

/// The steps of [`AfterInput`], in their order.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AfterInputStep {
    /// The inputs that ended are followed.
    FollowEndedInputs,
    /// Each agent's tools are narrowed to its role.
    NarrowToolsToRoles,
    /// The changes of the input and of the steps before are kept.
    CommitInput,
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
}
