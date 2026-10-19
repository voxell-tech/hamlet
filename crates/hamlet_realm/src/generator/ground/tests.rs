use std::collections::HashSet;

use bevy::prelude::*;

use super::sod::Recipe;
use super::*;
use crate::generator::{flora, testing};

fn land() -> Land {
    testing::land(1)
}

fn sight(centre: Vec2, far: f32) -> Sight {
    Sight {
        eye: Vec3::new(centre.x, far * 0.64, centre.y + far * 0.77),
        centre,
        reach: far * 2.3 + 135.0,
    }
}

/// Asserts that the tiles cover all the ground within reach, none
/// over another.
fn whole(land: &Land, tiles: &[Tile], sight: &Sight) {
    let chunk = land.set.land.chunk;
    for (i, a) in tiles.iter().enumerate() {
        for b in &tiles[i + 1..] {
            assert!(!a.covers(*b) && !b.covers(*a), "{a:?} {b:?}");
        }
    }
    for k in 0..400 {
        let p = sight.centre
            + Vec2::from_angle(k as f32 * 2.4)
                * (sight.reach * (k as f32 / 400.0).sqrt());
        let cell = (p / chunk).floor();
        let finest = Tile {
            level: 0,
            x: cell.x as i32,
            z: cell.y as i32,
        };
        assert!(
            tiles.iter().any(|t| t.covers(finest)),
            "nothing under {p}"
        );
    }
}

#[test]
fn a_river_does_not_spread_into_a_coarse_tile_beside_it() {
    let land = land();
    let detail = DetailSettings::default();
    let chunk = land.set.land.chunk;
    let water = land.set.land.water_y;
    let mut beside = 0;
    for x in -40..40 {
        for z in -40..40 {
            let tile = Tile { level: 4, x, z };
            let side = tile.side(chunk);
            let near = land.river_dist(tile.middle(chunk));
            if watered(&land, tile) || near > side {
                continue;
            }
            beside += 1;
            let recipe = Recipe::of(&land, &detail, tile);
            // Wet only where the true ground is under water.
            let (lo, step) =
                (tile.corner(chunk), side / recipe.cells as f32);
            let truly = (0..=recipe.cells).any(|i| {
                (0..=recipe.cells).any(|j| {
                    let p = lo + Vec2::new(i as f32, j as f32) * step;
                    land.height(p) < water + 0.15
                })
            });
            assert_eq!(
                sod(&land, recipe, &HashSet::new()).wet,
                truly,
                "{tile:?}"
            );
        }
    }
    assert!(beside > 5, "{beside} tiles beside a river");
}

#[test]
fn the_ground_is_whole_wherever_and_however_far_one_looks() {
    let land = land();
    let detail = DetailSettings::default();
    let mut most = 0;
    for centre in [
        Vec2::ZERO,
        Vec2::new(900.0, -4200.0),
        Vec2::new(-52000.0, 31000.0),
    ] {
        for far in [6.0, 72.0, 300.0, 1150.0] {
            let sight = sight(centre, far);
            for finer in [true, false] {
                let tiles =
                    wanted(&land, &detail, &sight, finer, |_| false);
                whole(&land, &tiles, &sight);
                most = most.max(tiles.len());
            }
        }
    }
    // The count is bounded by the view, wherever it is.
    assert!(most < 1500, "{most} tiles");
}

#[test]
fn a_tile_is_the_same_whatever_was_looked_at_before() {
    let land = land();
    let detail = DetailSettings::default();
    let tile = Tile {
        level: 1,
        x: 40,
        z: -77,
    };
    let recipe = Recipe::of(&land, &detail, tile);
    let stand = Stand::of(&detail, tile);
    let first = (
        sod(&land, recipe, &HashSet::new()).mesh,
        grove(&land, tile, stand),
    );
    // Other ground is built in between, far and near.
    for other in [(0, 0, 0), (3, -9, 4), (0, 81, -154)] {
        let other = Tile {
            level: other.0,
            x: other.1,
            z: other.2,
        };
        sod(
            &land,
            Recipe::of(&land, &detail, other),
            &HashSet::new(),
        );
        grove(&land, other, Stand::of(&detail, other));
    }
    let again = (
        sod(&land, recipe, &HashSet::new()).mesh,
        grove(&land, tile, stand),
    );
    let points = |mesh: &Mesh| {
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .map(<[_]>::to_vec)
            .unwrap()
    };
    assert_eq!(points(&first.0), points(&again.0));
    assert!(!first.1.trees.is_empty());
    assert_eq!(first.1.trees.len(), again.1.trees.len());
    for (a, b) in first.1.trees.iter().zip(&again.1.trees) {
        assert_eq!((a.at, a.kind), (b.at, b.kind));
    }
}

