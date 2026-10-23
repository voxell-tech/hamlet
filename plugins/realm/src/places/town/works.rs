//! A hamlet's work where it can be seen: a workbench in the middle of
//! each common for its folder, and clockwork on the bench for its
//! mode, which a click switches.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use glam::{Quat, Vec3};
use hamlet_engine_bevy::api::agents::{AgentsRequest, Selected};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    AlphaMode, Material, StandardMaterial,
};
use hamlet_engine_bevy::ui::open;

use crate::bridge::{Bridge, Mode};
use crate::build::frame::Frame;
use crate::build::mesh::Mb;
use crate::build::stock::{Stock, rgba};
use crate::build::upload::Draw;
use crate::places::lantern::lantern_frame;
use crate::places::town::kit::{Kit, Part};
use crate::places::town::lights::{CURRENT, Glow, SIGNAL};
use crate::society::TownId;
use crate::view::pick::Thing;
use crate::world::land::Land;
use crate::world::palette::Palette;
use crate::world::tint::to_linear;

const WORLD: &str = include_str!("../../../../../assets/world.toml");

/// Height of the bench's top above the paving.
const TOP: f32 = 0.91;

/// The middle of the lamp's glass, in the bench's frame.
const LAMP: Vec3 = Vec3::new(-0.85, 1.8, 0.36);

/// Wheels of the clockwork.
pub const COGS: usize = 3;

/// The wheels of the clockwork, the largest first: the hub of each in
/// the bench's frame, its radius and the number of its teeth.
const WHEELS: [(Vec3, f32, usize); COGS] = [
    (Vec3::new(0.36, 1.2, -0.15), 0.19, 16),
    (Vec3::new(0.677, 1.348, -0.15), 0.12, 10),
    (Vec3::new(0.614, 1.022, -0.15), 0.08, 7),
];

/// The key of a town's clockwork: a kind of its own past the realm's
/// things, with the town's number.
const CLOCKWORK: u32 = 15 << 28;
const TOWN: u32 = (1 << 28) - 1;

/// The Inspector, brought forward when a hamlet is chosen here.
const INSPECTOR: &str = "hamlet.inspector/inspector";

/// The workbench of a common.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorksSettings {
    pub size: f32,
    /// Radians a second the clockwork's largest wheel turns in auto.
    pub spin: f32,
    /// Brightness of the lamp, the light it throws after dark, and
    /// how far.
    pub lamp_glow: f32,
    pub lumens: f32,
    pub reach: f32,
}

/// The section of `assets/world.toml` the workbenches read.
#[derive(serde::Deserialize)]
struct WorldWorks {
    works: WorksSettings,
}

/// The workbenches' settings and the materials they share.
pub struct Benches {
    set: WorksSettings,
    /// The lamp's glass with no folder, and in each mode, manual
    /// first.
    dark: Handle<Material>,
    lamps: [Handle<Material>; 2],
    /// What is hit but not seen.
    unseen: Handle<Material>,
    /// The clock time of the frame before.
    last: Option<f32>,
}

/// A town's workbench as it is drawn.
pub struct Bench {
    town: TownId,
    /// Where it stands on its common.
    at: Vec3,
    laid: Entity,
    beacon: Entity,
    wheels: [Entity; COGS],
    /// Radians the largest wheel has turned.
    turned: f32,
    /// The mode shown, once it is shown.
    shown: Option<Option<Mode>>,
}

/// The colour a mode is shown in: the orange of what waits for the
/// chief, and the blue of what runs.
fn hue(mode: Mode) -> Vec3 {
    to_linear(match mode {
        Mode::Manual => SIGNAL,
        Mode::Auto => CURRENT,
    })
}

fn index(mode: Mode) -> usize {
    match mode {
        Mode::Manual => 0,
        Mode::Auto => 1,
    }
}

/// Each wheel's turns for one of the largest. Each is turned by the
/// largest, the other way round.
fn ratio(k: usize) -> f32 {
    if k == 0 {
        1.0
    } else {
        -(WHEELS[0].2 as f32) / WHEELS[k].2 as f32
    }
}

/// The place of wheel `k` once the largest has turned `turned`
/// radians. It stands on its edge, with its face to the bench's
/// front.
fn turn(k: usize, turned: f32) -> Frame {
    Frame::at(WHEELS[k].0).with_turn(
        Quat::from_rotation_x(FRAC_PI_2)
            * Quat::from_rotation_y(turned * ratio(k)),
    )
}

