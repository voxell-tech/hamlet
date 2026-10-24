//! The ground as tiles: which ones a view calls for, and the mesh
//! and the trees of each. Four tiles of one size lie under one of
//! the next, so near ground is fine and far ground cheap.

mod grove;
mod sod;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::prelude::*;

pub use self::grove::{Grove, Stand, grove};
pub use self::sod::{Recipe, Sod, sod};
use super::land::Land;

/// The extent of the ground drawn and the pace the world fills in at.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetailSettings {
    /// Camera distance beyond which a town is drawn as one plain
    /// mesh.
    pub town_far: f32,
    /// Camera distance beyond which a robot is not drawn.
    pub robots_far: f32,
    /// A robot further than this outside the view is not posed. It
    /// covers the reach of its lantern and of its shadow.
    pub robot_sight: f32,
    /// Distance from the camera's focus to which the ground is
    /// drawn, as a share of the distance at which the fog is whole.
    pub ground_reach: f32,
    /// Sizes of ground tile there are, each twice the side of the
    /// one before.
    pub tile_levels: u8,
    /// A tile is drawn as four finer ones while the camera is nearer
    /// to it than its side times this.
    pub tile_split: f32,
    /// Distance past that at which the four stay once drawn, so a
    /// camera at the line does not swap them back and forth.
    pub tile_keep: f32,
    /// Factors by which a tile with a river in it, and one with a
    /// town on it, is drawn finer further off.
    pub river_split: f32,
    pub town_split: f32,
    /// Depth of a tile of the second size under the true ground, so
    /// that roads and floors are not buried where the ground is
    /// drawn coarsely. Each size up doubles it, to the most given.
    pub tile_sink: (f32, f32),
    /// Drop of a tile that is being replaced under the tiles that
    /// take its place.
    pub stale_sink: f32,
    /// Depth under the water's level of the plain ground that shows
    /// where no tile is drawn yet. Deeper than any river bed.
    pub bed: f32,
    /// Sizes of tile, from the finest, on which every tree stands as
    /// itself. On larger ones a tile's trees are one plain mesh.
    pub tree_levels: u8,
    /// Sizes of tile, from the finest, on which those plain trees
    /// still have a crown. Beyond, each is a speck, and each size up
    /// keeps half as many.
    pub crown_levels: u8,
    /// Tiles of ground built at one time, and as many of trees.
    pub tile_tasks: usize,
    /// The most trees planted in one frame.
    pub trees_per_frame: usize,
    /// The most finished houses, roads and commons put into the
    /// world in one frame.
    pub builds_per_frame: usize,
}

/// One square of ground: its size, counted from the finest, and its
/// place among the tiles of that size.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Tile {
    pub level: u8,
    pub x: i32,
    pub z: i32,
}

impl Tile {
    pub fn side(self, chunk: f32) -> f32 {
        chunk * (1u32 << self.level) as f32
    }

    /// Its corner of least x and z.
    pub fn corner(self, chunk: f32) -> Vec2 {
        Vec2::new(self.x as f32, self.z as f32) * self.side(chunk)
    }

    pub fn middle(self, chunk: f32) -> Vec2 {
        self.corner(chunk) + self.side(chunk) * 0.5
    }

    /// The four tiles that lie under it.
    pub(crate) fn quarters(self) -> [Tile; 4] {
        [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dz)| Tile {
            level: self.level - 1,
            x: self.x * 2 + dx,
            z: self.z * 2 + dz,
        })
    }

    /// The tile of the next size that it lies under.
    pub fn over(self) -> Tile {
        Tile {
            level: self.level + 1,
            x: self.x.div_euclid(2),
            z: self.z.div_euclid(2),
        }
    }

    /// True when `other` is this tile or lies under it.
    pub fn is_over(self, other: Tile) -> bool {
        if other.level > self.level {
            return false;
        }
        let shift = self.level - other.level;
        (other.x >> shift, other.z >> shift) == (self.x, self.z)
    }

    /// The tile of `drawn`, of this size or larger, that covers the
    /// place `by` tiles of this size away. None where finer tiles
    /// lie there, or nothing does.
    pub(crate) fn beside(
        self,
        drawn: &HashSet<Tile>,
        top: u8,
        by: (i32, i32),
    ) -> Option<Tile> {
        let mut tile = Tile {
            level: self.level,
            x: self.x + by.0,
            z: self.z + by.1,
        };
        loop {
            if drawn.contains(&tile) {
                return Some(tile);
            }
            if tile.level >= top {
                return None;
            }
            tile = tile.over();
        }
    }

    /// Distance over the ground from `p` to the tile.
    fn away(self, chunk: f32, p: Vec2) -> f32 {
        let (lo, side) = (self.corner(chunk), self.side(chunk));
        (p - p.clamp(lo, lo + side)).length()
    }
}

