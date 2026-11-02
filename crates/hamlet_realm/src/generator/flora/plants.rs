//! What grows under the trees and between them, many to a tile's
//! one mesh: shrubs at the edge of the woods, reeds and lily pads at
//! the water, and toadstools in the deep woods that glow at night.
//! Each bends in the wind as much as it is tall, as
//! [`Meshing::grow`] marks it. The grass is the GPU's, about the eye.

use std::f32::consts::TAU;

use bevy::prelude::*;
use hamlet_vfx::leaves::Stroke;

use super::{Tree, card, litter, shaded_card, spread};
use crate::generator::land::Land;
use crate::generator::noise::{key, unit};
use crate::meshes::{Meshing, Rng, fbm, smoothstep};

/// Everything that grows between `lo` and `hi` under and about
/// `trees`, with the leaves fallen from them, as one mesh. None where
/// nothing grows.
pub(crate) fn plants(
    land: &Land,
    lo: Vec2,
    hi: Vec2,
    trees: &[Tree],
) -> Option<Mesh> {
    let mut meshing = Meshing::new();
    litter(&mut meshing, land, trees);
    for (p, h) in spots(land, lo, hi, 3.5, 82) {
        shrub(&mut meshing, land, p, h);
    }
    for (p, h) in spots(land, lo, hi, 0.9, 83) {
        reeds(&mut meshing, land, p, h);
    }
    for (p, h) in spots(land, lo, hi, 1.1, 84) {
        pad(&mut meshing, land, p, h);
    }
    for (p, h) in spots(land, lo, hi, 4.0, 85) {
        toadstools(&mut meshing, land, p, h);
    }
    // Marks what was not grown as still, so that the whole mesh is
    // drawn as plants are and none of it as one tree.
    meshing.grow(meshing.vertices(), 0.0, 1.0, 0.0);
    (!meshing.is_empty()).then(|| meshing.build())
}

/// One place in each square `cell` across between `lo` and `hi`,
/// somewhere in it, and a hash of it for what grows there.
fn spots(
    land: &Land,
    lo: Vec2,
    hi: Vec2,
    cell: f32,
    salt: u32,
) -> impl Iterator<Item = (Vec2, u32)> {
    let first = (lo / cell).floor();
    let last = (hi / cell).ceil();
    let seed = land.seed;
    (first.x as i32..last.x as i32)
        .flat_map(move |x| {
            (first.y as i32..last.y as i32).map(move |z| (x, z))
        })
        .filter_map(move |(x, z)| {
            let h = key(&[seed, salt, x as u32, z as u32]);
            let p = (Vec2::new(x as f32, z as f32)
                + Vec2::new(roll(h, 0), roll(h, 1)))
                * cell;
            (p.cmpge(lo).all() && p.cmplt(hi).all()).then_some((p, h))
        })
}

fn roll(h: u32, salt: u32) -> f32 {
    unit(key(&[h, salt]))
}

/// Whether nothing may grow at `p`: in a town, or on ground held
/// for something built.
fn is_kept(land: &Land, p: Vec2) -> bool {
    land.site_dist(p) < land.set.town.radius + 1.0
        || land.is_held(p, 0.5)
}

/// How level the ground is at `p`, from 1 on the flat to 0 from
/// about 30 degrees.
fn level(land: &Land, p: Vec2) -> f32 {
    smoothstep(0.14, 0.06, 1.0 - land.normal(p).y)
}

/// A shrub: a few low clumps of leaves, most at the edge of the
/// woods, where they meet open ground.
fn shrub(meshing: &mut Meshing, land: &Land, p: Vec2, h: u32) {
    let y = land.height(p);
    if y < land.set.land.water_y + 0.5 || y > 24.0 || is_kept(land, p)
    {
        return;
    }
    let f = land.forest(p);
    let edge = 4.0 * f * (1.0 - f);
    if roll(h, 2) > level(land, p) * (0.06 + 0.5 * edge) {
        return;
    }
    let pal = &land.pal;
    let mut rng = Rng::seeded(&format!("shrub {h}"));
    let size = rng.range(0.7, 1.1);
    let light = pal.foliage.lerp(pal.grass_tip, rng.f() * 0.4);
    let shade = pal.foliage_shade * 0.9;
    let at = Vec3::new(p.x, y - 0.1, p.y);
    let from = meshing.vertices();
    let lumps = 2 + (rng.f() * 3.0) as usize;
    for _ in 0..lumps {
        let a = rng.f() * TAU;
        let r = size * rng.range(0.35, 0.5);
        let middle = at
            + Vec3::new(a.cos(), 0.0, a.sin())
                * size
                * rng.range(0.1, 0.4)
            + Vec3::Y * r * 0.9;
        let core = shade * 0.8;
        meshing.blob(
            middle,
            Vec3::splat(r * 0.8),
            4,
            0.15,
            rng.f() * 40.0,
            move |_| core,
        );
        let n = ((50.0 * r * r) as usize).max(8);
        for d in spread(n, rng.f() * TAU) {
            if d.y < -0.4 {
                continue;
            }
            let leaf = middle + d * r;
            let face = (d + Vec3::Y * 0.3).normalize();
            let (u, w) = face.any_orthonormal_pair();
            let turn = rng.f() * TAU;
            let along = u * turn.cos() + w * turn.sin();
            let s = r * rng.range(0.8, 1.05);
            let up = smoothstep(-0.4, 0.8, d.y);
            card(
                meshing,
                leaf - along * s * 0.5,
                along * s,
                along.cross(face) * s * 1.1,
                if rng.f() < 0.6 {
                    Stroke::Dab
                } else {
                    Stroke::Blot
                },
                shade.lerp(light, up) * rng.value(0.08),
            );
        }
    }
    meshing.round(from, at + Vec3::Y * size * 0.3, 0.7);
    meshing.grow(from, at.y, size, 0.0);
}

