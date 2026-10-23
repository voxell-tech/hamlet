//! The land the realm is made of and its look: heights, rivers, sites
//! and plots, the ground's tiles and trees, the sky and the colours.

pub mod flora;
pub mod ground;
pub mod land;
pub mod layout;
pub mod noise;
pub mod palette;
pub mod sky;
pub mod tint;

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::SceneSync;

use crate::tuning::WorldTuning;
use crate::world::sky::{
    DayPart, Hour, Quality, SkyClock, finish, spend,
};

/// The part of the day, the quality the realm is drawn at, and the
/// sky held at dawn or at dusk. The quality is chosen in Settings.
pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<DayPart>()
        .init_resource::<Quality>()
        .add_command("sky_dawn", "Sky at Dawn", |sky: SkyHold| {
            sky.hold(|set| set.sunrise)
        })
        .add_menu_entry("View", "sky_dawn")
        .add_command("sky_dusk", "Sky at Dusk", |sky: SkyHold| {
            sky.hold(|set| set.sunset)
        })
        .add_menu_entry("View", "sky_dusk")
        .add_systems(PostUpdate, (spend, finish).before(SceneSync));
}

/// The sky's clock, to be held at an hour of the sky's settings.
#[derive(bevy_ecs::system::SystemParam)]
struct SkyHold<'w> {
    clock: ResMut<'w, SkyClock>,
    tuning: Option<Res<'w, WorldTuning>>,
}

impl SkyHold<'_> {
    fn hold(mut self, hour: impl Fn(&sky::SkySettings) -> f32) {
        if let Some(tuning) = &self.tuning {
            self.clock.hour = Hour::At(hour(&tuning.sky));
        }
    }
}