/// The places about a tile: across each edge and each corner.
const ABOUT: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// A number that changes when the tiles of `drawn` that the rim of
/// `tile` is cut to do: those about it that are no smaller, and
/// those about each of them that is larger. `top` is the largest
/// size there is. A tile built for one number is right for as long
/// as the number holds.
pub fn rim_key(drawn: &HashSet<Tile>, top: u8, tile: Tile) -> u64 {
    let mut hasher = DefaultHasher::new();
    let mut open = vec![tile];
    let mut at = 0;
    while let Some(&of) = open.get(at) {
        at += 1;
        for by in ABOUT {
            let beside = of.beside(drawn, top, by);
            beside.hash(&mut hasher);
            if let Some(beside) = beside
                && beside.level > of.level
                && !open.contains(&beside)
            {
                open.push(beside);
            }
        }
    }
    hasher.finish()
}

/// The camera's sight of the ground.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sight {
    pub eye: Vec3,
    /// The middle of the ground that is drawn, and how far from it
    /// the ground reaches.
    pub centre: Vec2,
    pub reach: f32,
}

/// True when a river runs through `tile`.
fn is_watered(land: &Land, tile: Tile) -> bool {
    let chunk = land.set.land.chunk;
    land.river_dist(tile.middle(chunk)) < tile.side(chunk) * 0.75
}

/// The tiles `sight` calls for, each as large as its distance
/// allows. With `finer`, tiles with a river or a town are kept finer
/// than the rest. `drawn` tells which tiles are there now: four that
/// are stay a little longer than they would be asked for anew.
pub fn wanted(
    land: &Land,
    detail: &DetailSettings,
    sight: &Sight,
    finer: bool,
    drawn: impl Fn(Tile) -> bool,
) -> Vec<Tile> {
    let chunk = land.set.land.chunk;
    let top = detail.tile_levels.max(1) - 1;
    let side = chunk * (1u32 << top) as f32;
    let lo = ((sight.centre - sight.reach) / side).floor();
    let hi = ((sight.centre + sight.reach) / side).floor();
    let hold = land.set.town.radius + 15.0;
    let mut out = Vec::new();
    let mut open = Vec::new();
    for x in lo.x as i32..=hi.x as i32 {
        for z in lo.y as i32..=hi.y as i32 {
            open.push(Tile { level: top, x, z });
        }
    }
    while let Some(tile) = open.pop() {
        if tile.away(chunk, sight.centre) > sight.reach {
            continue;
        }
        let side = tile.side(chunk);
        let far =
            Vec2::new(tile.away(chunk, sight.eye.xz()), sight.eye.y)
                .length();
        let mid = tile.middle(chunk);
        let town = || {
            land.sites
                .iter()
                .any(|s| s.at.distance(mid) < side * 0.71 + hold)
        };
        let kept =
            tile.level > 0 && tile.quarters().into_iter().any(&drawn);
        let near = side
            * detail.tile_split
            * if kept { detail.tile_keep } else { 1.0 };
        let split = tile.level > 0
            && (far < near
                || (finer
                    && far < near * detail.river_split
                    && is_watered(land, tile))
                || (finer
                    && far < near * detail.town_split
                    && town()));
        if split {
            open.extend(tile.quarters());
        } else {
            out.push(tile);
        }
    }
    out
}
