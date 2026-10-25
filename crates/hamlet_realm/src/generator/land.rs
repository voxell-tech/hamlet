//! The land: height, rivers, town sites and the corridors between
//! them. It has no edge: every part of it follows from the seed and
//! from where the sites lie.

mod corridor;
mod terrain;
#[cfg(test)]
mod tests;

use std::f32::consts::TAU;
use std::ops::Deref;
use std::sync::Arc;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::GeneratorSettings;
use super::noise::{key, signed, unit};
use crate::palette::Palette;

/// The shape of the land.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LandSettings {
    pub water_y: f32,
    /// Ground height of every town's centre.
    pub site_y: f32,
    /// Distance between neighbouring town sites.
    pub site_spacing: f32,
    /// Rings of sites the land has around the middle one before any
    /// more are asked for.
    pub site_rings: i32,
    /// Side of one tile of ground at its finest.
    pub chunk: f32,
    /// Cells along the side of a tile, and of one a river runs
    /// through. Everything stands on the ground these draw. The
    /// second is a multiple of the first, so that where the two
    /// kinds of tile touch, every point of the coarser edge is one
    /// of the finer.
    pub tile_cells: (usize, usize),
    /// The height the plains lie about.
    pub plain_y: f32,
    /// The hills of the plains and the fine grain over them, each as
    /// the scale of its noise and its height.
    pub hills: (f32, f32),
    pub grain: (f32, f32),
    /// Reach of the home plains from the middle. Mountains stand
    /// only beyond.
    pub plains: f32,
    /// Share of the far land that is mountain: the range noise at
    /// which the ground begins to rise and at which it is all range.
    pub ranges: (f32, f32),
    /// The wobble of the home plains' edge, as the scale of its
    /// noise and how far it reaches.
    pub edge: (f32, f32),
    /// The scale of the noise the ranges lie by.
    pub range_scale: f32,
    /// The warp that bends the ridges, as the scale of its noise and
    /// how far it reaches.
    pub ridge_warp: (f32, f32),
    /// The peaks: the scale of their noise, and the least and the
    /// most height they add.
    pub peaks: (f32, f32, f32),
    /// Distance between one river and the next.
    pub river_gap: f32,
    /// A river's bends, as the scale of the noise and how far it
    /// reaches: the broad ones and the tight ones.
    pub bends: (f32, f32),
    pub kinks: (f32, f32),
    /// The scale of the noise a river drifts by.
    pub drift_scale: f32,
    /// Farthest a river may wander from its own line, besides its
    /// bends. Well under half the gap, so no two meet.
    pub river_drift: f32,
    /// Variation of the land beyond the home plains from one region
    /// to the next: the thinning or thickening of its woods, and the
    /// share of its grass that turns to heath.
    pub wilds: f32,
    pub heath: f32,
    /// Side of a cell of the grid trees are scattered on, and where
    /// the corner of its first cell lies from the middle.
    pub tree_cell: f32,
    pub tree_origin: f32,
}

/// A cell of the grid the land lays its sites and corridors on.
type Cell = (i32, i32);

/// Level ground for a town.
#[derive(Clone, Debug, PartialEq)]
pub struct Site {
    pub at: Vec2,
    /// The town's own rotation, so towns do not line up.
    pub turn: f32,
    /// False where another site crowds it.
    pub buildable: bool,
    /// Its place until it is moved.
    pub home: Vec2,
    cell: Cell,
}

impl Site {
    pub fn has_moved(&self) -> bool {
        self.at != self.home
    }
}

/// Reason a town cannot stand somewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unfit {
    /// On water, or with a house in it.
    Water,
    /// In the mountains.
    Steep,
    /// On top of another town, or of something else that holds its
    /// ground.
    Crowded,
}

/// The lie of the land for one seed, shared with the background
/// tasks that build the ground and find the trees.
#[derive(Resource, Clone)]
pub struct Land(Arc<LandData>);

impl Land {
    pub fn new(
        seed: u32,
        set: GeneratorSettings,
        pal: Palette,
    ) -> Self {
        Land(Arc::new(LandData::new(seed, set, pal)))
    }

