use std::collections::HashSet;

use bevy::prelude::*;

use super::*;
use crate::generator::{flora, testing};

fn land() -> Land {
    testing::settled_land(1)
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
            assert!(!a.is_over(*b) && !b.is_over(*a), "{a:?} {b:?}");
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
            tiles.iter().any(|t| t.is_over(finest)),
            "nothing under {p}"
        );
    }
}

#[test]
fn the_woods_are_whole_wherever_and_however_far_one_looks() {
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
            let tiles = wanted(&land, &detail, &sight, |_| false);
            whole(&land, &tiles, &sight);
            most = most.max(tiles.len());
        }
    }
    // The count is bounded by the view, wherever it is.
    assert!(most < 1500, "{most} tiles");
}

#[test]
fn a_wood_is_the_same_whatever_was_looked_at_before() {
    let land = land();
    let detail = DetailSettings::default();
    let tile = Tile {
        level: 1,
        x: 2,
        z: 2,
    };
    let stand = Stand::of(&detail, tile);
    let first = grove(&land, tile, stand);
    // Other woods are found in between, far and near.
    for other in [(0, 0, 0), (3, -9, 4), (0, 81, -154)] {
        let other = Tile {
            level: other.0,
            x: other.1,
            z: other.2,
        };
        grove(&land, other, Stand::of(&detail, other));
    }
    let again = grove(&land, tile, stand);
    assert!(!first.trees.is_empty());
    assert_eq!(first.trees.len(), again.trees.len());
    for (a, b) in first.trees.iter().zip(&again.trees) {
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
        x: 0,
        z: -1,
    };
    let count = |every: u32| {
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
        wanted(&land, &detail, sight, |t| drawn.contains(&t))
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