impl Benches {
    pub fn make(
        pal: &Palette,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        let set = toml::from_str::<WorldWorks>(WORLD)
            .map_err(|why| format!("assets/world.toml: {why}"))?
            .works;
        let lamp = |mode: Mode| {
            let glow = hue(mode);
            StandardMaterial {
                emissive: rgba(glow * set.lamp_glow),
                perceptual_roughness: 0.4,
                ..StandardMaterial::colored(rgba(glow))
            }
        };
        Ok(Self {
            dark: draw.material(StandardMaterial {
                perceptual_roughness: 0.4,
                ..StandardMaterial::colored(rgba(pal.glass_dark))
            })?,
            lamps: [
                draw.material(lamp(Mode::Manual))?,
                draw.material(lamp(Mode::Auto))?,
            ],
            unseen: draw.material(StandardMaterial {
                base_color: (0.0, 0.0, 0.0, 0.0),
                alpha_mode: AlphaMode::Mask,
                unlit: true,
                fog_enabled: false,
                ..StandardMaterial::colored(rgba(Vec3::ZERO))
            })?,
            set,
            last: None,
        })
    }

    /// Stands the workbench of `town` on its common, `common`.
    pub fn bench(
        &self,
        land: &Land,
        kit: &Kit,
        stock: &Stock,
        town: TownId,
        common: Entity,
        draw: &mut Draw,
    ) -> Bench {
        let set = &land.set.town;
        let at =
            Vec3::new(set.bench.0.x, set.plaza_lift, set.bench.0.y);
        let entity = draw.spawn(Some(common), 0);
        draw.place(
            entity,
            Frame::at(at).with_size(Vec3::splat(self.set.size)),
        );
        let mut part =
            |part: Part, material: &Handle<Material>, key: u32| {
                let child = draw.spawn(Some(entity), key);
                if let Some(mesh) = kit.get(part) {
                    draw.draw(child, mesh, material);
                }
                child
            };
        part(Part::Bench, &stock.matte, 0);
        let laid = part(Part::Work, &stock.matte, 0);
        let beacon = part(Part::BenchLamp, &self.dark, 0);
        let wheels =
            [0, 1, 2].map(|k| part(Part::Cog(k), &stock.brass, 0));
        // The top answers as the common, and the clockwork as itself.
        let top =
            part(Part::Box, &self.unseen, Thing::Common(town).key());
        let clockwork = part(
            Part::Box,
            &self.unseen,
            CLOCKWORK | (town.0 & TOWN),
        );
        draw.place(
            top,
            Frame::at(Vec3::Y * (TOP + 0.1) * 0.5)
                .with_size(Vec3::new(2.1, TOP + 0.1, 1.1)),
        );
        draw.place(
            clockwork,
            Frame::at(Vec3::new(0.52, TOP + 0.36, -0.1))
                .with_size(Vec3::new(0.95, 0.7, 0.45)),
        );
        for boxed in [top, clockwork] {
            draw.watch(boxed);
            draw.shadows(boxed, false, false);
        }
        draw.show(laid, false);
        draw.place(beacon, Frame::at(LAMP));
        draw.shadows(beacon, false, true);
        for (k, wheel) in wheels.into_iter().enumerate() {
            draw.place(wheel, turn(k, 0.0));
        }
        Bench {
            town,
            at,
            laid,
            beacon,
            wheels,
            turned: 0.0,
            shown: None,
        }
    }

    /// Readies a frame at clock time `now`: the seconds since the
    /// last.
    pub fn tick(&mut self, now: f32) -> f32 {
        let dt = self.last.map_or(0.0, |last| (now - last).max(0.0));
        self.last = Some(now);
        dt
    }

    /// Keeps `bench` as its hamlet works now, `dt` seconds on: its
    /// work laid out, its lamp in the mode's colour, and its
    /// clockwork turning in auto.
    pub fn show(
        &self,
        bench: &mut Bench,
        bridge: &Bridge,
        dt: f32,
        draw: &mut Draw,
    ) {
        let mode = bridge.mode(bench.town);
        if bench.shown != Some(mode) {
            bench.shown = Some(mode);
            let glass = match mode {
                Some(mode) => &self.lamps[index(mode)],
                None => &self.dark,
            };
            draw.commands
                .entity(bench.beacon)
                .try_insert(MeshMaterial3d(glass.clone()));
            draw.show(bench.laid, mode.is_some());
        }
        let step = self.set.spin * dt;
        if mode == Some(Mode::Auto) && step != 0.0 {
            bench.turned += step;
            for (k, wheel) in bench.wheels.into_iter().enumerate() {
                draw.place(wheel, turn(k, bench.turned));
            }
        }
    }

