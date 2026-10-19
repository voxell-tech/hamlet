//! The wind made visible: a gust now and then over the ground the
//! camera looks at. The lines themselves are `hamlet_vfx`'s.

use bevy::prelude::*;
use hamlet_vfx::Density;
use hamlet_vfx::magic::chance;
use hamlet_vfx::wind::{Breeze, WindSettings, gust};

use super::land::Land;
use super::view::GenView;
use crate::sky::SkySettings;
use crate::{Clock, Sky};

/// The time the next gust is due, and the count of gusts so far.
#[derive(Default)]
pub(super) struct Due {
    at: f32,
    count: u32,
}

/// Sets off a gust when one is due. A world with no particles has
/// no wind to see, and a still has none in the air.
pub(super) fn gusts(
    mut commands: Commands,
    clock: Res<Clock>,
    set: Res<WindSettings>,
    density: Res<Density>,
    breeze: Option<Res<Breeze>>,
    view: Res<GenView>,
    land: Res<Land>,
    sky: Res<Sky>,
    sky_set: Res<SkySettings>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut due: Local<Due>,
) {
    let Some(breeze) = breeze else {
        return;
    };
    if clock.frozen || density.0 <= 0.0 || clock.t < due.at {
        return;
    }
    let own = due.count as f32 * 7.31;
    let roll = |salt: f32| chance(own + salt);
    due.count += 1;
    due.at = clock.t
        + (set.gap.0 + (set.gap.1 - set.gap.0) * roll(0.1))
            / density.0;

    // Somewhere in what the camera sees, larger from further off so
    // that it still shows.
    let size = (view.dist / 70.0).clamp(0.7, 7.0);
    let reach = view.dist * 0.55;
    let over = view.focus
        + Vec2::new(roll(0.2) - 0.5, roll(0.3) - 0.5) * 2.0 * reach;
    let height =
        set.height.0 + (set.height.1 - set.height.0) * roll(0.4);
    let at = land.on_ground(over) + Vec3::Y * height * size;
    let yaw = (set.heading + set.veer * (roll(0.5) * 2.0 - 1.0))
        .to_radians();
    let day = sky_set.daylight(sky.hour);
    gust(
        &mut commands,
        &breeze,
        &mut materials,
        &set,
        clock.t,
        at,
        yaw,
        size * (0.8 + 0.4 * roll(0.6)),
        set.night + (1.0 - set.night) * day,
    );
}