    /// Readies one more site and returns its number: the nearest
    /// cell to the middle that suits a town, has no site lying in
    /// it and is not `taken`.
    pub(crate) fn open_site(
        &mut self,
        taken: impl Fn(Vec2) -> bool,
    ) -> usize {
        let apart = self.set.town.keep_apart;
        let mut ring = 0;
        let found = loop {
            let free = self.ring(ring).into_iter().find_map(|cell| {
                let home = self.hex_at(cell.0, cell.1);
                let open = !self
                    .sites
                    .iter()
                    .any(|s| s.at.distance(home) < apart)
                    && self.suits(home).is_ok()
                    && !taken(home);
                open.then_some((cell, home))
            });
            if let Some(found) = free {
                break found;
            }
            ring += 1;
        };
        let (cell, home) = found;
        let turn = self.turn_of(cell);
        let land = Arc::make_mut(&mut self.0);
        land.sites.push(Site {
            at: home,
            turn,
            buildable: true,
            home,
            cell,
        });
        land.index();
        land.sites.len() - 1
    }

    /// Moves a site, and the level ground with it. The ground it
    /// leaves is as the land would have it.
    pub(crate) fn move_site(&mut self, site: usize, to: Vec2) {
        if self.sites.get(site).is_none_or(|s| s.at == to) {
            return;
        }
        let land = Arc::make_mut(&mut self.0);
        land.sites[site].at = to;
        land.index();
    }

    /// Keeps each round of ground level and clear of trees, and lets
    /// go of any held before that is not among them.
    pub fn hold(&mut self, rounds: &[(Vec2, f32)]) {
        if self.held() == rounds {
            return;
        }
        let land = Arc::make_mut(&mut self.0);
        land.holds.clear();
        land.holds = rounds
            .iter()
            .map(|&(at, radius)| Hold {
                at,
                radius,
                y: land.height(at),
            })
            .collect();
    }
}

impl Deref for Land {
    type Target = LandData;

    fn deref(&self) -> &LandData {
        &self.0
    }
}

#[derive(Clone)]
pub struct LandData {
    pub seed: u32,
    /// The settings it was built from.
    pub set: GeneratorSettings,
    pub sites: Vec<Site>,
    river_dir: Vec2,
    /// The sites that reach into each square of ground.
    near: HashMap<Cell, Vec<u32>>,
    /// Ground held level by something that is not a town.
    holds: Vec<Hold>,
    pub pal: Palette,
}

/// A round of ground kept level and clear of trees, as a site is.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hold {
    at: Vec2,
    radius: f32,
    /// The height it is kept at: the land's own at its middle.
    y: f32,
}

/// Distance past a hold's edge over which the ground eases back to
/// its own lie.
const HOLD_EASE: f32 = 7.0;

/// Distance a road between towns keeps from a hold's edge.
const HOLD_CLEAR: f32 = 3.0;

const HEX_DIRS: [Cell; 6] =
    [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];

fn hex_ring(q: i32, r: i32) -> i32 {
    (q.abs() + r.abs() + (q + r).abs()) / 2
}

impl LandData {
    fn new(seed: u32, set: GeneratorSettings, pal: Palette) -> Self {
        let a = unit(key(&[seed, 11])) * TAU;
        let rings = set.land.site_rings;
        let spacing = set.land.site_spacing;
        let mut land = Self {
            seed,
            set,
            sites: Vec::new(),
            river_dir: Vec2::new(a.cos(), a.sin()),
            near: HashMap::default(),
            holds: Vec::new(),
            pal,
        };
        // Outward from the middle, so the first towns are neighbours.
        for ring in 0..=rings {
            for cell in land.ring(ring) {
                let home = land.hex_at(cell.0, cell.1);
                let crowded = land
                    .sites
                    .iter()
                    .any(|s| s.at.distance(home) < spacing * 0.8);
                land.sites.push(Site {
                    at: home,
                    turn: land.turn_of(cell),
                    buildable: !crowded,
                    home,
                    cell,
                });
            }
        }
        land.index();
        land
    }