/// A clump of reeds where the ground runs into the water, some with
/// a cattail's head.
fn reeds(meshing: &mut Meshing, land: &Land, p: Vec2, h: u32) {
    let y = land.height(p);
    let water = land.set.land.water_y;
    if y < water - 0.4 || y > water + 0.35 || is_kept(land, p) {
        return;
    }
    let patch =
        smoothstep(0.4, 0.6, fbm(p.x * 0.12 - 5.0, p.y * 0.12 + 9.0));
    if roll(h, 2) > 0.8 * patch * level(land, p).max(0.3) {
        return;
    }
    let pal = &land.pal;
    let tall = 0.9 + 0.7 * roll(h, 3);
    let at = Vec3::new(p.x, y.min(water) - 0.1, p.y);
    let from = meshing.vertices();
    let turn = roll(h, 4) * TAU;
    for k in 0..3 {
        let a = turn + k as f32 / 3.0 * TAU * 0.5;
        let across = Vec3::new(a.cos(), 0.0, a.sin()) * tall * 0.45;
        let top = at + Vec3::Y * tall * (0.85 + 0.3 * roll(h, 5 + k));
        shaded_card(
            meshing,
            top,
            at - top,
            across,
            Stroke::Blades,
            [pal.reed * 1.1, pal.reed * 0.6],
        );
    }
    if roll(h, 9) < 0.5 {
        let stem = at
            + Vec3::new(roll(h, 10) - 0.5, 0.0, roll(h, 11) - 0.5)
                * 0.2;
        let head = stem + Vec3::Y * tall * 1.05;
        meshing.tube(
            stem,
            head,
            0.012,
            0.01,
            4,
            pal.reed * 0.7,
            false,
        );
        meshing.tube(
            head,
            head + Vec3::Y * 0.18,
            0.035,
            0.03,
            5,
            pal.bark * 0.7,
            true,
        );
    }
    meshing.round(from, at - Vec3::Y * 1.5, 0.6);
    meshing.grow(from, at.y, tall, 0.0);
}

/// A lily pad lying on shallow water, in patches, some in flower.
fn pad(meshing: &mut Meshing, land: &Land, p: Vec2, h: u32) {
    let y = land.height(p);
    let water = land.set.land.water_y;
    if y < water - 1.3 || y > water - 0.3 || is_kept(land, p) {
        return;
    }
    let patch =
        smoothstep(0.5, 0.65, fbm(p.x * 0.1 + 41.0, p.y * 0.1 - 3.0));
    if roll(h, 2) > 0.75 * patch {
        return;
    }
    let pal = &land.pal;
    let s = 0.4 + 0.35 * roll(h, 3);
    let at = Vec3::new(p.x, water + 0.03, p.y);
    let spin = roll(h, 4) * TAU;
    let along = Vec3::new(spin.cos(), 0.0, spin.sin());
    card(
        meshing,
        at - along * s * 0.5,
        along * s,
        along.cross(Vec3::Y) * s,
        Stroke::Pad,
        pal.lily_pad * (0.85 + 0.3 * roll(h, 5)),
    );
    if roll(h, 6) < 0.15 {
        let colour = if roll(h, 7) < 0.5 {
            pal.flowers[2]
        } else {
            pal.blossom
        };
        for k in 0..3 {
            let a = spin + k as f32 / 3.0 * TAU;
            let out = Vec3::new(a.cos(), 0.0, a.sin());
            card(
                meshing,
                at + Vec3::Y * 0.12 + out * 0.02,
                out * 0.1 - Vec3::Y * 0.1,
                out.cross(Vec3::Y) * 0.12,
                Stroke::Blot,
                colour,
            );
        }
    }
}

/// A ring of toadstools in the deep woods, whose caps glow at night.
fn toadstools(meshing: &mut Meshing, land: &Land, p: Vec2, h: u32) {
    let y = land.height(p);
    if y < land.set.land.water_y + 0.5 || is_kept(land, p) {
        return;
    }
    let deep = smoothstep(0.5, 0.75, land.forest(p));
    if roll(h, 2) > 0.3 * deep * level(land, p) {
        return;
    }
    let pal = &land.pal;
    let count = 3 + (roll(h, 3) * 4.0) as u32;
    for k in 0..count {
        let a = roll(h, 4 + k) * TAU;
        let q = p + Vec2::from_angle(a) * 0.7 * roll(h, 12 + k);
        let foot = Vec3::new(q.x, land.height(q) - 0.02, q.y);
        let tall = 0.08 + 0.14 * roll(h, 20 + k);
        let r = 0.05 + 0.07 * roll(h, 28 + k);
        let from = meshing.vertices();
        meshing.tube(
            foot,
            foot + Vec3::Y * tall,
            r * 0.35,
            r * 0.3,
            5,
            pal.parchment,
            false,
        );
        meshing.grow(from, foot.y, 1.0, 0.25);
        let from = meshing.vertices();
        let cap = pal.glow_cap * (0.85 + 0.3 * roll(h, 36 + k));
        meshing.blob(
            foot + Vec3::Y * tall,
            Vec3::new(r, r * 0.55, r),
            3,
            0.1,
            k as f32,
            move |_| cap,
        );
        meshing.grow(from, foot.y, 1.0, 1.0);
    }
}
