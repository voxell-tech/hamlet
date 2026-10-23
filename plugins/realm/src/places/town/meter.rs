//! The budget where it can be seen: a light meter on each common,
//! whose light sinks as the hamlet's budget is spent, with a pointer
//! at where it would stand were the budget spent evenly.

use std::f32::consts::TAU;

use glam::{Quat, Vec2, Vec3};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{Material, Pbr};

use crate::bridge::Bridge;
use crate::build::frame::Frame;
use crate::build::mesh::Mb;
use crate::build::stock::{Stock, rgba};
use crate::build::upload::Draw;
use crate::places::town::kit::{Kit, Part};
use crate::places::town::lights::Glow;
use crate::realm::Ctx;
use crate::society::TownId;
use crate::world::land::Land;
use crate::world::palette::Palette;

const WORLD: &str = include_str!("../../../../../assets/world.toml");

/// Number of colours a meter's light passes through as it sinks.
const SHADES: usize = 24;

/// The height above the ground at which the light of a meter begins,
/// and its height when the budget is whole.
const COLUMN: (f32, f32) = (0.42, 2.9);

/// A hamlet's meter on its common.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
pub struct MeterSettings {
    /// Its place on the common, in the town's frame, and its size.
    pub meter_at: Vec2,
    pub meter_size: f32,
    /// Brightness of its light, the light it throws on what is about
    /// it after dark and how far, the blinks a second of a spent
    /// one, and how near the camera comes before what is left is
    /// written over it.
    pub meter_glow: f32,
    pub meter_lumens: f32,
    pub meter_reach: f32,
    pub meter_blink: f32,
    pub meter_read_far: f32,
}

/// The section of `assets/world.toml` the meters read.
#[derive(serde::Deserialize)]
struct WorldPlay {
    play: MeterSettings,
}

/// The meters' settings and the lights they share.
pub struct Meters {
    set: MeterSettings,
    /// The light of a meter by how much is left, in [`SHADES`] steps
    /// from nothing to all of it, and the colour each throws.
    glow: Vec<Handle<Material>>,
    tints: Vec<Vec3>,
}

/// A town's meter as it is drawn.
pub struct Meter {
    town: TownId,
    /// The middle of its light's foot, in the town's frame.
    foot: Vec3,
    level: Entity,
    pace: Entity,
    readout: Entity,
    /// What it shows: the step of its light, its light's height,
    /// whether it is lit, the pointer's height and the words over
    /// it.
    shown: Option<(usize, f32, bool, f32, Option<String>)>,
}

/// The colour of a meter's light with `left` of its budget, from 0
/// to 1: red when empty, through orange and yellow, to green when
/// full. `stops` are those four, evenly apart.
fn shade(stops: &[Vec3; 4], left: f32) -> Vec3 {
    let along = left.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let from = (along as usize).min(stops.len() - 2);
    stops[from].lerp(stops[from + 1], along - from as f32)
}

fn step(left: f32) -> usize {
    (left.clamp(0.0, 1.0) * (SHADES - 1) as f32).round() as usize
}

