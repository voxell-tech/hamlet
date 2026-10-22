//! The bazaar's lights: its lanterns and the light they throw after
//! dark, what glows among its wares, the fairy lights and the glows
//! over the avenue that take on their colours, and the flare of a
//! sale.

use std::f32::consts::TAU;

use glam::Vec3;
use hamlet_engine_sdk::scene::{
    Entity, Light, Material, Pbr, PointLight,
};

use crate::build::frame::Frame;
use crate::build::stock::rgba;
use crate::places::bazaar::build::FAIRY;
use crate::places::bazaar::{BazaarSettings, Stall, build};
use crate::world::palette::Palette;
use crate::world::tint::to_linear;

/// The glows over the plaza and the promenade, from the bazaar's
/// middle.
const GLOWS: [Vec3; 3] = [
    Vec3::new(-2.6, 3.0, 6.6),
    Vec3::new(2.6, 3.0, 6.6),
    Vec3::new(0.0, 3.0, -9.0),
];

/// Where the flare of a sale burns, from its stall's middle: over
/// its counter.
const FLARE: Vec3 = Vec3::new(0.0, 2.0, 1.6);

/// Brightness of the bazaar `since` seconds after a sale cheered for
/// `cheers` seconds: up at once, flickering while it is cheered, and
/// down again.
pub fn flare(since: f32, cheers: f32) -> f32 {
    if !(0.0..cheers).contains(&since) {
        return 0.0;
    }
    let up = (since / 0.2).min(1.0);
    let down = ((cheers - since) / 1.5).min(1.0);
    up * down * (0.75 + 0.25 * (since * 12.0).sin())
}

/// The share of its glow the bulb of colour `k` has at `t`, of a
/// chase that runs along every string `rounds` times a second: dim
/// between the light passing and bright as it passes.
fn chased(k: usize, t: f32, rounds: f32) -> f32 {
    let wave = 0.5
        + 0.5 * (TAU * (t * rounds - k as f32 / FAIRY as f32)).sin();
    0.3 + 0.7 * wave * wave
}

/// A point light that throws no shadow.
fn point(colour: Vec3, lumens: f32, range: f32) -> Option<Light> {
    (lumens > 0.0).then_some(Light::Point(PointLight {
        color: rgba(colour),
        intensity: lumens,
        range,
        radius: 0.0,
        shadows: false,
    }))
}

/// A light of the bazaar and what it was last given.
struct Lamp {
    entity: Entity,
    shown: Option<(Vec3, f32)>,
}

impl Lamp {
    fn make(parent: &Entity, at: Vec3) -> Result<Self, String> {
        let entity = Entity::make(Some(parent), 0)?;
        entity.set_transform(Frame::at(at).scene());
        Ok(Self {
            entity,
            shown: None,
        })
    }

    /// Lights it in `colour`, `lumens` strong and reaching `range`,
    /// where that differs from what it was.
    fn burn(&mut self, colour: Vec3, lumens: f32, range: f32) {
        let want = (lumens > 0.0).then_some((colour, lumens));
        if self.shown == want {
            return;
        }
        self.shown = want;
        if let Err(why) =
            self.entity.set_light(point(colour, lumens, range))
        {
            hamlet_engine_sdk::warn(&format!(
                "a bazaar light: {why}"
            ));
        }
    }
}

/// The bazaar's materials that glow and its lights.
pub struct Lights {
    pub glass: Material,
    pub gleam: Material,
    pub bulbs: [Material; FAIRY],
    /// The light each of `glass`, `gleam` and `bulbs` was last
    /// given.
    shown: (f32, f32, [f32; FAIRY]),
    lamps: Vec<Lamp>,
    glows: Vec<Lamp>,
    flare: Lamp,
    /// The stall the flare was last set over.
    flared: Option<Stall>,
}

