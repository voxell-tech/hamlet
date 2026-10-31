//! The wind made visible: a gust now and then over the ground the
//! camera looks at. The lines themselves are `hamlet_vfx`'s.

use bevy::prelude::*;
use hamlet_vfx::Density;
use hamlet_vfx::magic::chance;
use hamlet_vfx::wind::{Breeze, WindSettings, gust};

use super::land::Land;
use super::view::GeneratorView;
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
    view: Res<GeneratorView>,
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

    // Where a band of strong wind is passing, of a few places in what
    // the camera sees, so that the line and the flags under it agree.
    // Larger from further off so that it still shows.
    let size = (view.dist / 70.0).clamp(0.7, 7.0);
    let reach = view.dist * 0.55;
    let (over, strong) = (0..PLACES)
        .map(|i| {
            let salt = 0.2 + i as f32 * 0.17;
            let over = view.focus
                + Vec2::new(
                    roll(salt) - 0.5,
                    roll(salt + 0.08) - 0.5,
                ) * 2.0
                    * reach;
            (over, set.strength(over, clock.t))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((view.focus, 1.0));
    let height =
        set.height.0 + (set.height.1 - set.height.0) * roll(0.04);
    let at = land.on_ground(over) + Vec3::Y * height * size;
    let day = sky_set.daylight(sky.hour);
    gust(
        &mut commands,
        &breeze,
        &mut materials,
        &set,
        clock.t,
        at,
        set.blowing(clock.t),
        size * (0.8 + 0.4 * roll(0.06)),
        strong * (set.night + (1.0 - set.night) * day),
    );
}

/// The places a gust may set off from, of which it takes the
/// windiest.
const PLACES: usize = 6;
