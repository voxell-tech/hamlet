//! Houses: one mesh per agent, put together from a few parts chosen
//! by the agent's id, so each agent's house is its own and always the
//! same.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::math::Affine3A;
use bevy::prelude::*;

use super::land::Land;
use super::noise::key;
use super::society::AgentId;
use super::yard::{self, Yard};
use crate::lantern::lantern_cage;
use crate::meshes::{Meshing, Rng, noise2};
use crate::palette::Palette;
pub use crate::palette::TownStyle;

/// The remains of an archived agent's house.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Deserialize)]
pub enum Remains {
    /// The base, broken walls and moss.
    Ruin,
    /// Nothing but the ground.
    Bare,
}

/// The build of a house.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HouseSettings {
    /// Height of the stone base.
    pub base: f32,
    /// Overhang of the roof past the walls.
    pub eave: f32,
    /// The remains of a removed agent's house.
    pub removed: Remains,
    /// The remains of a house merged into another.
    pub merged: Remains,
}

impl TownStyle {
    /// The style of the town on a site. Neighbouring sites differ.
    pub fn of(land: &Land, site: usize) -> Self {
        let towns = &land.pal.towns;
        let shift =
            (key(&[land.seed, 40]) % towns.len() as u32) as usize;
        towns[(site + shift) % towns.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Roof {
    /// Ridge across the front.
    Gable,
    /// Ridge front to back, so the gable faces the road.
    Fronted,
    Hip,
    /// A long slope at the front and a short one behind.
    Saltbox,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wall {
    Plaster,
    Ochre,
    Whitewash,
    Stone,
    Framed,
}

/// The traits that make one house unlike the next.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    half: Vec2,
    storeys: u32,
    roof: Roof,
    wall: Wall,
    livery: Vec3,
    pitch: f32,
    /// -1 or 1: the side the chimney and the garden are on.
    side: f32,
    garden: u32,
    /// Height of the stone base.
    base: f32,
    /// Overhang of the roof past the walls.
    eave: f32,
}

impl Look {
    pub fn of(land: &Land, agent: AgentId, hall: bool) -> Self {
        let pick = |salt: u32, n: u32| {
            (key(&[land.seed, 41, agent.0, salt]) % n) as usize
        };
        let livery = land.pal.livery[pick(3, 8)];
        let (base, eave) = (land.set.house.base, land.set.house.eave);
        if hall {
            return Self {
                half: Vec2::new(2.0, 1.4),
                storeys: 2,
                roof: Roof::Gable,
                wall: Wall::Stone,
                livery,
                pitch: 1.15,
                side: 1.0,
                garden: 4,
                base,
                eave,
            };
        }
        Self {
            half: Vec2::new(
                1.25 + 0.14 * pick(0, 4) as f32,
                1.05 + 0.13 * pick(1, 3) as f32,
            ),
            storeys: 1 + (pick(2, 5) / 3) as u32,
            roof: [
                Roof::Gable,
                Roof::Fronted,
                Roof::Hip,
                Roof::Saltbox,
            ][pick(4, 4)],
            wall: [
                Wall::Plaster,
                Wall::Ochre,
                Wall::Whitewash,
                Wall::Stone,
                Wall::Framed,
            ][pick(5, 5)],
            livery,
            pitch: 0.95 + 0.15 * pick(6, 3) as f32,
            side: if pick(7, 2) == 0 { -1.0 } else { 1.0 },
            garden: pick(8, 4) as u32,
            base,
            eave,
        }
    }

    fn wall_top(&self) -> f32 {
        self.base + 1.45 + 1.15 * (self.storeys - 1) as f32
    }

    fn wall_colour(&self, pal: &Palette) -> Vec3 {
        match self.wall {
            Wall::Plaster | Wall::Framed => pal.plaster,
            Wall::Ochre => pal.ochre,
            Wall::Whitewash => pal.whitewash,
            Wall::Stone => pal.limestone,
        }
    }
}

/// The work yard of an agent's house, whose door is turned `yaw`
/// from +Z.
pub fn yard_of(
    land: &Land,
    agent: AgentId,
    hall: bool,
    yaw: f32,
    wings: u8,
) -> Yard {
    Yard::new(
        Look::of(land, agent, hall).half,
        yaw,
        hall,
        wings,
        land.set.yard,
    )
}

/// The tree in a house's garden, if it has one: where it stands
/// about the house, which kind and how big. It keeps to the side
/// away from the yard.
pub(crate) fn garden_tree(
    land: &Land,
    agent: AgentId,
    yard: &Yard,
) -> Option<(Vec3, usize, f32)> {
    let look = Look::of(land, agent, false);
    let h = key(&[land.seed, 42, agent.0]);
    (h % 10 < 7).then(|| {
        (
            Vec3::new(
                -yard.side * (look.half.x + 0.25),
                0.0,
                -look.half.y - 0.55,
            ),
            (h / 16 % 5) as usize,
            0.55 + 0.3 * ((h / 256 % 100) as f32 / 100.0),
        )
    })
}

/// A house's meshes, about the middle of its plot with the door
/// toward +Z.
pub struct HouseMesh {
    pub body: Meshing,
    /// The window panes, which light at night.
    pub panes: Meshing,
    /// The place the lantern by the door hangs at.
    pub lamp: Vec3,
    /// The point the mayor's banner hangs from, and the colours of
    /// its stripes in turn.
    pub banner: Option<(Vec3, [Vec3; 2])>,
}

/// One slope of tiles from an eave to a ridge, laid in lapped
/// courses with each tile its own colour.
pub fn slope(
    meshing: &mut Meshing,
    rng: &mut Rng,
    eave: [Vec3; 2],
    ridge: [Vec3; 2],
    style: &TownStyle,
    pal: &Palette,
) {
    let (rows, cols) = (8, 9);
    let run = (ridge[0] + ridge[1] - eave[0] - eave[1]) * 0.5;
    let n = (eave[1] - eave[0]).cross(run).normalize();
    let n = if n.y < 0.0 { -n } else { n };
    let down = -run.normalize();
    let at = |u: f32, v: f32| {
        let sag = 0.07 * (u * PI).sin() * v;
        eave[0].lerp(eave[1], u).lerp(ridge[0].lerp(ridge[1], u), v)
            - Vec3::Y * sag
    };
    // The underside, so an eave seen from behind is not a hole.
    meshing.face(
        [
            eave[0] - n * 0.03,
            eave[1] - n * 0.03,
            ridge[1] - n * 0.03,
            ridge[0] - n * 0.03,
        ],
        -n,
        pal.timber_dark * 0.8,
    );
    let mossy = rng.f() * 40.0;
    for r in 0..rows {
        let (v0, v1) = (
            r as f32 / rows as f32,
            ((r as f32 + 1.3) / rows as f32).min(1.0),
        );
        let shift = if r % 2 == 0 { 0.0 } else { 0.5 };
        for c in 0..cols + 1 {
            let u0 = ((c as f32 - shift) / cols as f32).max(0.0);
            let u1 =
                ((c as f32 + 1.0 - shift) / cols as f32).min(1.0);
            if u1 - u0 < 1e-3 {
                continue;
            }
            let mut col = style.roof.lerp(style.drift, rng.f() * 0.8)
                * rng.value(0.1);
            if noise2(u0 * 3.0 + mossy, v0 * 3.0) > 0.68 {
                col = col.lerp(pal.moss, rng.range(0.35, 0.75));
            }
            let lip = n * 0.05 + down * rng.range(0.0, 0.05);
            meshing.face_cols(
                [
                    at(u0, v0) + lip,
                    at(u1, v0) + lip,
                    at(u1, v1),
                    at(u0, v1),
                ],
                n,
                [col * 0.7, col * 0.7, col, col],
            );
        }
    }
}

fn roof(
    meshing: &mut Meshing,
    rng: &mut Rng,
    look: &Look,
    style: &TownStyle,
    pal: &Palette,
    half: Vec2,
    top: f32,
    wall: Vec3,
) {
    let (ex, ez) = (half.x + look.eave, half.y + look.eave);
    let y = top - 0.08;
    let p = |x: f32, h: f32, z: f32| Vec3::new(x, y + h, z);
    let ridge_cap =
        |meshing: &mut Meshing, rng: &mut Rng, a: Vec3, b: Vec3| {
            let d = b - a;
            let rot = Quat::from_rotation_arc(Vec3::X, d.normalize());
            meshing.rbox(
                (a + b) * 0.5 + Vec3::Y * 0.03,
                Vec3::new(d.length() * 0.5 + 0.06, 0.06, 0.1),
                0.04,
                rot,
                style.roof * 0.85 * rng.value(0.06),
                0.0,
            );
        };
    match look.roof {
        Roof::Gable | Roof::Saltbox => {
            let back = if look.roof == Roof::Saltbox {
                0.45
            } else {
                0.0
            };
            let rz = -ez * back;
            let rise = look.pitch * (ez + rz.abs());
            let drop = if back > 0.0 { rise * 0.3 } else { 0.0 };
            slope(
                meshing,
                rng,
                [p(-ex, 0.0, ez), p(ex, 0.0, ez)],
                [p(-ex, rise, rz), p(ex, rise, rz)],
                style,
                pal,
            );
            slope(
                meshing,
                rng,
                [p(ex, drop, -ez), p(-ex, drop, -ez)],
                [p(ex, rise, rz), p(-ex, rise, rz)],
                style,
                pal,
            );
            ridge_cap(
                meshing,
                rng,
                p(-ex, rise, rz),
                p(ex, rise, rz),
            );
            for side in [-1.0f32, 1.0] {
                let x = side * (half.x - 0.03);
                let apex = Vec3::new(x, y + rise - 0.06, rz);
                meshing.face(
                    [
                        Vec3::new(x, top - 0.05, -half.y),
                        Vec3::new(x, top - 0.05, half.y),
                        apex,
                        apex,
                    ],
                    Vec3::X * side,
                    wall * 0.97,
                );
            }
        }
        Roof::Fronted => {
            let rise = look.pitch * ex;
            slope(
                meshing,
                rng,
                [p(ex, 0.0, ez), p(ex, 0.0, -ez)],
                [p(0.0, rise, ez), p(0.0, rise, -ez)],
                style,
                pal,
            );
            slope(
                meshing,
                rng,
                [p(-ex, 0.0, -ez), p(-ex, 0.0, ez)],
                [p(0.0, rise, -ez), p(0.0, rise, ez)],
                style,
                pal,
            );
            ridge_cap(
                meshing,
                rng,
                p(0.0, rise, -ez),
                p(0.0, rise, ez),
            );
            for side in [-1.0f32, 1.0] {
                let z = side * (half.y - 0.03);
                let apex = Vec3::new(0.0, y + rise - 0.06, z);
                meshing.face(
                    [
                        Vec3::new(-half.x, top - 0.05, z),
                        Vec3::new(half.x, top - 0.05, z),
                        apex,
                        apex,
                    ],
                    Vec3::Z * side,
                    wall * 0.97,
                );
            }
        }
        Roof::Hip => {
            let rise = look.pitch * ez * 1.1;
            let rl = (ex - ez).max(0.0) + 0.15;
            let (a, b) = (p(-rl, rise, 0.0), p(rl, rise, 0.0));
            slope(
                meshing,
                rng,
                [p(-ex, 0.0, ez), p(ex, 0.0, ez)],
                [a, b],
                style,
                pal,
            );
            slope(
                meshing,
                rng,
                [p(ex, 0.0, -ez), p(-ex, 0.0, -ez)],
                [b, a],
                style,
                pal,
            );
            slope(
                meshing,
                rng,
                [p(ex, 0.0, ez), p(ex, 0.0, -ez)],
                [b, b],
                style,
                pal,
            );
            slope(
                meshing,
                rng,
                [p(-ex, 0.0, -ez), p(-ex, 0.0, ez)],
                [a, a],
                style,
                pal,
            );
            ridge_cap(meshing, rng, a, b);
        }
    }
}

fn window(
    body: &mut Meshing,
    panes: &mut Meshing,
    pal: &Palette,
    c: Vec3,
    face: Quat,
    livery: Vec3,
    rng: &mut Rng,
) {
    let (w, h) = (0.2, 0.27);
    let at = |x: f32, y: f32, z: f32| c + face * Vec3::new(x, y, z);
    let n = face * Vec3::Z;
    body.rbox(
        at(0.0, 0.0, 0.0),
        Vec3::new(w + 0.045, h + 0.045, 0.03),
        0.015,
        face,
        pal.timber_dark,
        0.0,
    );
    panes.face(
        [
            at(-w, -h, 0.034),
            at(w, -h, 0.034),
            at(w, h, 0.034),
            at(-w, h, 0.034),
        ],
        n,
        Vec3::ONE,
    );
    for (hw, hh) in [(0.014, h), (w, 0.014)] {
        body.face(
            [
                at(-hw, -hh, 0.04),
                at(hw, -hh, 0.04),
                at(hw, hh, 0.04),
                at(-hw, hh, 0.04),
            ],
            n,
            pal.timber_dark,
        );
    }
    for side in [-1.0f32, 1.0] {
        body.rbox(
            at(side * (w + 0.15), 0.0, 0.035),
            Vec3::new(0.1, h + 0.02, 0.018),
            0.012,
            face * Quat::from_rotation_y(side * -0.22),
            livery * rng.value(0.07),
            0.1,
        );
    }
}

fn bush(
    meshing: &mut Meshing,
    rng: &mut Rng,
    pal: &Palette,
    c: Vec3,
    r: f32,
) {
    let (light, shade) = (pal.foliage, pal.foliage_shade);
    for _ in 0..2 {
        let o =
            Vec3::new(rng.sym(r * 0.5), r * 0.4, rng.sym(r * 0.5));
        let rr = r * rng.range(0.65, 0.95);
        let tint = rng.value(0.08);
        meshing.blob(
            c + o,
            Vec3::new(rr, rr * 0.8, rr),
            4,
            0.25,
            rng.f() * 30.0,
            move |d| {
                shade.lerp(light, (d.y * 0.6 + 0.5).clamp(0.0, 1.0))
                    * tint
            },
        );
    }
}

/// Grass and a few flowers close round the walls, where no road
/// runs.
fn verge(
    meshing: &mut Meshing,
    rng: &mut Rng,
    pal: &Palette,
    look: &Look,
    yard: &Yard,
) {
    let tip = pal.grass_tip;
    let bloom = pal.flowers[(rng.f() * 5.0) as usize % 5];
    for i in 0..22 {
        // A point just outside the walls, anywhere but the path to
        // the door.
        let a = rng.f() * std::f32::consts::TAU;
        let out = rng.range(0.2, 0.75);
        let edge = Vec2::new(a.cos(), a.sin());
        let k = (look.half.x / edge.x.abs().max(1e-3))
            .min(look.half.y / edge.y.abs().max(1e-3));
        let p = edge * (k + out);
        if p.y > look.half.y && p.x.abs() < look.half.x * 0.75 {
            continue;
        }
        // The yard is paved.
        if p.x * yard.side > look.half.x {
            continue;
        }
        let at = Vec3::new(p.x, -0.02, p.y);
        if i % 4 == 0 {
            let col = bloom * rng.value(0.1);
            let h = rng.range(0.14, 0.24);
            meshing.tube(
                at,
                at + Vec3::Y * h,
                0.012,
                0.01,
                3,
                pal.foliage_shade,
                false,
            );
            meshing.blob(
                at + Vec3::Y * h,
                Vec3::new(0.06, 0.045, 0.06),
                3,
                0.0,
                0.0,
                move |_| col,
            );
            continue;
        }
        for _ in 0..3 {
            let foot =
                at + Vec3::new(rng.sym(0.09), 0.0, rng.sym(0.09));
            let h = rng.range(0.13, 0.26);
            let base = pal.grass.lerp(pal.grass_shade, rng.f() * 0.5);
            let top = base.lerp(tip, rng.range(0.4, 0.9));
            meshing.lathe(
                foot,
                &[(0.055, 0.0), (0.0, h)],
                4,
                |r, _| {
                    if r == 0 { base } else { top }
                },
            );
        }
    }
}

/// The garden, on `side` of the house.
fn garden(
    meshing: &mut Meshing,
    rng: &mut Rng,
    pal: &Palette,
    look: &Look,
    side: f32,
) {
    let x = side * (look.half.x + 0.55);
    let z = look.half.y - 0.2;
    match look.garden {
        0 => {
            bush(meshing, rng, pal, Vec3::new(x, 0.0, z), 0.42);
            bush(
                meshing,
                rng,
                pal,
                Vec3::new(-x * 0.9, 0.0, z + 0.4),
                0.3,
            );
        }
        1 => {
            // A barrel and a crate.
            let staves = [
                (0.0, 0.0),
                (0.18, 0.0),
                (0.23, 0.25),
                (0.18, 0.56),
                (0.0, 0.56),
            ];
            let timber = pal.timber;
            meshing.lathe(
                Vec3::new(x, 0.0, z),
                &staves,
                8,
                |_, s| timber * (1.1 + 0.08 * (s % 3) as f32),
            );
            meshing.rbox(
                Vec3::new(x + side * 0.1, 0.16, z - 0.55),
                Vec3::splat(0.16),
                0.02,
                Quat::from_rotation_y(0.4),
                pal.timber * 1.2,
                0.3,
            );
        }
        2 => {
            // A bed of flowers along the front wall.
            for i in 0..7 {
                let col = pal.flowers[i % 4] * rng.value(0.1);
                let p = Vec3::new(
                    -x * 0.25 - side * (0.3 + i as f32 * 0.17),
                    0.14,
                    look.half.y + 0.28 + rng.sym(0.06),
                );
                meshing.blob(
                    p,
                    Vec3::new(0.08, 0.11, 0.08),
                    3,
                    0.0,
                    0.0,
                    move |d| if d.y < -0.2 { col * 0.5 } else { col },
                );
            }
            bush(meshing, rng, pal, Vec3::new(x, 0.0, z - 0.6), 0.34);
        }
        3 => {
            // A woodpile under the eave.
            for i in 0..9 {
                let (row, k) = (i / 4, i % 4);
                let p = Vec3::new(
                    x - side * 0.2,
                    0.1 + row as f32 * 0.17,
                    -0.5 + k as f32 * 0.19 + row as f32 * 0.09,
                );
                meshing.tube(
                    p - Vec3::X * 0.3,
                    p + Vec3::X * 0.3,
                    0.085,
                    0.085,
                    6,
                    pal.log * rng.value(0.12),
                    true,
                );
            }
        }
        _ => {}
    }
}

/// Length and height of the mayor's banner.
pub(crate) const BANNER: Vec2 = Vec2::new(0.95, 0.55);

/// The mast that marks the mayor's house. Returns where its banner
/// hangs from. The cloth itself waves, and is `hamlet_vfx`'s.
fn mast(meshing: &mut Meshing, foot: Vec3, pal: &Palette) -> Vec3 {
    let top = foot + Vec3::Y * 1.9;
    meshing.tube(foot, top, 0.035, 0.025, 6, pal.timber_dark, true);
    meshing.blob(top, Vec3::splat(0.07), 3, 0.0, 0.0, |_| pal.brass);
    top - Vec3::Y * 0.08
}

/// Whether a house flies the mayor's banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Banner {
    Flown,
    None,
}

/// The house of `agent`, with its work yard. `wings` is the number
/// of houses merged into it.
pub fn house(
    land: &Land,
    agent: AgentId,
    style: &TownStyle,
    hall: bool,
    banner: Banner,
    wings: u8,
    yard: &Yard,
) -> HouseMesh {
    let (pal, seed) = (&land.pal, land.seed);
    let look = Look::of(land, agent, hall);
    let (base, door_out) = (look.base, land.set.town.door_out);
    let mut rng = Rng::seeded(&format!("house {seed} {}", agent.0));
    let mut b = Meshing::new();
    let mut panes = Meshing::new();
    let (hx, hz) = (look.half.x, look.half.y);
    let top = look.wall_top();
    let wall = look.wall_colour(pal) * rng.value(0.04);

    // A stone base, sunk so a slope never shows under it.
    b.rbox(
        Vec3::new(0.0, base * 0.5 - 0.3, 0.0),
        Vec3::new(hx + 0.06, base * 0.5 + 0.3, hz + 0.06),
        0.05,
        Quat::IDENTITY,
        pal.limestone_shade.lerp(pal.limestone, 0.5),
        0.35,
    );
    let lean = rng.wobble(0.6);
    let first = if look.storeys > 1 { base + 1.45 } else { top };
    b.rbox(
        Vec3::new(0.0, (base + first) * 0.5, 0.0),
        Vec3::new(hx, (first - base) * 0.5 + 0.02, hz),
        0.04,
        lean,
        wall,
        0.14,
    );
    // An upper storey that juts out a little, as timber houses do.
    let jut = if look.storeys > 1 { 0.09 } else { 0.0 };
    let upper = Vec2::new(hx + jut, hz + jut);
    if look.storeys > 1 {
        b.rbox(
            Vec3::new(0.0, first + 0.04, 0.0),
            Vec3::new(upper.x + 0.03, 0.05, upper.y + 0.03),
            0.025,
            Quat::IDENTITY,
            pal.timber_dark,
            0.0,
        );
        let upper_wall = if look.wall == Wall::Stone {
            pal.plaster
        } else {
            wall
        };
        b.rbox(
            Vec3::new(0.0, (first + top) * 0.5 + 0.04, 0.0),
            Vec3::new(upper.x, (top - first) * 0.5, upper.y),
            0.04,
            lean,
            upper_wall,
            0.08,
        );
    }
    let roof_wall = if look.storeys > 1 && look.wall == Wall::Stone {
        pal.plaster
    } else {
        wall
    };

    match look.wall {
        Wall::Framed => {
            // Posts at the corners, a rail under the eave and braces
            // across the front.
            for (sx, sz) in [
                (-1.0f32, 1.0f32),
                (1.0, 1.0),
                (-1.0, -1.0),
                (1.0, -1.0),
            ] {
                b.rbox(
                    Vec3::new(
                        sx * (upper.x - 0.03),
                        (base + top) * 0.5,
                        sz * (upper.y - 0.03),
                    ),
                    Vec3::new(0.07, (top - base) * 0.5, 0.07),
                    0.02,
                    rng.wobble(0.8),
                    pal.timber * rng.value(0.08),
                    0.2,
                );
            }
            for sz in [-1.0f32, 1.0] {
                b.rbox(
                    Vec3::new(0.0, top - 0.1, sz * (upper.y + 0.005)),
                    Vec3::new(upper.x, 0.055, 0.03),
                    0.015,
                    Quat::IDENTITY,
                    pal.timber * rng.value(0.08),
                    0.0,
                );
            }
            for side in [-1.0f32, 1.0] {
                b.rbox(
                    Vec3::new(
                        side * (upper.x - 0.42),
                        top - 0.5,
                        upper.y + 0.005,
                    ),
                    Vec3::new(0.04, 0.5, 0.025),
                    0.012,
                    Quat::from_rotation_z(side * 0.6),
                    pal.timber_dark * rng.value(0.08),
                    0.0,
                );
            }
        }
        Wall::Stone => {
            // Corner stones, laid long and short by turns.
            for (sx, sz) in [
                (-1.0f32, 1.0f32),
                (1.0, 1.0),
                (-1.0, -1.0),
                (1.0, -1.0),
            ] {
                let mut y = base + 0.12;
                let mut k = 0;
                while y < first - 0.1 {
                    let long = if k % 2 == 0 {
                        (0.2, 0.11)
                    } else {
                        (0.11, 0.2)
                    };
                    b.rbox(
                        Vec3::new(
                            sx * (hx - long.0 + 0.035),
                            y,
                            sz * (hz - long.1 + 0.035),
                        ),
                        Vec3::new(long.0, 0.13, long.1),
                        0.03,
                        rng.wobble(2.0),
                        pal.limestone_shade
                            .lerp(pal.limestone, rng.range(0.3, 0.9)),
                        0.1,
                    );
                    y += 0.28;
                    k += 1;
                }
            }
        }
        _ => {
            // Patches where the plaster has worn through.
            for _ in 0..4 {
                let (w, h) =
                    (rng.range(0.15, 0.36), rng.range(0.1, 0.24));
                b.rbox(
                    Vec3::new(
                        rng.sym(hx - 0.5),
                        rng.range(base + 0.25, first - 0.3),
                        hz - 0.004,
                    ),
                    Vec3::new(w, h, 0.012),
                    0.01,
                    Quat::from_rotation_z(rng.sym(0.3)),
                    pal.plaster_worn.lerp(pal.plaster, rng.f()),
                    0.0,
                );
            }
        }
    }

    roof(&mut b, &mut rng, &look, style, pal, upper, top, roof_wall);

    // The door, off to one side, with a stone frame.
    let door_x = -look.side * hx * 0.3;
    let door = Vec3::new(door_x, base, hz);
    b.rbox(
        door + Vec3::new(0.0, 0.5, 0.0),
        Vec3::new(0.27, 0.52, 0.04),
        0.02,
        Quat::IDENTITY,
        look.livery * rng.value(0.05),
        0.25,
    );
    for side in [-1.0f32, 1.0] {
        b.rbox(
            door + Vec3::new(side * 0.34, 0.5, 0.02),
            Vec3::new(0.075, 0.56, 0.07),
            0.03,
            rng.wobble(1.5),
            pal.limestone * rng.value(0.07),
            0.15,
        );
    }
    b.rbox(
        door + Vec3::new(0.0, 1.12, 0.02),
        Vec3::new(0.46, 0.085, 0.08),
        0.03,
        rng.wobble(1.5),
        pal.limestone * rng.value(0.07),
        0.0,
    );
    b.blob(
        door + Vec3::new(look.side * 0.15, 0.5, 0.05),
        Vec3::splat(0.035),
        3,
        0.0,
        0.0,
        |_| pal.brass,
    );
    // Flagstones from the door out to where the roads end.
    let mut z = hz + 0.3;
    let mut k = 0;
    while z < door_out + 0.2 {
        let len = rng.range(0.42, 0.6);
        let t = ((z - hz) / (door_out - hz)).clamp(0.0, 1.0);
        for side in [-1.0f32, 1.0] {
            b.rbox(
                Vec3::new(
                    door_x * (1.0 - t) + side * 0.3,
                    base - 0.36 * t.min(1.0) - 0.05,
                    z,
                ),
                Vec3::new(0.27, 0.07, len * 0.5 - 0.02),
                0.03,
                rng.wobble(1.4),
                pal.limestone
                    .lerp(pal.limestone_shade, rng.f() * 0.4),
                0.3,
            );
        }
        z += len;
        k += 1;
        if k > 6 {
            break;
        }
    }

    // Windows: beside the door, above it, and on the gable ends.
    let sill = base + 0.82;
    let mut spots = vec![(
        Vec3::new(door_x + look.side * hx * 0.72, sill, hz + 0.005),
        Quat::IDENTITY,
    )];
    if hx > 1.5 || hall {
        spots.push((
            Vec3::new(door_x - look.side * 0.95, sill, hz + 0.005),
            Quat::IDENTITY,
        ));
    }
    if look.storeys > 1 {
        for x in [-0.5f32, 0.5] {
            spots.push((
                Vec3::new(
                    x * hx * 1.1,
                    first + 0.62,
                    upper.y + 0.005,
                ),
                Quat::IDENTITY,
            ));
        }
    }
    for side in [-1.0f32, 1.0] {
        let y = if look.storeys > 1 { first + 0.62 } else { sill };
        spots.push((
            Vec3::new(side * (upper.x + 0.005), y, 0.1),
            Quat::from_rotation_y(side * FRAC_PI_2),
        ));
    }
    spots.push((
        Vec3::new(look.side * 0.3, sill, -hz - 0.005),
        Quat::from_rotation_y(PI),
    ));
    for (c, face) in spots {
        window(
            &mut b,
            &mut panes,
            pal,
            c,
            face,
            look.livery,
            &mut rng,
        );
    }

    // The chimney, through the roof on the garden side.
    let stack = Vec3::new(look.side * (hx - 0.5), top, -hz * 0.35);
    let reach = look.pitch * (hz + look.eave) + 0.55;
    b.rbox(
        stack + Vec3::Y * reach * 0.5,
        Vec3::new(0.19, reach * 0.5, 0.19),
        0.03,
        rng.wobble(1.2),
        pal.limestone * rng.value(0.08),
        0.1,
    );
    b.rbox(
        stack + Vec3::Y * (reach + 0.03),
        Vec3::new(0.24, 0.04, 0.24),
        0.02,
        Quat::IDENTITY,
        pal.limestone_shade,
        0.0,
    );

    // A wing for each house merged into this one: the first where
    // the garden was, the second on the yard's side.
    for w in 0..wings.min(2) {
        let side = if w == 0 { -yard.side } else { yard.side };
        let (wx, wz, wh) = (0.7, hz * 0.75, base + 1.15);
        let c = Vec3::new(side * (hx + wx - 0.05), 0.0, -0.15);
        b.rbox(
            c + Vec3::Y * (wh * 0.5 - 0.15),
            Vec3::new(wx, wh * 0.5 + 0.15, wz),
            0.04,
            Quat::IDENTITY,
            wall * 0.96,
            0.2,
        );
        let (out, inn) = (c.x + side * (wx + 0.2), c.x - side * wx);
        slope(
            &mut b,
            &mut rng,
            [
                Vec3::new(out, wh - 0.1, side * (wz + 0.2)),
                Vec3::new(out, wh - 0.1, -side * (wz + 0.2)),
            ],
            [
                Vec3::new(inn, wh + 0.75, side * (wz + 0.2)),
                Vec3::new(inn, wh + 0.75, -side * (wz + 0.2)),
            ],
            style,
            pal,
        );
    }

    if hall {
        tower(&mut b, &mut panes, &mut rng, &look, style, pal);
    }
    let mut flies = None;
    if banner == Banner::Flown {
        let foot = if hall {
            Vec3::new(-hx - 0.2, top + 3.0, -hz + 0.2)
        } else {
            Vec3::new(
                -look.side * (hx - 0.3),
                top + look.pitch * 0.6,
                0.0,
            )
        };
        flies = Some((
            mast(&mut b, foot, pal),
            [style.banner, style.roof],
        ));
    }

    if wings == 0 {
        garden(&mut b, &mut rng, pal, &look, -yard.side);
    }
    verge(&mut b, &mut rng, pal, &look, yard);
    yard::pave(&mut b, &mut rng, yard, pal);

    // The bracket the lantern hangs from.
    let lamp =
        Vec3::new(door_x + look.side * 0.62, base + 1.0, hz + 0.3);
    b.tube(
        lamp + Vec3::new(0.0, 0.27, -0.3),
        lamp + Vec3::new(0.0, 0.3, 0.0),
        0.016,
        0.016,
        5,
        pal.bed,
        true,
    );
    b.tube(
        lamp + Vec3::Y * 0.3,
        lamp + Vec3::Y * 0.12,
        0.008,
        0.008,
        4,
        pal.bed,
        false,
    );
    lantern_cage(&mut b, lamp, 0.075, 0.17, pal.brass);
    b.lathe(
        lamp + Vec3::Y * 0.085,
        &[(0.095, 0.0), (0.05, 0.06), (0.0, 0.085)],
        8,
        |_, _| pal.brass,
    );

    HouseMesh {
        body: b,
        panes,
        lamp,
        banner: flies,
    }
}

/// The round tower at the back corner of a town's hall.
fn tower(
    b: &mut Meshing,
    panes: &mut Meshing,
    rng: &mut Rng,
    look: &Look,
    style: &TownStyle,
    pal: &Palette,
) {
    let c = Vec3::new(-look.half.x - 0.2, 0.0, -look.half.y + 0.2);
    let h = look.wall_top() + 1.2;
    let stone = pal.limestone;
    let shade = pal.limestone_shade;
    b.lathe(
        c - Vec3::Y * 0.3,
        &[(0.92, 0.0), (0.86, 0.5), (0.8, h + 0.3), (0.9, h + 0.42)],
        12,
        |r, s| stone.lerp(shade, ((r * 7 + s * 3) % 5) as f32 * 0.07),
    );
    let (roof, drift) = (style.roof, style.drift);
    b.lathe(
        c + Vec3::Y * (h + 0.05),
        &[(1.12, 0.0), (0.7, 0.75), (0.3, 1.5), (0.0, 1.95)],
        12,
        |r, s| roof.lerp(drift, ((r * 5 + s * 7) % 4) as f32 * 0.25),
    );
    for a in [0.6f32, 2.5] {
        let face = Quat::from_rotation_y(a);
        window(
            b,
            panes,
            pal,
            c + Vec3::Y * (h - 0.55) + face * Vec3::Z * 0.83,
            face,
            look.livery,
            rng,
        );
    }
}

/// The remains of an archived agent's house: the base, broken walls
/// and moss, on the same footprint.
pub fn ruin(land: &Land, agent: AgentId, hall: bool) -> Meshing {
    let (pal, seed) = (&land.pal, land.seed);
    let look = Look::of(land, agent, hall);
    let base = look.base;
    let mut rng = Rng::seeded(&format!("ruin {seed} {}", agent.0));
    let mut b = Meshing::new();
    let (hx, hz) = (look.half.x, look.half.y);
    let grey = |c: Vec3, rng: &mut Rng| {
        c.lerp(pal.rock, 0.55).lerp(pal.moss, rng.range(0.0, 0.45))
    };
    b.rbox(
        Vec3::new(0.0, base * 0.5 - 0.3, 0.0),
        Vec3::new(hx + 0.06, base * 0.5 + 0.3, hz + 0.06),
        0.05,
        Quat::IDENTITY,
        grey(pal.limestone_shade, &mut rng),
        0.35,
    );
    // Stubs of wall around the edge, highest at the corners.
    let wall = look.wall_colour(pal);
    let around = [
        (Vec2::new(-hx, hz), Vec2::new(hx, hz)),
        (Vec2::new(hx, hz), Vec2::new(hx, -hz)),
        (Vec2::new(hx, -hz), Vec2::new(-hx, -hz)),
        (Vec2::new(-hx, -hz), Vec2::new(-hx, hz)),
    ];
    for (a, c) in around {
        let pieces = 4;
        for i in 0..pieces {
            let (t0, t1) = (
                i as f32 / pieces as f32,
                (i + 1) as f32 / pieces as f32,
            );
            let corner = (t0 + t1 - 1.0).abs();
            let h =
                rng.range(0.1, 0.5) + corner * rng.range(0.3, 1.0);
            // The doorway stays open.
            if a.y == hz && c.y == hz && i == 1 {
                continue;
            }
            let (p, q) = (a.lerp(c, t0), a.lerp(c, t1));
            let mid = (p + q) * 0.5;
            let d = q - p;
            b.rbox(
                Vec3::new(mid.x, base + h * 0.5, mid.y),
                Vec3::new(d.length() * 0.5, h * 0.5, 0.09),
                0.03,
                Quat::from_rotation_y(-d.y.atan2(d.x))
                    * rng.wobble(1.5),
                grey(wall, &mut rng),
                0.25,
            );
        }
    }
    // Fallen stones, and moss taking the floor.
    for _ in 0..7 {
        let r = rng.range(0.1, 0.22);
        let col = grey(pal.limestone, &mut rng);
        b.rbox(
            Vec3::new(
                rng.sym(hx + 0.6),
                base * rng.range(0.0, 1.0) + r * 0.5 - 0.1,
                rng.sym(hz + 0.6),
            ),
            Vec3::new(r, r * 0.7, r * 0.8),
            0.03,
            rng.wobble(25.0),
            col,
            0.3,
        );
    }
    for _ in 0..3 {
        let c = Vec3::new(rng.sym(hx * 0.7), base, rng.sym(hz * 0.7));
        let r = rng.range(0.25, 0.4);
        bush(&mut b, &mut rng, pal, c, r);
    }
    b
}

/// A house as a few flat faces, for towns seen from far away.
pub fn far(
    meshing: &mut Meshing,
    panes: &mut Meshing,
    xf: Affine3A,
    land: &Land,
    agent: AgentId,
    style: &TownStyle,
    hall: bool,
    remains: Option<Remains>,
) {
    if remains == Some(Remains::Bare) {
        return;
    }
    let pal = &land.pal;
    let look = Look::of(land, agent, hall);
    let (base, eave) = (look.base, look.eave);
    let (hx, hz) = (look.half.x, look.half.y);
    let wall = look.wall_colour(pal);
    meshing.at(xf);
    if remains.is_some() {
        meshing.rbox(
            Vec3::Y * 0.2,
            Vec3::new(hx, 0.45, hz),
            0.05,
            Quat::IDENTITY,
            wall.lerp(pal.rock, 0.6),
            0.3,
        );
        meshing.at(Affine3A::IDENTITY);
        return;
    }
    let top = look.wall_top();
    let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    for (a, c, out) in [
        (p(-hx, 0.0, hz), p(hx, 0.0, hz), Vec3::Z),
        (p(hx, 0.0, hz), p(hx, 0.0, -hz), Vec3::X),
        (p(hx, 0.0, -hz), p(-hx, 0.0, -hz), -Vec3::Z),
        (p(-hx, 0.0, -hz), p(-hx, 0.0, hz), -Vec3::X),
    ] {
        let up = Vec3::Y * top;
        meshing.face_cols(
            [a - Vec3::Y * 0.3, c - Vec3::Y * 0.3, c + up, a + up],
            out,
            [wall * 0.7, wall * 0.7, wall, wall],
        );
    }
    let rise = look.pitch * (hz + look.eave);
    let fronted = look.roof == Roof::Fronted;
    for side in [-1.0f32, 1.0] {
        let (a, c, apex, out) = if fronted {
            (
                p(-hx, top, side * hz),
                p(hx, top, side * hz),
                p(0.0, top + rise, side * hz),
                Vec3::Z * side,
            )
        } else {
            (
                p(side * hx, top, -hz),
                p(side * hx, top, hz),
                p(side * hx, top + rise, 0.0),
                Vec3::X * side,
            )
        };
        meshing.face([a, c, apex, apex], out, wall);
    }
    panes.at(xf);
    for (c, face) in [
        (p(0.4, base + 0.82, hz + 0.02), Quat::IDENTITY),
        (
            p(hx + 0.02, base + 0.82, 0.0),
            Quat::from_rotation_y(FRAC_PI_2),
        ),
        (
            p(-hx - 0.02, base + 0.82, 0.0),
            Quat::from_rotation_y(-FRAC_PI_2),
        ),
    ] {
        let at = |x: f32, y: f32| c + face * Vec3::new(x, y, 0.0);
        panes.face(
            [
                at(-0.2, -0.27),
                at(0.2, -0.27),
                at(0.2, 0.27),
                at(-0.2, 0.27),
            ],
            face * Vec3::Z,
            Vec3::ONE,
        );
    }
    panes.at(Affine3A::IDENTITY);
    let (ex, ez) = (hx + look.eave, hz + eave);
    let col = style.roof.lerp(style.drift, 0.4);
    let (y0, y1) = (top - 0.08, top - 0.08 + rise);
    if fronted {
        for side in [-1.0f32, 1.0] {
            meshing.face_cols(
                [
                    p(side * ex, y0, ez),
                    p(side * ex, y0, -ez),
                    p(0.0, y1, -ez),
                    p(0.0, y1, ez),
                ],
                Vec3::new(side, 1.0, 0.0),
                [col * 0.85, col * 0.85, col, col],
            );
        }
    } else {
        for side in [-1.0f32, 1.0] {
            meshing.face_cols(
                [
                    p(-ex, y0, side * ez),
                    p(ex, y0, side * ez),
                    p(ex, y1, 0.0),
                    p(-ex, y1, 0.0),
                ],
                Vec3::new(0.0, 1.0, side),
                [col * 0.85, col * 0.85, col, col],
            );
        }
    }
    meshing.at(Affine3A::IDENTITY);
}
