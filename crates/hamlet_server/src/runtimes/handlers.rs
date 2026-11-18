//! A runtime's messages: one system for each kind. `PreUpdate` reads
//! a runtime's line into its message, written as a [`FromRuntime`] of
//! its kind, and `Update` runs the system for that kind: those of the
//! lifecycle of runtimes and conversations in [`lifecycle`], of a
//! running turn in [`turns`], and of credentials and providers in
//! [`providers`].

mod lifecycle;
mod providers;
mod turns;

use bevy_app::{App, PreUpdate};
use bevy_ecs::entity::Entity;
use bevy_ecs::error::Result;
use bevy_ecs::message::{Message, Messages};
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::on_message;
use bevy_ecs::system::{Commands, Res, ResMut};
use bevy_log::warn;
use hamlet_protocol::runtime::{ProtocolError, RuntimeMessage};

use super::Runtimes;
use super::process::Received;
use crate::app::input::{AddHandler, InputOrigin};
use crate::core::Core;
use crate::realm::credentials::Credentials;

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<RuntimeLineReceived>()
        .add_systems(
            PreUpdate,
            take_in_runtime_line
                .run_if(on_message::<RuntimeLineReceived>),
        )
        .add_handler::<FromRuntime<ProtocolError>, _>(error)
        .add_plugins((
            lifecycle::plugin,
            turns::plugin,
            providers::plugin,
        ));
}

/// A line from a runtime, or the end of its output.
#[derive(Message)]
pub(crate) struct RuntimeLineReceived(pub Received);

/// A message of the kind `M` from the runtime `runtime`.
#[derive(Message)]
pub(crate) struct FromRuntime<M: Send + Sync + 'static> {
    pub runtime: Entity,
    pub message: M,
}

/// Takes in a runtime's line: the end of its output fails the
/// runtime, and a message is written as one of its kind.
fn take_in_runtime_line(
    mut lines: ResMut<Messages<RuntimeLineReceived>>,
    mut core: Core,
    mut origin: ResMut<InputOrigin>,
    mut commands: Commands,
) {
    for RuntimeLineReceived(received) in lines.drain() {
        if !core.runtimes.is_current(&received) {
            continue;
        }
        let from = received.runtime;
        let Some(line) = received.line else {
            core.runtimes.failed(from);
            core.leases.gone(from);
            continue;
        };
        let message =
            match serde_json::from_str::<RuntimeMessage>(&line) {
                Ok(message) => message,
                Err(error) => {
                    let words = core
                        .credentials
                        .scrub_text(&error.to_string());
                    warn!(
                        "a runtime's line is not a message: {words}"
                    );
                    continue;
                }
            };
        *origin = InputOrigin::Runtime {
            runtime: from,
            awaited: awaited(&message),
        };
        write(&mut commands, from, message);
    }
}

/// Writes `message` from the runtime `from` as a [`FromRuntime`] of
/// its kind, for its system to read.
pub(crate) fn write(
    commands: &mut Commands,
    from: Entity,
    message: RuntimeMessage,
) {
    fn to<M: Send + Sync + 'static>(
        commands: &mut Commands,
        runtime: Entity,
        message: M,
    ) {
        commands.write_message(FromRuntime { runtime, message });
    }
    match message {
        RuntimeMessage::Allow(m) => to(commands, from, m),
        RuntimeMessage::Operation(m) => to(commands, from, m),
        RuntimeMessage::Hello(m) => to(commands, from, m),
        RuntimeMessage::Created(m) => to(commands, from, m),
        RuntimeMessage::Admitted(m) => to(commands, from, m),
        RuntimeMessage::Settled(m) => to(commands, from, m),
        RuntimeMessage::Kept(m) => to(commands, from, m),
        RuntimeMessage::Live(m) => to(commands, from, m),
        RuntimeMessage::Cancelled(m) => to(commands, from, m),
        RuntimeMessage::Configured(m) => to(commands, from, m),
        RuntimeMessage::Prompt(m) => to(commands, from, m),
        RuntimeMessage::Attempt(m) => to(commands, from, m),
        RuntimeMessage::Usage(m) => to(commands, from, m),
        RuntimeMessage::Yield(m) => to(commands, from, m),
        RuntimeMessage::Scrub(m) => to(commands, from, m),
        RuntimeMessage::Error(m) => to(commands, from, m),
        RuntimeMessage::CredentialRequest(m) => to(commands, from, m),
        RuntimeMessage::ProviderResult(m) => to(commands, from, m),
        RuntimeMessage::ProviderEvent(m) => to(commands, from, m),
        RuntimeMessage::Limits(m) => to(commands, from, m),
        RuntimeMessage::Page(_) | RuntimeMessage::Statuses(_) => {}
    }
}

/// The id of `message` when the runtime waits for its answer.
fn awaited(message: &RuntimeMessage) -> Option<String> {
    let id = match message {
        RuntimeMessage::Prompt(prompt) => &prompt.id,
        RuntimeMessage::Attempt(attempt) => &attempt.id,
        RuntimeMessage::Usage(report) => &report.id,
        RuntimeMessage::Allow(allow) => &allow.id,
        RuntimeMessage::Operation(operation) => &operation.id,
        RuntimeMessage::Yield(answer) => &answer.id,
        RuntimeMessage::Scrub(scrub) => &scrub.id,
        RuntimeMessage::CredentialRequest(request) => &request.id,
        _ => return None,
    };
    Some(id.clone())
}

/// A message of the server's the runtime could not handle.
fn error(
    mut heard: ResMut<Messages<FromRuntime<ProtocolError>>>,
    credentials: Res<Credentials>,
    runtimes: Runtimes,
) -> Result {
    heard.drain().try_for_each(
        |FromRuntime {
             runtime: from,
             message: error,
         }| {
            let words = credentials.scrub_text(&error.message);
            warn!("{}: {words}", runtimes.id(from)?);
            Ok(())
        },
    )
}