impl Lights {
    /// The lights under `parent`, all out.
    pub fn make(
        parent: &Entity,
        pal: &Palette,
    ) -> Result<Self, String> {
        let bulb = |k: usize| {
            Material::make(Pbr::colored(rgba(pal.bazaar.fairy[k])))
        };
        let [a, b, c, d, e, f] = [0, 1, 2, 3, 4, 5].map(bulb);
        Ok(Self {
            glass: Material::make(Pbr::colored(rgba(to_linear(
                pal.glass_lit,
            ))))?,
            gleam: Material::make(Pbr {
                unlit: true,
                ..Pbr::colored(rgba(Vec3::ONE))
            })?,
            bulbs: [a?, b?, c?, d?, e?, f?],
            shown: (f32::NAN, f32::NAN, [f32::NAN; FAIRY]),
            lamps: Vec::new(),
            glows: GLOWS
                .iter()
                .map(|&at| Lamp::make(parent, at))
                .collect::<Result<_, _>>()?,
            flare: Lamp::make(parent, Vec3::ZERO)?,
            flared: None,
        })
    }

    /// Hangs a light of the lanterns under `parent` at `at`, from the
    /// bazaar's middle.
    pub fn hang(&mut self, parent: &Entity, at: Vec3) {
        match Lamp::make(parent, at) {
            Ok(lamp) => self.lamps.push(lamp),
            Err(why) => hamlet_engine_sdk::warn(&format!(
                "a bazaar lamp: {why}"
            )),
        }
    }

    /// Burns everything as clock time `now` has it, after dark as
    /// `candle` says and brighter after `sale`, a sale at a stall and
    /// when it was.
    pub fn shine(
        &mut self,
        set: &BazaarSettings,
        pal: &Palette,
        (now, candle): (f32, f32),
        sale: Option<(Stall, f32)>,
    ) {
        let level =
            sale.map_or(0.0, |(_, at)| flare(now - at, set.cheers));
        let glass = to_linear(pal.glass_lit);
        let (day, night) = set.glass;
        let bright = day + night * candle + set.flash * level;
        if self.shown.0 != bright {
            self.shown.0 = bright;
            self.glass.set(Pbr {
                emissive: rgba(glass * bright),
                roughness: 0.4,
                ..Pbr::colored(rgba(glass))
            });
        }
        let (day, night) = set.gleam;
        let bright = day + night * candle + level;
        if self.shown.1 != bright {
            self.shown.1 = bright;
            self.gleam.set(Pbr {
                unlit: true,
                ..Pbr::colored(rgba(Vec3::splat(bright)))
            });
        }
        // By day and with no sale the fairy lights stand still.
        let still = candle < 0.05 && level == 0.0;
        let (day, gain, sold) = set.bulb;
        let (rounds, faster) = set.chase;
        let rounds = rounds * (1.0 + faster * level);
        for (k, bulb) in self.bulbs.iter().enumerate() {
            let chase = match still {
                true => 1.0,
                false => chased(k, now, rounds),
            };
            let bright = day + (gain * candle + sold * level) * chase;
            if self.shown.2[k] != bright {
                self.shown.2[k] = bright;
                let colour = pal.bazaar.fairy[k];
                bulb.set(Pbr {
                    emissive: rgba(colour * bright),
                    ..Pbr::colored(rgba(colour))
                });
            }
        }
        for lamp in &mut self.lamps {
            lamp.burn(glass, set.lamp * candle, set.lamp_reach);
        }
        let fairy = pal.bazaar.fairy;
        let lumens = set.glow * (candle + level);
        for (k, glow) in self.glows.iter_mut().enumerate() {
            let turn = now / set.glow_holds
                + (k * FAIRY / GLOWS.len()) as f32;
            let (from, share) = (turn.floor() as usize, turn.fract());
            let colour = fairy[from % FAIRY]
                .lerp(fairy[(from + 1) % FAIRY], share);
            glow.burn(colour, lumens, set.glow_reach);
        }
        if let Some((stall, _)) = sale
            && self.flared != Some(stall)
        {
            self.flared = Some(stall);
            self.flare.entity.set_transform(
                Frame::at(build::at_stall(stall, FLARE)).scene(),
            );
        }
        self.flare.burn(
            pal.ember,
            set.flare * level,
            set.flare_reach,
        );
    }
}