    /// The cells at `ring` steps from the middle one, in the order
    /// their sites are numbered.
    fn ring(&self, ring: i32) -> Vec<Cell> {
        let mut cells = Vec::new();
        for q in -ring..=ring {
            for r in -ring..=ring {
                if hex_ring(q, r) == ring {
                    cells.push((q, r));
                }
            }
        }
        cells.sort_by_key(|&(q, r)| {
            let p = self.hex_at(q, r);
            (p.y.atan2(p.x) * 1000.0) as i32
        });
        cells
    }

    fn turn_of(&self, (q, r): Cell) -> f32 {
        unit(key(&[self.seed, 13, q as u32, r as u32])) * TAU
    }

    /// Reach of a site's hold on the ground and on what grows there.
    fn reach(&self) -> f32 {
        let town = &self.set.town;
        (town.radius + 15.0).max(town.gate_radius + 12.0) + 1.0
    }

    /// Notes which sites reach into which square of ground.
    fn index(&mut self) {
        let reach = self.reach();
        let mut near = HashMap::<Cell, Vec<u32>>::default();
        for (i, site) in self.sites.iter().enumerate() {
            let lo = ((site.at - reach) / reach).floor();
            let hi = ((site.at + reach) / reach).floor();
            for x in lo.x as i32..=hi.x as i32 {
                for z in lo.y as i32..=hi.y as i32 {
                    near.entry((x, z)).or_default().push(i as u32);
                }
            }
        }
        self.near = near;
    }

    /// Distance from `p` to the nearest site. Very large where none
    /// is within reach.
    pub(crate) fn site_dist(&self, p: Vec2) -> f32 {
        let square = (p / self.reach()).floor();
        let Some(ids) =
            self.near.get(&(square.x as i32, square.y as i32))
        else {
            return f32::MAX;
        };
        ids.iter()
            .map(|&i| self.sites[i as usize].at.distance_squared(p))
            .fold(f32::MAX, f32::min)
            .sqrt()
    }

    /// The place of a cell's site: a jittered hex grid, moved back
    /// from the river.
    fn hex_at(&self, q: i32, r: i32) -> Vec2 {
        let turn = unit(key(&[self.seed, 12])) * TAU;
        let grid = Vec2::new(
            q as f32 + r as f32 * 0.5,
            r as f32 * 0.866_025_4,
        ) * self.set.land.site_spacing;
        let jitter = Vec2::new(
            signed(key(&[self.seed, 14, q as u32, r as u32])),
            signed(key(&[self.seed, 15, q as u32, r as u32])),
        ) * 6.0;
        let p = Vec2::from_angle(turn).rotate(grid) + jitter;
        self.clear_of_river(p, self.set.town.radius + 3.0)
    }

    /// The cell whose middle `p` is nearest, jitter aside.
    fn cell_of(&self, p: Vec2) -> Cell {
        let turn = unit(key(&[self.seed, 12])) * TAU;
        let grid = Vec2::from_angle(-turn).rotate(p)
            / self.set.land.site_spacing;
        let r = grid.y / 0.866_025_4;
        let q = grid.x - r * 0.5;
        // Rounded as a cube coordinate, so the nearest hexagon wins.
        let s = -q - r;
        let (mut rq, mut rr, rs) = (q.round(), r.round(), s.round());
        let (dq, dr, ds) =
            ((rq - q).abs(), (rr - r).abs(), (rs - s).abs());
        if dq > dr && dq > ds {
            rq = -rr - rs;
        } else if dr > ds {
            rr = -rq - rs;
        }
        (rq as i32, rr as i32)
    }

    /// The rounds of ground that are held, each with its radius.
    pub fn held(&self) -> Vec<(Vec2, f32)> {
        self.rounds().collect()
    }

    /// The rounds of [`LandData::held`], one by one.
    pub fn rounds(&self) -> impl Iterator<Item = (Vec2, f32)> + '_ {
        self.holds.iter().map(|h| (h.at, h.radius))
    }

    /// True when `p` lies on held ground or within `margin` of it.
    pub fn is_held(&self, p: Vec2, margin: f32) -> bool {
        self.holds
            .iter()
            .any(|h| h.at.distance(p) < h.radius + margin)
    }

    /// Difference of the ground about a hold of `radius` from the
    /// land's own.
    pub(crate) fn hold_reach(radius: f32) -> f32 {
        radius + HOLD_EASE
    }
}
