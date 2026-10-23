//! The door lanterns and the common's lantern: their glass shows the
//! state of the agent inside, and after dark each in view throws
//! light on the ground.

use std::f32::consts::PI;

use glam::Vec3;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    Light, Material, PointLight, StandardMaterial,
};

use crate::build::frame::Frame;
use crate::build::stock::rgba;
use crate::build::upload::Draw;
use crate::folk::keeper::KState;
use crate::world::palette::Palette;
use crate::world::sky::Sky;
use crate::world::tint::to_linear;

/// The hues of a lantern, in sRGB: at work, waiting for the chief,
/// faulted, and a candle at rest.
pub const CURRENT: Vec3 = Vec3::new(0.247, 0.847, 0.910);
pub const SIGNAL: Vec3 = Vec3::new(1.0, 0.541, 0.122);
const FAULT: Vec3 = Vec3::new(1.0, 0.322, 0.278);
const CANDLE: Vec3 = Vec3::new(1.0, 0.816, 0.541);

/// How bright a candle burns, and a lantern that is lit.
const EMBER: f32 = 1.5;
const LIT: f32 = 4.0;

/// Lights that shine at once, the nearest the camera: the engine's 64
/// lights a scene, less 8 kept for the sun and its fill, the library
/// and the chief.
const SPILLS: usize = 56;
/// Of those, the lights kept for the glows of the towns, the Hearth
/// and the library, which shine from any distance. The lamps have
/// the rest.
const TOWNS: usize = 20;
/// How much more a glow is than a door lantern's light, in lumens,
/// and how much further it reaches. Strong over a reach not much
/// wider than the place, so that it lights it and does not wash it.
const TOWN_LUMENS: f32 = 24.0;
const TOWN_REACH: f32 = 2.5;
/// Keys of the spills' entities.
const SPILL_KEY: u32 = 6;

/// The states in the order of [`Lamps`]' materials.
const STATES: [KState; 5] = [
    KState::Rest,
    KState::Working,
    KState::Waiting,
    KState::Fault,
    KState::Failed,
];

/// The place of `state` among [`STATES`].
pub fn state_index(state: KState) -> usize {
    STATES.iter().position(|&s| s == state).unwrap_or(0)
}

/// A door lantern's hue and strength for the state of the agent
/// inside, `t` seconds into the clock.
fn lantern(state: KState, t: f32) -> (Vec3, f32) {
    match state {
        KState::Rest => (to_linear(CANDLE), EMBER * 1.4),
        KState::Working => (to_linear(CURRENT), LIT),
        KState::Waiting => {
            (to_linear(SIGNAL), LIT * (0.8 + 0.2 * (t * PI).sin()))
        }
        KState::Fault => {
            let tick = (t * 2.6).floor();
            let drop =
                ((tick * 12.9898).sin() * 43758.547).fract().abs()
                    < 0.4;
            (to_linear(FAULT), if drop { LIT * 0.08 } else { LIT })
        }
        KState::Failed => (Vec3::ZERO, 0.0),
    }
}

/// The glass of every lantern: one shared material per state,
/// whatever the number of houses, and the lights the nearest throw.
pub struct Lamps {
    pub glass: [Handle<Material>; 5],
    /// The glow each glass was last given.
    shown: [Vec3; 5],
    /// Each light, and how it shines while it does.
    spills: Vec<(Entity, Option<Glow>)>,
}

/// The light a lamp throws on the ground.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Glow {
    pub at: Vec3,
    /// Linear.
    pub colour: Vec3,
    pub lumens: f32,
    pub reach: f32,
}

impl Glow {
    /// The glow of a town whose common's lantern hangs at `at`: a
    /// candle's colour, as strong as door lanterns of `lumens` and
    /// reaching `reach` give, but wider, so that a town lit from far
    /// is not dark.
    pub fn town(at: Vec3, lumens: f32, reach: f32) -> Self {
        Self {
            at,
            colour: lantern(KState::Rest, 0.0).0,
            lumens: lumens * TOWN_LUMENS,
            reach: reach * TOWN_REACH,
        }
    }

    /// The light of a door lantern at `at` for the state of the agent
    /// inside, `lumens` strong and reaching `reach`.
    pub fn lantern(
        at: Vec3,
        state: KState,
        lumens: f32,
        reach: f32,
    ) -> Self {
        Self {
            at,
            colour: lantern(state, 0.0).0,
            lumens,
            reach,
        }
    }
}

impl Lamps {
    pub fn make(
        pal: &Palette,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        let pbr = StandardMaterial {
            perceptual_roughness: 0.4,
            ..StandardMaterial::colored(rgba(pal.glass_lit))
        };
        let glass = [
            draw.material(pbr)?,
            draw.material(pbr)?,
            draw.material(pbr)?,
            draw.material(pbr)?,
            draw.material(pbr)?,
        ];
        let spills = (0..SPILLS)
            .map(|_| (draw.spawn(None, SPILL_KEY), None))
            .collect();
        Ok(Self {
            glass,
            shown: [Vec3::NAN; 5],
            spills,
        })
    }

    /// The material of a lantern of `state`.
    pub fn of(&self, state: KState) -> &Handle<Material> {
        &self.glass[state_index(state)]
    }

    /// Lights each glass as its state shows at clock time `t`. A
    /// lantern at rest is a candle: lit only after dark.
    pub fn light(
        &mut self,
        pal: &Palette,
        sky: &Sky,
        t: f32,
        draw: &Draw,
    ) {
        for (i, state) in STATES.into_iter().enumerate() {
            let (hue, strength) = lantern(state, t);
            let strength = if state == KState::Rest {
                strength * sky.look.candle
            } else {
                strength
            };
            let glow = hue * strength;
            if self.shown[i] != glow {
                self.shown[i] = glow;
                draw.set_material(
                    &self.glass[i],
                    StandardMaterial {
                        emissive: rgba(glow),
                        perceptual_roughness: 0.4,
                        ..StandardMaterial::colored(rgba(
                            pal.glass_lit,
                        ))
                    },
                );
            }
        }
    }

    /// Has the lamps of `lit` throw light on the ground, and the
    /// towns of `towns` glow, the nearest `eye` first while there are
    /// lights to give. None that is dark does.
    pub fn spill(
        &mut self,
        lit: Vec<Glow>,
        towns: Vec<Glow>,
        eye: Vec3,
        draw: &mut Draw,
    ) {
        let nearest = |mut glows: Vec<Glow>| {
            glows.retain(|glow| glow.lumens > 0.0);
            glows.sort_by(|a, b| {
                a.at.distance_squared(eye)
                    .total_cmp(&b.at.distance_squared(eye))
            });
            glows
        };
        let (lit, towns) = (nearest(lit), nearest(towns));
        for (i, (entity, shown)) in self.spills.iter_mut().enumerate()
        {
            let want = match i.checked_sub(TOWNS) {
                None => towns.get(i),
                Some(at) => lit.get(at),
            }
            .copied();
            if *shown == want {
                continue;
            }
            *shown = want;
            match want {
                Some(glow) => {
                    draw.place(*entity, Frame::at(glow.at));
                    draw.commands.entity(*entity).insert(SceneLight(
                        Light::Point(PointLight {
                            color: rgba(glow.colour),
                            intensity: glow.lumens,
                            range: glow.reach,
                            radius: 0.0,
                            shadow_maps_enabled: false,
                        }),
                    ));
                }
                None => {
                    draw.commands
                        .entity(*entity)
                        .remove::<SceneLight>();
                }
            }
        }
    }
}
