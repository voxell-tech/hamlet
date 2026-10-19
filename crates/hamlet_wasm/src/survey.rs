//! Whether the realm keeps its survey for the plugins.

use bevy::prelude::*;
use hamlet_realm_api::survey::SurveyWanted;
use hamlet_sdk::capability::{Capability, Granted};

/// Wants the survey while a plugin holds `read.world`, or names
/// `world.act`, whose target is found in it.
pub fn want(
    plugins: Query<&Granted>,
    mut wanted: ResMut<SurveyWanted>,
) {
    let now = plugins.iter().any(|granted| {
        granted.holds(Capability::ReadWorld)
            || granted.verdict(Capability::WorldAct).is_some()
    });
    wanted.set_if_neq(SurveyWanted(now));
}
