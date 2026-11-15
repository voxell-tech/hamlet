//! The systems of `Startup`, which the first update runs before
//! anything else: the runtimes start, the tools added by hand load,
//! each agent's tools are narrowed to its role, the providers are
//! asked what they serve, and the turns the last stop left are
//! followed. `PostUpdate` keeps what they changed.

use bevy_app::{App, Startup};
use bevy_ecs::error::Result;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::ResMut;
use hamlet_store::ids::Actor;

use crate::core::Core;
use crate::realm::Realm;
use crate::talk::follow::follow;
use crate::talk::inputs;
use crate::talk::runtimes::{self, Runtimes};
use crate::work::added;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        Startup,
        (
            start_runtimes,
            added::reload,
            crate::follow_up::narrow_tools_to_roles,
            ask_providers,
            follow_unfollowed,
        )
            .chain(),
    );
}

fn start_runtimes(
    realm: ResMut<Realm>,
    runtimes: Runtimes,
) -> Result {
    runtimes::start(realm, runtimes)?;
    Ok(())
}

fn ask_providers(mut core: Core) -> Result {
    core.start_providers()?;
    Ok(())
}

/// Follows each input the last stop left unfollowed; no runtime waits
/// on anything from before it.
fn follow_unfollowed(mut core: Core) -> Result {
    let unfollowed =
        inputs::unfollowed(core.realm.store().connection())?;
    for input in unfollowed {
        core.change(Actor::Realm, |cx, env| {
            follow(cx, env, input.conversation, &input.request)
        })?;
    }
    Ok(())
}
