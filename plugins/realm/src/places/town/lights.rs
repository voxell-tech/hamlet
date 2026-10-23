//! The door lanterns and the common's lantern: their glass shows the
//! state of the agent inside, and after dark the nearest throw light
//! on the ground.

use std::f32::consts::PI;

use glam::Vec3;
use hamlet_engine_sdk::scene::{
    Entity, Light, Material, Pbr, PointLight,
};

use crate::build::stock::rgba;
use crate::folk::keeper::KState;
use crate::world::palette::Palette;
use crate::world::sky::Sky;
use crate::world::tint::to_linear;

/// The hues of a lantern, in sRGB: at work, waiting for the chief,
/// faulted, and a candle at rest.
const CURRENT: Vec3 = Vec3::new(0.247, 0.847, 0.910);
const SIGNAL: Vec3 = Vec3::new(1.0, 0.541, 0.122);
const FAULT: Vec3 = Vec3::new(1.0, 0.322, 0.278);
const CANDLE: Vec3 = Vec3::new(1.0, 0.816, 0.541);

/// How bright a candle burns, and a lantern that is lit.
const EMBER: f32 = 1.5;
const LIT: f32 = 4.0;

/// Lanterns that throw light at once, the nearest the camera.
const SPILLS: usize = 8;
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
    pub glass: [Material; 5],
    /// The glow each glass was last given.
    shown: [Vec3; 5],
    /// Each light, and how it shines while it does.
    spills: Vec<(Entity, Option<Spill>)>,
}

/// A lantern's light: where it shines, for which state and how
/// strongly.
type Spill = (Vec3, KState, f32);

impl Lamps {
    pub fn make(pal: &Palette) -> Result<Self, String> {
        let glass = |_| Material::make(Self::glass(pal));
        let [a, b, c, d, e] = STATES.map(glass);
        let spills = (0..SPILLS)
            .map(|_| Entity::make(None, SPILL_KEY).map(|e| (e, None)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            glass: [a?, b?, c?, d?, e?],
            shown: [Vec3::NAN; 5],
            spills,
        })
    }

    /// The look of a lantern's glass before it is lit.
    pub fn glass(pal: &Palette) -> Pbr {
        Pbr {
            roughness: 0.4,
            ..Pbr::colored(rgba(pal.glass_lit))
        }
    }

    /// The material of a lantern of `state`.
    pub fn of(&self, state: KState) -> &Material {
        &self.glass[state_index(state)]
    }

    /// Lights each glass as its state shows at clock time `t`. A
    /// lantern at rest is a candle: lit only after dark.
    pub fn light(&mut self, pal: &Palette, sky: &Sky, t: f32) {
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
                self.glass[i].set(Pbr {
                    emissive: rgba(glow),
                    ..Self::glass(pal)
                });
            }
        }
    }

    /// Has the lanterns of `lit`, the nearest first, throw light on
    /// the ground, `lumens` strong and reaching `reach`. None do by
    /// day.
    pub fn spill(
        &mut self,
        lit: &[(Vec3, KState)],
        lumens: f32,
        reach: f32,
    ) {
        for (i, (entity, shown)) in self.spills.iter_mut().enumerate()
        {
            let want = lit
                .get(i)
                .filter(|_| lumens > 0.0)
                .map(|&(at, state)| (at, state, lumens));
            if *shown == want {
                continue;
            }
            *shown = want;
            let light = want.map(|(at, state, _)| {
                entity.set_transform(
                    crate::build::frame::Frame::at(at).scene(),
                );
                let hue = lantern(state, 0.0).0;
                Light::Point(PointLight {
                    color: rgba(hue),
                    intensity: lumens,
                    range: reach,
                    radius: 0.0,
                    shadows: false,
                })
            });
            if let Err(why) = entity.set_light(light) {
                hamlet_engine_sdk::warn(&format!(
                    "a lantern's light: {why}"
                ));
            }
        }
    }
}