#[test]
fn tiles_share_no_tree_and_far_ones_hold_fewer() {
    let land = land();
    let detail = DetailSettings::default();
    let chunk = land.set.land.chunk;
    let over = Tile {
        level: 1,
        x: 3,
        z: -2,
    };
    let mut seen = HashSet::new();
    let mut under = 0;
    for tile in over.quarters() {
        let grove = grove(&land, tile, Stand::of(&detail, tile));
        assert!(grove.canopy.is_none());
        let (lo, side) = (tile.corner(chunk), tile.side(chunk));
        for tree in &grove.trees {
            let p = tree.at.xz();
            assert!(p.cmpge(lo).all() && p.cmplt(lo + side).all());
            assert!(seen.insert((p.x.to_bits(), p.y.to_bits())));
        }
        under += grove.trees.len();
    }
    let lo = over.corner(chunk);
    let all = flora::trees(&land, lo, lo + over.side(chunk), 1);
    assert_eq!(all.len(), under);

    // The same wood from far off is one mesh, and from further
    // one of fewer trees.
    let wood = Tile {
        level: 3,
        x: 2,
        z: 1,
    };
    let count = |every: u64| {
        let lo = wood.corner(chunk);
        flora::trees(&land, lo, lo + wood.side(chunk), every).len()
    };
    assert!(count(1) > 200);
    assert!(count(4) * 2 < count(1));
    let far = Tile { level: 5, ..wood };
    assert_eq!(
        Stand::of(&detail, far),
        Stand::Canopy {
            every: 16,
            crowns: false
        }
    );
    let mid = Tile { level: 2, ..wood };
    assert_eq!(
        Stand::of(&detail, mid),
        Stand::Canopy {
            every: 1,
            crowns: true
        }
    );
    let grove = grove(&land, wood, Stand::of(&detail, wood));
    assert!(grove.trees.is_empty() && grove.canopy.is_some());
}

#[test]
fn tiles_that_are_drawn_stay_while_the_view_wavers() {
    let land = land();
    let detail = DetailSettings::default();
    let here = sight(Vec2::new(10.0, 10.0), 200.0);
    let set = |sight: &Sight, drawn: &HashSet<Tile>| {
        wanted(&land, &detail, sight, true, |t| drawn.contains(&t))
            .into_iter()
            .collect::<HashSet<_>>()
    };
    let first = set(&here, &HashSet::new());
    assert_eq!(set(&here, &first), first);
    // A little higher, some tiles would be asked for larger, but
    // those that are drawn stay.
    let mut higher = here;
    higher.eye.y *= 1.08;
    assert_ne!(set(&higher, &HashSet::new()), first);
    assert_eq!(set(&higher, &first), first);
}

/// The height of a tile's mesh over `p`.
fn drawn_at(
    land: &Land,
    recipe: Recipe,
    mesh: &Mesh,
    p: Vec2,
) -> f32 {
    use bevy::mesh::VertexAttributeValues;
    let Some(VertexAttributeValues::Float32x3(at)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("a tile has no points");
    };
    let chunk = land.set.land.chunk;
    let n = recipe.cells;
    let step = recipe.tile.side(chunk) / n as f32;
    let q = ((p - recipe.tile.corner(chunk)) / step)
        .clamp(Vec2::ZERO, Vec2::splat(n as f32 - 1e-3));
    let (i, j) = (q.x as usize, q.y as usize);
    let f = q - Vec2::new(i as f32, j as f32);
    let h = |i: usize, j: usize| at[i * (n + 1) + j][1];
    let (h00, h11) = (h(i, j), h(i + 1, j + 1));
    if f.y >= f.x {
        let h01 = h(i, j + 1);
        h00 + (h11 - h01) * f.x + (h01 - h00) * f.y
    } else {
        let h10 = h(i + 1, j);
        h00 + (h10 - h00) * f.x + (h11 - h10) * f.y
    }
}

