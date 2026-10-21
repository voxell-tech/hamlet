//! The circle of runes and the motes at each end of a Wickstep,
//! drawn by a few entities made once and shown while they play.

use std::f32::consts::{FRAC_PI_4, PI, TAU};

use glam::{Quat, Vec3};
use hamlet_engine_sdk::scene::{Entity, Material, Mesh, Shape};

use crate::chief::ChiefSettings;
use crate::frame::Frame;
use crate::mesh::{Mb, smoothstep};
use crate::stock::glow;

/// A number in 0..1 that follows from `n` and looks random.
pub fn chance(n: f32) -> f32 {
    (n * 12.9898 + 4.1414)
        .sin()
        .mul_add(43_758.547, 0.0)
        .fract()
        .abs()
}

/// A flat band of radius 1 lying on the ground, its inner edge at
/// `inner`, of `sides` straight sides turned by `turn`.
fn band(inner: f32, sides: usize, turn: f32) -> Mb {
    let mut mb = Mb::new();
    let ring = (0..sides)
        .map(|i| {
            let about = Quat::from_rotation_y(
                turn + i as f32 / sides as f32 * TAU,
            );
            (
                mb.v(about * Vec3::X * inner, Vec3::Y, Vec3::ONE),
                mb.v(about * Vec3::X, Vec3::Y, Vec3::ONE),
            )
        })
        .collect::<Vec<_>>();
    for i in 0..sides {
        let (a, b) = (ring[i], ring[(i + 1) % sides]);
        mb.quad(a.0, a.1, b.1, b.0);
    }
    mb
}

/// The two shapes of a circle of runes: a ring, and a diamond in it.
pub fn shapes() -> [Mb; 2] {
    [band(0.86, 48, 0.0), band(0.6, 4, FRAC_PI_4)]
}

/// One shape of a circle of runes as it plays.
struct Rune {
    entity: Entity,
    material: Material,
    /// Its turns over its life, and when it began, and for how long.
    turn: f32,
    playing: Option<RunePlay>,
}

#[derive(Clone, Copy)]
struct RunePlay {
    began: f32,
    long: f32,
    /// True when it opens, as on arriving, or closes.
    opens: bool,
    light: Vec3,
    at: Vec3,
}

/// A mote of a Wickstep.
struct Mote {
    entity: Entity,
    playing: Option<MotePlay>,
}

#[derive(Clone, Copy)]
struct MotePlay {
    began: f32,
    from: Vec3,
    own: f32,
    /// True when it flies out from an arrival, or is drawn up after
    /// a departure.
    bursts: bool,
}

/// The runes and motes of the Wicksteps of one mascot.
pub struct Charm {
    runes: [Rune; 2],
    motes: Vec<Mote>,
    _mote: Mesh,
}