    /// The light the lamp of `bench` throws after dark, `candle`
    /// strong, on a common at `common`.
    pub fn glow(
        &self,
        bench: &Bench,
        common: &Frame,
        candle: f32,
    ) -> Option<Glow> {
        let mode = bench.shown.flatten()?;
        Some(Glow {
            at: common.point(bench.at + LAMP * self.set.size),
            colour: hue(mode),
            lumens: self.set.lumens * candle,
            reach: self.set.reach,
        })
    }
}

/// Switches the mode of the hamlet whose clockwork has `key`. One
/// that works on no folder is chosen instead, so that one can be
/// chosen for it. True when the mode was switched.
pub fn switch(
    key: u32,
    bridge: &Bridge,
    commands: &mut Commands,
) -> bool {
    if key & !TOWN != CLOCKWORK {
        return false;
    }
    let town = TownId(key & TOWN);
    let Some(hamlet) = bridge.hamlet(town) else {
        return false;
    };
    let hamlet = hamlet.to_owned();
    match bridge.mode(town) {
        None => {
            commands.agents(&AgentsRequest::Select(
                Selected::Hamlet(hamlet),
            ));
            open(INSPECTOR);
            false
        }
        Some(mode) => {
            let to = match mode {
                Mode::Manual => Mode::Auto,
                Mode::Auto => Mode::Manual,
            };
            commands.agents(&AgentsRequest::Decree {
                hamlet,
                text: format!("/mode {}", to.name()),
            });
            true
        }
    }
}

/// The bench: a plank top on four legs with a shelf under it, a
/// lantern on a post at one corner, and the plate the clockwork is
/// mounted on at the other end.
pub fn bench(pal: &Palette) -> Mb {
    let mut mb = Mb::new();
    mb.rbox(
        Vec3::Y * (TOP - 0.045),
        Vec3::new(1.0, 0.045, 0.48),
        0.015,
        Quat::IDENTITY,
        pal.log,
        0.1,
    );
    for (x, z) in
        [(-1.0f32, -1.0f32), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)]
    {
        mb.rbox(
            Vec3::new(x * 0.88, (TOP - 0.09) * 0.5, z * 0.38),
            Vec3::new(0.06, (TOP - 0.09) * 0.5, 0.06),
            0.015,
            Quat::IDENTITY,
            pal.timber_dark,
            0.3,
        );
    }
    mb.rbox(
        Vec3::Y * 0.26,
        Vec3::new(0.86, 0.025, 0.36),
        0.01,
        Quat::IDENTITY,
        pal.timber,
        0.2,
    );
    let foot = Vec3::new(LAMP.x, TOP, LAMP.z);
    mb.tube(
        foot,
        LAMP - Vec3::Y * 0.15,
        0.03,
        0.022,
        6,
        pal.bed,
        false,
    );
    lantern_frame(&mut mb, LAMP, 0.1, 0.24, pal.brass);
    mb.rbox(
        Vec3::new(0.52, TOP + 0.3, -0.22),
        Vec3::new(0.44, 0.3, 0.025),
        0.012,
        Quat::IDENTITY,
        pal.timber_dark,
        0.1,
    );
    for (hub, ..) in WHEELS {
        mb.tube(
            Vec3::new(hub.x, hub.y, -0.2),
            Vec3::new(hub.x, hub.y, hub.z - 0.03),
            0.025,
            0.025,
            6,
            pal.bed,
            false,
        );
    }
    mb
}