impl Meters {
    pub fn make(
        pal: &Palette,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        let set = toml::from_str::<WorldPlay>(WORLD)
            .map_err(|why| format!("assets/world.toml: {why}"))?
            .play;
        let tints = (0..SHADES)
            .map(|step| {
                shade(&pal.meter, step as f32 / (SHADES - 1) as f32)
            })
            .collect::<Vec<_>>();
        let glow = tints
            .iter()
            .map(|&light| {
                draw.material(Pbr {
                    emissive: rgba(light * set.meter_glow),
                    roughness: 0.6,
                    ..Pbr::colored(rgba(light))
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { set, glow, tints })
    }

    /// Stands the meter of `town` on its common, `common`.
    pub fn meter(
        &self,
        land: &Land,
        kit: &Kit,
        stock: &Stock,
        town: TownId,
        common: Entity,
        draw: &mut Draw,
    ) -> Meter {
        let (foot, tall) = COLUMN;
        let set = &self.set;
        let at = Vec3::new(
            set.meter_at.x,
            land.set.town.plaza_lift,
            set.meter_at.y,
        );
        let stand = draw.spawn(Some(common), 0);
        draw.place(
            stand,
            Frame::at(at)
                .with_turn(Quat::from_rotation_y(0.5))
                .with_size(Vec3::splat(set.meter_size)),
        );
        let mut part = |part, material: &Handle<Material>, y| {
            let child = draw.spawn(Some(stand), 0);
            draw.place(child, Frame::at(Vec3::Y * y));
            if let Some(mesh) = kit.get(part) {
                draw.draw(child, mesh, material);
            }
            child
        };
        part(Part::Meter, &stock.matte, 0.0);
        let level = part(Part::Level, &self.glow[SHADES - 1], foot);
        let pace = part(Part::Pace, &stock.brass, foot + tall);
        draw.shadows(level, false, true);
        // What is left is written just over its cap.
        let readout = draw.spawn(Some(stand), 0);
        draw.place(readout, Frame::at(Vec3::Y * (foot + tall + 0.8)));
        Meter {
            town,
            foot: at + Vec3::Y * foot * set.meter_size,
            level,
            pace,
            readout,
            shown: None,
        }
    }

    /// Keeps `meter` showing its hamlet's budget: its light as high
    /// as what is left, blinking red once it is all spent, its
    /// pointer where the light would stand were it spent evenly,
    /// and what is left written over it while the camera is near. The
    /// light it throws on a common at `common`, while it is lit.
    pub fn show(
        &self,
        meter: &mut Meter,
        ctx: &Ctx,
        bridge: &Bridge,
        common: &Frame,
        draw: &mut Draw,
    ) -> Option<Glow> {
        let set = &self.set;
        let (foot, tall) = COLUMN;
        let budget = bridge.budget(meter.town);
        // A budget with no cap is whole.
        let left = budget.and_then(|b| b.left).unwrap_or(1.0);
        let gone = budget.map_or(0.0, |b| b.gone);
        // Spent, the whole of it blinks red: an alarm, where a dark
        // meter would say nothing.
        let empty = left <= 0.0;
        let lit =
            !empty || (ctx.clock * set.meter_blink).fract() < 0.5;
        // The light sinks as the budget is spent.
        let height = if empty { 1.0 } else { left.max(0.02) };
        let top = common.point(
            meter.foot + Vec3::Y * (tall + 0.8) * set.meter_size,
        );
        let words = budget
            .and_then(|b| {
                Some(format!(
                    "{} LEFT {}",
                    b.remains.as_ref()?,
                    b.period.to_uppercase()
                ))
            })
            .filter(|_| ctx.eye.distance(top) < set.meter_read_far);
        let want = (
            step(left),
            height,
            lit,
            foot + tall * (1.0 - gone),
            words,
        );
        if meter.shown.as_ref() != Some(&want) {
            let (shade, height, lit, pace, words) = &want;
            draw.commands.entity(meter.level).try_insert(
                MeshMaterial3d(self.glow[*shade].clone()),
            );
            draw.place(
                meter.level,
                Frame::at(Vec3::Y * foot)
                    .with_size(Vec3::new(1.0, *height, 1.0)),
            );
            draw.show(meter.level, *lit);
            draw.place(meter.pace, Frame::at(Vec3::Y * *pace));
            match words {
                Some(words) => {
                    draw.commands
                        .entity(meter.readout)
                        .try_insert(SceneLabel(words.clone()));
                }
                None => {
                    draw.commands
                        .entity(meter.readout)
                        .try_remove::<SceneLabel>();
                }
            }
            meter.shown = Some(want);
        }
        // It throws its colour on the common after dark, and nothing
        // while it is blinked out.
        lit.then(|| Glow {
            at: common.point(meter.foot),
            colour: self.tints[step(left)],
            lumens: set.meter_lumens * ctx.sky.look.candle,
            reach: set.meter_reach,
        })
    }
}

/// A round bar from `low` to `high`, `radius` wide.
fn drum(
    mb: &mut Mb,
    low: f32,
    high: f32,
    radius: f32,
    sides: usize,
    colour: Vec3,
) {
    mb.lathe(
        Vec3::Y * low,
        &[
            (0.0, 0.0),
            (radius, 0.0),
            (radius, high - low),
            (0.0, high - low),
        ],
        sides,
        |_, _| colour,
    );
}

/// The stand of a meter, round so that it reads from every side: a
/// stone foot, a dark core the light stands about, three brass rods
/// with a ring at every tenth, and a cap.
pub fn stand(pal: &Palette) -> Mb {
    let mut mb = Mb::new();
    let (foot, tall) = COLUMN;
    let top = foot + tall;
    drum(&mut mb, 0.0, foot * 0.55, 0.62, 12, pal.limestone_shade);
    drum(&mut mb, foot * 0.55, foot, 0.48, 12, pal.limestone);
    drum(&mut mb, foot, top, 0.12, 8, pal.timber_dark * 0.45);
    for rod in 0..3 {
        let around = rod as f32 * TAU / 3.0 + 0.5;
        mb.rbox(
            Vec3::new(
                around.cos() * 0.4,
                foot + tall * 0.5,
                around.sin() * 0.4,
            ),
            Vec3::new(0.028, tall * 0.5, 0.028),
            0.01,
            Quat::from_rotation_y(-around),
            pal.brass,
            0.1,
        );
    }
    // A ring for each tenth, the middle one heavier.
    for tenth in 1..10 {
        let thick = if tenth == 5 { 0.03 } else { 0.014 };
        let at = foot + tall * tenth as f32 / 10.0;
        mb.lathe(
            Vec3::Y * (at - thick),
            &[
                (0.37, 0.0),
                (0.43, 0.0),
                (0.43, thick * 2.0),
                (0.37, thick * 2.0),
                (0.37, 0.0),
            ],
            12,
            |_, _| pal.brass * 1.1,
        );
    }
    drum(&mut mb, top, top + 0.12, 0.5, 12, pal.brass);
    mb.lathe(
        Vec3::Y * (top + 0.12),
        &[(0.0, 0.0), (0.3, 0.0), (0.12, 0.22), (0.0, 0.3)],
        10,
        |_, _| pal.brass * 1.1,
    );
    mb
}

/// The light of a meter at its fullest, standing on the ground it is
/// scaled from.
pub fn level() -> Mb {
    let mut mb = Mb::new();
    drum(&mut mb, 0.0, COLUMN.1, 0.3, 14, Vec3::ONE);
    mb
}

/// The pointer that shows where the light would stand were the budget
/// spent evenly: a heavy brass ring about the rods.
pub fn pace(pal: &Palette) -> Mb {
    let mut mb = Mb::new();
    mb.lathe(
        Vec3::Y * -0.05,
        &[
            (0.44, 0.0),
            (0.56, 0.0),
            (0.56, 0.1),
            (0.44, 0.1),
            (0.44, 0.0),
        ],
        14,
        |_, _| pal.brass * 1.35,
    );
    mb
}
