//! The steps the runner takes once, before the first input: the
//! runtimes start, the tools added by hand load, each agent's tools
//! are narrowed to its role, the providers are asked what they
//! serve, and the turns the last stop left are followed. `Apply`
//! keeps what they changed.

use bevy_app::App;
use bevy_ecs::error::Result;
use bevy_ecs::schedule::{
    IntoScheduleConfigs, Schedule, ScheduleLabel,
    SingleThreadedExecutor,
};
use bevy_ecs::system::{Res, ResMut};
use hamlet_store::ids::Actor;

use crate::core::{Core, Tooling};
use crate::realm::Realm;
use crate::talk::follow::follow;
use crate::talk::inputs;
use crate::talk::runtimes::{self, Runtimes};
use crate::work::added;

pub(crate) fn plugin(app: &mut App) {
    let mut start = Schedule::new(Start);
    start.set_executor(SingleThreadedExecutor::new());
    app.add_schedule(start).add_systems(
        Start,
        (
            start_runtimes,
            load_tools,
            crate::apply::retool,
            ask_providers,
            follow_unfollowed,
        )
            .chain(),
    );
}

/// The schedule the runner runs once, before the first input.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Start;

fn start_runtimes(
    realm: ResMut<Realm>,
    runtimes: Runtimes,
) -> Result {
    runtimes::start(realm, runtimes)?;
    Ok(())
}

fn load_tools(
    realm: Res<Realm>,
    tools: Tooling,
    runtimes: Runtimes,
    changed: ResMut<crate::apply::Changed>,
) -> Result {
    added::reload(realm, tools, runtimes, changed)?;
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