/// The work a hamlet with a folder has on its bench: a drawing
/// unrolled, a mallet, an inkpot with its quill, and two books.
pub fn work(pal: &Palette) -> Mb {
    let mut mb = Mb::new();
    let turn = Quat::from_rotation_y(0.12);
    let sheet = Vec3::new(-0.35, TOP + 0.008, 0.08);
    mb.rbox(
        sheet,
        Vec3::new(0.36, 0.006, 0.27),
        0.003,
        turn,
        pal.parchment,
        0.0,
    );
    for line in 0..4 {
        let along = 0.3 - line as f32 * 0.07;
        let down = -0.2 + line as f32 * 0.12;
        mb.rbox(
            sheet + turn * Vec3::new(-0.04, 0.008, down),
            Vec3::new(along, 0.002, 0.008),
            0.001,
            turn,
            pal.bed,
            0.0,
        );
    }
    mb.tube(
        sheet + turn * Vec3::new(-0.38, 0.03, -0.28),
        sheet + turn * Vec3::new(-0.38, 0.03, 0.28),
        0.035,
        0.035,
        8,
        pal.parchment * 0.88,
        true,
    );
    mb.tube(
        Vec3::new(-0.1, TOP + 0.025, 0.4),
        Vec3::new(0.14, TOP + 0.025, 0.33),
        0.02,
        0.02,
        6,
        pal.timber,
        true,
    );
    mb.rbox(
        Vec3::new(0.16, TOP + 0.045, 0.325),
        Vec3::new(0.045, 0.045, 0.085),
        0.012,
        Quat::from_rotation_y(0.28),
        pal.timber_dark,
        0.0,
    );
    let pot = Vec3::new(0.05, TOP, -0.3);
    mb.lathe(
        pot,
        &[
            (0.0, 0.0),
            (0.05, 0.0),
            (0.06, 0.05),
            (0.035, 0.08),
            (0.0, 0.08),
        ],
        8,
        |_, _| pal.bed,
    );
    mb.tube(
        pot + Vec3::Y * 0.06,
        pot + Vec3::new(0.09, 0.3, -0.05),
        0.006,
        0.018,
        5,
        pal.whitewash,
        true,
    );
    for (lift, half, colour, yaw) in [
        (0.03, Vec3::new(0.14, 0.03, 0.1), pal.terracotta, 0.0),
        (0.085, Vec3::new(0.12, 0.025, 0.085), pal.ochre, 0.25),
    ] {
        mb.rbox(
            Vec3::new(-0.45, TOP + lift, -0.3),
            half,
            0.008,
            Quat::from_rotation_y(yaw),
            colour,
            0.0,
        );
    }
    mb
}

/// Wheel `k` of the clockwork about its hub, lying flat: its teeth,
/// and three dark holes in its face so its turning shows. The largest
/// has the handle it is turned by hand with.
pub fn cog(pal: &Palette, k: usize) -> Mb {
    let (_, radius, teeth) = WHEELS[k.min(COGS - 1)];
    let mut mb = Mb::new();
    let thick = 0.03;
    let disc =
        |mb: &mut Mb, at: Vec3, r: f32, tall: f32, col: Vec3| {
            mb.lathe(
                at,
                &[(0.0, 0.0), (r, 0.0), (r, tall), (0.0, tall)],
                teeth * 2,
                |_, _| col,
            );
        };
    disc(&mut mb, Vec3::Y * -thick, radius, thick * 2.0, pal.brass);
    for tooth in 0..teeth {
        let around = tooth as f32 / teeth as f32 * TAU;
        mb.rbox(
            Vec3::new(around.cos(), 0.0, around.sin())
                * (radius + 0.012),
            Vec3::new(
                0.03,
                thick * 0.9,
                radius * PI / teeth as f32 * 0.5,
            ),
            0.005,
            Quat::from_rotation_y(-around),
            pal.brass * 1.15,
            0.0,
        );
    }
    for hole in 0..3 {
        let around = hole as f32 / 3.0 * TAU + 0.5;
        disc(
            &mut mb,
            Vec3::new(around.cos(), 0.0, around.sin()) * radius * 0.6
                + Vec3::Y * thick,
            radius * 0.18,
            0.003,
            pal.bed,
        );
    }
    disc(
        &mut mb,
        Vec3::Y * thick,
        radius * 0.28,
        0.02,
        pal.brass * 1.3,
    );
    if k == 0 {
        let out = radius * 0.8;
        mb.rbox(
            Vec3::new(out * 0.5, thick + 0.035, 0.0),
            Vec3::new(out * 0.5 + 0.02, 0.012, 0.022),
            0.006,
            Quat::IDENTITY,
            pal.bed,
            0.0,
        );
        mb.tube(
            Vec3::new(out, thick + 0.04, 0.0),
            Vec3::new(out, thick + 0.2, 0.0),
            0.028,
            0.028,
            8,
            pal.timber,
            true,
        );
    }
    mb
}