/// The stretch of edge two tiles share, as its start and its length
/// along each axis. None for two that touch at a corner or not at
/// all.
fn shared(chunk: f32, a: Tile, b: Tile) -> Option<(Vec2, Vec2)> {
    let lo = a.corner(chunk).max(b.corner(chunk));
    let hi = (a.corner(chunk) + a.side(chunk))
        .min(b.corner(chunk) + b.side(chunk));
    let size = hi - lo;
    (size.min_element().abs() < 1e-3 && size.max_element() > 1.0)
        .then_some((lo, size))
}

#[test]
fn tiles_drawn_together_meet_at_one_height_in_the_ranges() {
    let land = land();
    let detail = DetailSettings::default();
    let chunk = land.set.land.chunk;
    // Ranges with rivers through them, where a coarse tile lies
    // furthest from the ground and tiles of both grains meet.
    let sight = sight(Vec2::new(-4000.0, -300.0), 600.0);
    assert!(land.mountain(sight.centre) > 0.9);
    let tiles = wanted(&land, &detail, &sight, true, |_| false);
    let drawn = tiles.iter().copied().collect::<HashSet<_>>();
    let built = tiles
        .iter()
        .map(|tile| {
            let recipe = Recipe::of(&land, &detail, *tile);
            (recipe, sod(&land, recipe, &drawn).mesh)
        })
        .collect::<Vec<_>>();
    let (mut sizes, mut grains) = (0, 0);
    for (i, (a, mesh_a)) in built.iter().enumerate() {
        for (b, mesh_b) in &built[i + 1..] {
            let Some((lo, size)) = shared(chunk, a.tile, b.tile)
            else {
                continue;
            };
            sizes += usize::from(a.tile.level != b.tile.level);
            grains += usize::from(
                a.tile.level == b.tile.level && a.cells != b.cells,
            );
            for k in 0..=64 {
                let p = lo + size * (k as f32 / 64.0);
                let step = drawn_at(&land, *a, mesh_a, p)
                    - drawn_at(&land, *b, mesh_b, p);
                assert!(
                    step.abs() < 0.1,
                    "a step of {step} at {p} between {:?} and {:?}",
                    a.tile,
                    b.tile
                );
            }
        }
    }
    assert!(sizes > 100 && grains > 20, "{sizes} {grains}");
}

#[test]
fn no_gap_shows_between_tiles_of_different_sizes() {
    let land = land();
    let detail = DetailSettings::default();
    let chunk = land.set.land.chunk;
    let mut worst = (f32::MIN, None);
    let mut seams = 0;
    // From far above the first town, and nearer over hills.
    for (centre, far) in
        [(Vec2::ZERO, 1150.0), (Vec2::new(900.0, -4200.0), 400.0)]
    {
        let sight = sight(centre, far);
        let tiles = wanted(&land, &detail, &sight, true, |_| false);
        let built = tiles
            .iter()
            .map(|tile| {
                let recipe = Recipe::of(&land, &detail, *tile);
                (recipe, sod(&land, recipe, &HashSet::new()).mesh)
            })
            .collect::<Vec<_>>();
        for (i, (a, mesh_a)) in built.iter().enumerate() {
            for (b, mesh_b) in &built[i + 1..] {
                if a.tile.level == b.tile.level {
                    continue;
                }
                let (sa, sb) =
                    (a.tile.side(chunk), b.tile.side(chunk));
                let Some((lo, size)) = shared(chunk, a.tile, b.tile)
                else {
                    continue;
                };
                seams += 1;
                for k in 0..=64 {
                    let p = lo + size * (k as f32 / 64.0);
                    let ya = drawn_at(&land, *a, mesh_a, p);
                    let yb = drawn_at(&land, *b, mesh_b, p);
                    // The higher edge hangs a skirt down
                    // past the lower one.
                    let hem = if ya > yb {
                        a.hem(&land, sa, ya)
                    } else {
                        b.hem(&land, sb, yb)
                    };
                    let gap = hem - ya.min(yb);
                    if gap > worst.0 {
                        worst = (
                            gap,
                            Some((a.tile, b.tile, p, ya, yb, hem)),
                        );
                    }
                }
            }
        }
    }
    assert!(seams > 40, "{seams} seams");
    assert!(worst.0 <= 0.0, "a gap of {} at {:?}", worst.0, worst.1);
}

#[test]
fn far_ground_never_covers_what_stands_on_the_near() {
    let land = land();
    let detail = DetailSettings::default();
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
                let over =
                    drawn_at(&land, recipe, &sod.mesh, p) - stood;
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