impl Charm {
    /// The runes and as many motes as two ends of a Wickstep take,
    /// none shown, the motes drawn with `flame`.
    pub fn make(
        set: &ChiefSettings,
        flame: &Material,
    ) -> Result<Self, String> {
        let rune = |turn: f32| -> Result<Rune, String> {
            let entity = Entity::make(None, 0)?;
            entity.set_visible(false);
            entity.set_shadows(false, false);
            Ok(Rune {
                entity,
                material: Material::make(glow(Vec3::ZERO, 1.0))?,
                turn,
                playing: None,
            })
        };
        let mote = Mesh::shape(Shape::Sphere(1.0))?;
        let motes = (0..set.motes * 2)
            .map(|_| {
                let entity = Entity::make(None, 0)?;
                entity.set_visible(false);
                entity.set_shadows(false, false);
                entity.draw(&mote, flame);
                Ok(Mote {
                    entity,
                    playing: None,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            runes: [rune(1.2)?, rune(-2.0)?],
            motes,
            _mote: mote,
        })
    }

    /// Gives the runes their shapes, once they are handed over.
    pub fn shape(&self, shapes: &[&Mesh]) {
        for (rune, shape) in self.runes.iter().zip(shapes) {
            rune.entity.draw(shape, &rune.material);
        }
    }

    /// The circle and the motes of one end of a Wickstep, at
    /// `ground`, in the colour `light`.
    pub fn cast(
        &mut self,
        set: &ChiefSettings,
        light: Vec3,
        ground: Vec3,
        now: f32,
        arrives: bool,
    ) {
        let long = if arrives {
            set.in_time * 1.5
        } else {
            set.out_time
        };
        let at = ground + Vec3::Y * set.rune_lift;
        for rune in &mut self.runes {
            rune.playing = Some(RunePlay {
                began: now,
                long,
                opens: arrives,
                light: light * set.rune_light,
                at,
            });
            rune.entity.set_visible(true);
        }
        let free =
            self.motes.iter_mut().filter(|m| m.playing.is_none());
        for (i, mote) in free.take(set.motes as usize).enumerate() {
            let own = i as f32 + chance(now + i as f32);
            let around = chance(own) * TAU;
            let out =
                chance(own + 0.5).sqrt() * set.rune_radius * 0.55;
            let from = ground
                + Vec3::new(around.cos(), 0.0, around.sin()) * out
                + Vec3::Y * (0.15 + chance(own + 0.8));
            mote.playing = Some(MotePlay {
                began: now,
                from,
                own,
                bursts: arrives,
            });
            mote.entity.set_transform(
                Frame::at(from).with_size(Vec3::ZERO).scene(),
            );
            mote.entity.set_visible(true);
        }
    }

    /// True while any of it plays.
    pub fn playing(&self) -> bool {
        self.runes.iter().any(|rune| rune.playing.is_some())
            || self.motes.iter().any(|mote| mote.playing.is_some())
    }

    /// Plays the circles and the motes, and hides each at its end.
    pub fn play(&mut self, set: &ChiefSettings, now: f32) {
        for rune in &mut self.runes {
            let Some(play) = rune.playing else {
                continue;
            };
            let u = (now - play.began) / play.long;
            if u >= 1.0 {
                rune.playing = None;
                rune.entity.set_visible(false);
                continue;
            }
            let u = u.max(0.0);
            let (reach, fade) = if play.opens {
                // It springs open under the feet and fades as they
                // settle.
                let open = 1.0 - (1.0 - (u * 2.6).min(1.0)).powi(3);
                (0.3 + 0.7 * open, (1.0 - u).powf(1.5))
            } else {
                // It closes on the place that is left.
                (
                    1.0 - 0.78 * smoothstep(0.2, 1.0, u),
                    1.0 - smoothstep(0.8, 1.0, u),
                )
            };
            rune.entity.set_transform(
                Frame::at(play.at)
                    .with_turn(Quat::from_rotation_y(u * rune.turn))
                    .with_size(Vec3::splat(
                        (set.rune_radius * reach).max(1e-3),
                    ))
                    .scene(),
            );
            rune.material.set(glow(
                play.light * fade * (u * 8.0).min(1.0),
                1.0,
            ));
        }
        for mote in &mut self.motes {
            let Some(play) = mote.playing else {
                continue;
            };
            // Each sets out a little after the last.
            let wait = chance(play.own + 0.25) * set.mote_time * 0.25;
            let u =
                (now - play.began - wait) / (set.mote_time - wait);
            if u >= 1.0 {
                mote.playing = None;
                mote.entity.set_visible(false);
                continue;
            }
            if u < 0.0 {
                continue;
            }
            let own = chance(play.own + 0.75);
            let around = chance(play.own) * TAU;
            let out = Vec3::new(around.cos(), 0.0, around.sin());
            let eased = 1.0 - (1.0 - u).powi(2);
            let (at, size) = if play.bursts {
                // Out from the feet and up, like sparks off a wick
                // that catches.
                (
                    play.from
                        + out
                            * set.mote_spread
                            * (0.4 + 0.6 * own)
                            * eased
                        + Vec3::Y * set.mote_rise * 0.6 * own * eased,
                    set.mote_size * (u * 8.0).min(1.0) * (1.0 - u),
                )
            } else {
                // Drawn in and up after the flame, like its smoke.
                let sway = Vec3::new(
                    (u * 5.0 + play.own).sin(),
                    0.0,
                    (u * 4.0 + play.own * 1.7).cos(),
                ) * 0.18;
                (
                    play.from - out * 0.3 * u
                        + Vec3::Y
                            * set.mote_rise
                            * (0.5 + 0.5 * own)
                            * u
                            * u
                        + sway * u,
                    set.mote_size
                        * (u * PI).sin().max(0.0).sqrt()
                        * 0.8,
                )
            };
            mote.entity.set_transform(
                Frame::at(at).with_size(Vec3::splat(size)).scene(),
            );
        }
    }
}
