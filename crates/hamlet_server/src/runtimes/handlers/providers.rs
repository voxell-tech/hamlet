//! The messages about credentials and providers: a credential
//! requested and answered once the leases let it through, and a
//! provider's result or event, and what a turn learnt of a
//! provider's limits.

use bevy_app::{App, PostUpdate};
use bevy_ecs::error::Result;
use bevy_ecs::message::Messages;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Res, ResMut};
use hamlet_protocol::runtime;

use super::FromRuntime;
use crate::FollowUpSystems;
use crate::app::follow_up::keep_or_roll_back_input;
use crate::app::input::{AddHandler, InputOrigin};
use crate::core::Core;
use crate::runtimes::leases::Authority;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        answer_credentials
            .run_if(is_from_runtime)
            .after(keep_or_roll_back_input)
            .in_set(FollowUpSystems::KeepOrRollBackInput),
    )
    .add_handler::<FromRuntime<runtime::CredentialRequest>, _>(
        credential_request,
    )
    .add_handler::<FromRuntime<runtime::ProviderResult>, _>(
        provider_result,
    )
    .add_handler::<FromRuntime<runtime::ProviderEvent>, _>(
        provider_event,
    )
    .add_handler::<FromRuntime<runtime::LimitsSeen>, _>(limits_seen);
}

/// Run condition for an input from a runtime.
fn is_from_runtime(origin: Res<InputOrigin>) -> bool {
    matches!(*origin, InputOrigin::Runtime { .. })
}

/// Answers the credentials the leases now let through, after every
/// line of a runtime, failed or not.
fn answer_credentials(mut core: Core) {
    core.answer_credentials();
}

/// A credential requested, answered once the leases let it through.
fn credential_request(
    mut heard: ResMut<
        Messages<FromRuntime<runtime::CredentialRequest>>,
    >,
    mut leases: ResMut<Authority>,
) {
    for FromRuntime { runtime, message } in heard.drain() {
        leases.request(runtime, message);
    }
}

fn provider_result(
    mut heard: ResMut<Messages<FromRuntime<runtime::ProviderResult>>>,
    mut core: Core,
) -> Result {
    heard
        .drain()
        .try_for_each(|FromRuntime { runtime, message }| {
            core.provider_result(Some(runtime), message)?;
            Ok(())
        })
}

fn provider_event(
    mut heard: ResMut<Messages<FromRuntime<runtime::ProviderEvent>>>,
    mut core: Core,
) -> Result {
    heard
        .drain()
        .try_for_each(|FromRuntime { runtime, message }| {
            core.provider_event(runtime, message)?;
            Ok(())
        })
}

fn limits_seen(
    mut heard: ResMut<Messages<FromRuntime<runtime::LimitsSeen>>>,
    mut core: Core,
) -> Result {
    heard.drain().try_for_each(|FromRuntime { message, .. }| {
        core.limits_seen(message)?;
        Ok(())
    })
}
