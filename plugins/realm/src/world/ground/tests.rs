use std::collections::HashSet;

use glam::Vec2;

use super::sod::{Recipe, Sod, SodJob};
use super::*;
use crate::tuning::WorldTuning;

/// The land as designed, from the seed 1, and how it is drawn.
fn land() -> (Land, DetailSettings) {
    let (tuning, pal) = WorldTuning::read().unwrap();
    (Land::new(1, tuning.generator, pal), tuning.detail)
}

/// The ground of `recipe`'s tile, built whole.
fn sod(land: &Land, recipe: Recipe, drawn: &HashSet<Tile>) -> Sod {
    let mut job = SodJob::new(land, recipe, drawn);
    loop {
        if let Some(sod) = job.step(land) {
            return sod;
        }
    }
}

/// The height of a tile's mesh over `p`.
fn drawn_at(land: &Land, recipe: Recipe, sod: &Sod, p: Vec2) -> f32 {
    let at = &sod.mesh.positions;
    let chunk = land.set.land.chunk;
    let n = recipe.cells;
    let step = recipe.tile.side(chunk) / n as f32;
    let q = ((p - recipe.tile.corner(chunk)) / step)
        .clamp(Vec2::ZERO, Vec2::splat(n as f32 - 1e-3));
    let (i, j) = (q.x as usize, q.y as usize);
    let f = q - Vec2::new(i as f32, j as f32);
    let h = |i: usize, j: usize| at[(i * (n + 1) + j) * 3 + 1];
    let (h00, h11) = (h(i, j), h(i + 1, j + 1));
    if f.y >= f.x {
        let h01 = h(i, j + 1);
        h00 + (h11 - h01) * f.x + (h01 - h00) * f.y
    } else {
        let h10 = h(i + 1, j);
        h00 + (h10 - h00) * f.x + (h11 - h10) * f.y
    }
}

#[test]
fn far_ground_never_covers_what_stands_on_the_near() {
    let (land, detail) = land();
    let chunk = land.set.land.chunk;
    let dry = land.set.land.water_y + 0.6;
    for level in 0..4u8 {
        for (x, z) in [(0, 0), (2, -1), (-3, 1)] {
            let tile = Tile { level, x, z };
            let recipe = Recipe::of(&land, &detail, tile);
            let sod = sod(&land, recipe, &HashSet::new());
            let side = tile.side(chunk);
            for k in 0..900 {
                let p = tile.corner(chunk)
                    + Vec2::new(
                        (k % 30) as f32 + 0.37,
                        (k / 30) as f32 + 0.61,
                    ) * side
                        / 30.0;
                let stood = land.surface(p);
                let over = drawn_at(&land, recipe, &sod, p) - stood;
                if level == 0 {
                    // The near ground is what things stand on.
                    assert!(over.abs() < 1e-3, "{tile:?} {over}");
                } else if stood > dry {
                    assert!(over < 0.0, "{tile:?} at {p}: {over}");
                }
            }
        }
    }
}
