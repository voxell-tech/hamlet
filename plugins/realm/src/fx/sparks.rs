//! Sparks: embers that burst from the ground, and a swarm of
//! fireflies over each town by night. A realm drawn at the lowest
//! quality has none.

use std::collections::HashMap;

use glam::Vec2;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::Material;

use crate::build::upload::Draw;
use crate::fx::motes::{Look, Motes, Way};
use crate::fx::ready;
use crate::realm::Now;
use crate::society::{Society, TownId};
use crate::tuning::{VfxTuning, WorldTuning};
use crate::world::land::Land;
use crate::world::palette::Palette;
use crate::world::sky::{Quality, Sky};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Fireflies>()
        .add_systems(Update, keep_fireflies.run_if(ready));
}

/// The realm's sparks.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SparkSettings {
    /// Embers of a falling house at full quality, the seconds each
    /// lasts at most, how fast they rise and how big they begin.
    pub embers: u32,
    pub ember_life: f32,
    pub ember_speed: f32,
    pub ember_size: f32,
    /// Brightness of an ember and of a firefly, over white.
    pub ember_light: f32,
    pub firefly_light: f32,
    /// Fireflies born each second in a town at full quality, the
    /// seconds each lives, how far from the common they roam and how
    /// high, how fast they drift and how big they are.
    pub fireflies: f32,
    pub firefly_life: f32,
    pub firefly_reach: f32,
    pub firefly_height: f32,
    pub firefly_speed: f32,
    pub firefly_size: f32,
}

impl SparkSettings {
    /// The burst of embers of a falling house, at `quality`. None
    /// where the realm has no sparks.
    pub fn embers(
        &self,
        pal: &Palette,
        quality: Quality,
        salt: f32,
    ) -> Option<Look> {
        let count = (self.embers as f32 * quality.density()) as u32;
        (count > 0).then(|| Look {
            colour: pal.ember * self.ember_light,
            way: Way::Burst,
            count,
            reach: 2.4 + self.ember_speed * self.ember_life,
            salt: salt.rem_euclid(4096.0).floor(),
            own: [
                2.4,
                self.ember_speed,
                self.ember_life,
                self.ember_size,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
            added: false,
        })
    }

    /// The fireflies of a town, at `quality`. None where the realm
    /// has no sparks.
    fn swarm(&self, pal: &Palette, quality: Quality) -> Option<Look> {
        let rate = self.fireflies * quality.density();
        (rate > 0.0).then(|| Look {
            colour: pal.firefly * self.firefly_light,
            way: Way::Swarm,
            count: (rate * self.firefly_life).ceil() as u32 + 8,
            reach: self.firefly_reach
                + self.firefly_height
                + self.firefly_speed * self.firefly_life,
            salt: 0.0,
            own: [
                self.firefly_reach,
                self.firefly_height,
                self.firefly_speed,
                self.firefly_size,
                self.firefly_life,
                rate,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
            added: false,
        })
    }
}

/// How a town's fireflies look, and their material.
type Swarm = (Look, Handle<Material>);

/// The swarm of each town near the camera, and the material they
/// share, made for the quality it was made for.
#[derive(Resource, Default)]
struct Fireflies {
    made: Option<(Quality, Option<Swarm>)>,
    swarms: HashMap<TownId, (Vec2, Entity)>,
}

/// Keeps a swarm of fireflies over each town near the camera by
/// night.
fn keep_fireflies(
    tuning: Res<WorldTuning>,
    vfx: Res<VfxTuning>,
    pal: Res<Palette>,
    quality: Res<Quality>,
    motes: Res<Motes>,
    sky: Res<Sky>,
    land: Res<Land>,
    society: Res<Society>,
    now: Res<Now>,
    mut flies: ResMut<Fireflies>,
    mut draw: Draw,
) {
    let flies = &mut *flies;
    if flies
        .made
        .as_ref()
        .is_none_or(|(made, _)| made != &*quality)
    {
        for (_, (_, swarm)) in flies.swarms.drain() {
            draw.despawn(swarm);
        }
        let swarm =
            vfx.sparks.swarm(&pal, *quality).and_then(|look| {
                match motes.material(&look, 0.0, &mut draw) {
                    Ok(material) => Some((look, material)),
                    Err(why) => {
                        hamlet_engine_bevy::warn(&format!(
                            "fireflies: {why}"
                        ));
                        None
                    }
                }
            });
        flies.made = Some((*quality, swarm));
    }
    let Some((_, Some((look, material)))) = &flies.made else {
        return;
    };
    let night = tuning.sky.night(sky.hour);
    let far = tuning.detail.town_far;
    let wanted = society
        .towns
        .iter()
        .map(|town| (town.id, land.sites[town.site].at))
        .filter(|(_, at)| {
            night && land.on_ground(*at).distance(now.eye) < far
        })
        .collect::<HashMap<_, _>>();
    // A swarm stays where its town stands.
    flies.swarms.retain(|town, (at, swarm)| {
        let kept = wanted.get(town) == Some(at);
        if !kept {
            draw.despawn(*swarm);
        }
        kept
    });
    for (town, at) in wanted {
        if flies.swarms.contains_key(&town) {
            continue;
        }
        let ground = land.on_ground(at);
        let swarm = motes.spawn(material, look, ground, &mut draw);
        flies.swarms.insert(town, (at, swarm));
    }
}
