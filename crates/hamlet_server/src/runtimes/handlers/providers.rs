//! The messages about credentials and providers: a credential
//! requested and answered once the leases let it through, and a
//! provider's result or event.

use bevy_app::{App, PostUpdate};
use bevy_ecs::error::Result;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Res, ResMut};
use hamlet_protocol::runtime;

use super::FromRuntime;
use crate::FollowUpSystems;
use crate::app::follow_up::keep_or_roll_back_input;
use crate::app::input::{AddHandler, InputMessage, InputOrigin};
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
    );
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
    mut heard: InputMessage<FromRuntime<runtime::CredentialRequest>>,
    mut leases: ResMut<Authority>,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: request,
    }) = heard.take()
    else {
        return Ok(());
    };
    leases.request(from, request);
    Ok(())
}

fn provider_result(
    mut heard: InputMessage<FromRuntime<runtime::ProviderResult>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: result,
    }) = heard.take()
    else {
        return Ok(());
    };
    core.provider_result(Some(from), result)?;
    Ok(())
}

fn provider_event(
    mut heard: InputMessage<FromRuntime<runtime::ProviderEvent>>,
    mut core: Core,
) -> Result {
    let Some(FromRuntime {
        runtime: from,
        message: event,
    }) = heard.take()
    else {
        return Ok(());
    };
    core.provider_event(from, event)?;
    Ok(())
}
