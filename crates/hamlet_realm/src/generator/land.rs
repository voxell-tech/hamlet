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
    /// The step the two islands' radii are rounded up to. The hamlet
    /// island moves only when an island's radius crosses a step.
    pub island_step: f32,
    /// Open water at least this wide between the two islands' land.
    pub channel: f32,
    /// Distance from the Hand's tower at which the Hearth and the
    /// library first stand.
    pub city_ring: f32,
    /// Side of one tile of ground at its finest.
    pub chunk: f32,
    /// Width of a cell of the finest grid the ground is drawn in, up
    /// close. Everything stands on the ground it draws. A whole
    /// number of them spans a tile.
    pub cell: f32,
    /// The height the plains lie about.
    pub plain_y: f32,
    /// The hills of the plains and the fine grain over them, each as
    /// the scale of its noise and its height.
    pub hills: (f32, f32),
    pub grain: (f32, f32),
    /// The island: how far its land reaches from the middle and from
    /// every town, the wobble of its coast as the scale of its noise
    /// and how far it reaches, and the depth under the water of the
    /// floor off the shore and far out.
    pub island: f32,
    pub coast: (f32, f32),
    pub shelf: f32,
    pub sea_floor: f32,
    /// The least and the most width of the skirt the island's rim
    /// falls to the sea floor in: steep where narrow, gentle where
    /// wide.
    pub rim: (f32, f32),
    /// How far the edges of hills and of the coast are pushed in and
    /// out, so that they wander.
    pub ragged: f32,
    /// Reach of the home plains from the middle. Hills stand only
    /// beyond.
    pub plains: f32,
    /// The wobble of the home plains' edge, as the scale of its
    /// noise and how far it reaches.
    pub edge: (f32, f32),
    /// The hills beyond the plains, each a stroke: the side of the
    /// cells one may stand in and the share of cells with one; the
    /// most length of its line; the least and the most height of its
    /// top, width of its level top about the line, and angle of its
    /// skirt, in degrees.
    pub mounds: (f32, f32),
    pub mound_length: f32,
    pub mound_height: (f32, f32),
    pub mound_size: (f32, f32),
    pub mound_skirt: (f32, f32),
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
    /// the corner of its first cell lies from the middle of its
    /// island.
    pub tree_cell: f32,
    pub tree_origin: f32,
}

/// A cell of the grid the land lays its sites and corridors on.
type Cell = (i32, i32);

/// One of the two islands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IslandId {
    /// Where the scholars live, centred on the origin.
    City,
    /// Where the hamlets stand, across the channel.
    Hamlet,
}

impl IslandId {
    pub const BOTH: [IslandId; 2] =
        [IslandId::City, IslandId::Hamlet];

    pub fn other(self) -> IslandId {
        match self {
            IslandId::City => IslandId::Hamlet,
            IslandId::Hamlet => IslandId::City,
        }
    }
}

/// A piece of land with water all round it.
#[derive(Clone, Debug, PartialEq)]
pub struct Island {
    /// Where it lies. Its noise and its rivers are measured from
    /// here, so that it moves as one.
    pub centre: Vec2,
    /// The points its land reaches about: its centre, then what
    /// stands on it.
    pub shores: Vec<Vec2>,
}

/// Where a point stands on the land: its island, its place measured
/// from that island's centre, and how far inside the coast it lies.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    pub isle: IslandId,
    pub local: Vec2,
    pub inside: f32,
}

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
    /// On a hill.
    Steep,
    /// On top of another town, or of something else that holds its
    /// ground.
    Crowded,
    /// On the island of the other kind of place.
    Elsewhere,
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
    /// cell to the middle of the hamlet island that suits a town,
    /// has no site lying in it and is not `taken`.
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
                    && self.island_at(home) == IslandId::Hamlet
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
        land.reshore();
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

    /// Grows the hamlet island about the sites in `settled`, which
    /// have towns, and lets go of the land about any others.
    pub fn settle(&mut self, settled: &[usize]) {
        if self.settled == settled {
            return;
        }
        let land = Arc::make_mut(&mut self.0);
        land.settled = settled.to_vec();
        land.reshore();
    }

    /// The places the city island is grown about, in the order
    /// [`Land::fill_city`] was given them.
    pub fn city_spots(&self) -> &[Vec2] {
        &self.city
    }

    /// Grows the city island about `spots`, the places of what
    /// stands in the city, and lets go of the land about any others.
    pub fn fill_city(&mut self, spots: &[Vec2]) {
        if self.city == spots {
            return;
        }
        let land = Arc::make_mut(&mut self.0);
        land.city = spots.to_vec();
        land.reshore();
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
    /// The sites of the hamlet island.
    pub sites: Vec<Site>,
    /// The city island, then the hamlet island.
    islands: [Island; 2],
    /// The way from the city island to the hamlet island.
    bearing: Vec2,
    river_dir: Vec2,
    /// The sites that reach into each square of ground.
    near: HashMap<Cell, Vec<u32>>,
    /// Ground held level by something that is not a town.
    holds: Vec<Hold>,
    /// The sites that have a town, which the hamlet island reaches
    /// about.
    settled: Vec<usize>,
    /// The places of what stands in the city, which its island
    /// reaches about.
    city: Vec<Vec2>,
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
        let bearing = Vec2::from_angle(unit(key(&[seed, 16])) * TAU);
        let mut land = Self {
            seed,
            set,
            sites: Vec::new(),
            islands: [(); 2].map(|()| Island {
                centre: Vec2::ZERO,
                shores: vec![Vec2::ZERO],
            }),
            bearing,
            river_dir: Vec2::new(a.cos(), a.sin()),
            near: HashMap::default(),
            holds: Vec::new(),
            settled: Vec::new(),
            city: Vec::new(),
            pal,
        };
        land.islands[1].centre = land.hamlet_goal();
        land.islands[1].shores = vec![land.islands[1].centre];
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
        let centre = self.hamlet_centre();
        cells.sort_by_key(|&(q, r)| {
            let p = self.hex_at(q, r) - centre;
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

    /// The place of a cell's site: a jittered hex grid about the
    /// hamlet island's middle, moved back from the river.
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
        let p = self.hamlet_centre()
            + Vec2::from_angle(turn).rotate(grid)
            + jitter;
        self.clear_of_river(p, self.set.town.radius + 3.0)
    }

    /// The cell whose middle `p` is nearest, jitter aside.
    fn cell_of(&self, p: Vec2) -> Cell {
        let turn = unit(key(&[self.seed, 12])) * TAU;
        let grid = Vec2::from_angle(-turn)
            .rotate(p - self.hamlet_centre())
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

    /// The sites that have a town.
    pub fn settled(&self) -> &[usize] {
        &self.settled
    }

    /// The way the rivers run.
    pub(crate) fn river_dir(&self) -> Vec2 {
        self.river_dir
    }

    /// Each round of held ground: its middle, its radius and the
    /// height it is held at.
    pub(crate) fn hold_levels(
        &self,
    ) -> impl Iterator<Item = Vec4> + '_ {
        self.holds
            .iter()
            .map(|h| Vec4::new(h.at.x, h.at.y, h.radius, h.y))
    }

    /// The island `id`.
    pub fn island(&self, id: IslandId) -> &Island {
        &self.islands[id as usize]
    }

    /// The way from the city island to the hamlet island, as a unit
    /// vector.
    pub fn bearing(&self) -> Vec2 {
        self.bearing
    }

    /// The middle of the hamlet island.
    pub fn hamlet_centre(&self) -> Vec2 {
        self.islands[IslandId::Hamlet as usize].centre
    }

    /// The points either island reaches about.
    pub fn shores(&self) -> impl Iterator<Item = Vec2> + '_ {
        self.islands
            .iter()
            .flat_map(|isle| isle.shores.iter().copied())
    }

    /// How far the land of `id` can lie from its middle: from its
    /// farthest shore, out by the island's reach, the coast's
    /// wobble and the most the smooth joining of the reaches adds.
    pub fn extent(&self, id: IslandId) -> f32 {
        let set = &self.set.land;
        let isle = self.island(id);
        let out = isle
            .shores
            .iter()
            .map(|at| at.distance(isle.centre))
            .fold(0.0, f32::max);
        out + set.island * 1.3 + set.coast.1 + set.ragged
    }

    /// The radius the island `id` counts for in keeping the two
    /// apart: its [`LandData::extent`] rounded up to a step.
    pub fn radius(&self, id: IslandId) -> f32 {
        let step = self.set.land.island_step.max(1.0);
        (self.extent(id) / step).ceil().max(1.0) * step
    }

    /// Where the hamlet island's middle belongs: past the city
    /// island's radius and its own, and the channel, along the
    /// bearing.
    fn hamlet_goal(&self) -> Vec2 {
        let apart = self.radius(IslandId::City)
            + self.radius(IslandId::Hamlet)
            + self.set.land.channel;
        self.islands[IslandId::City as usize].centre
            + self.bearing * apart
    }

    /// Gathers each island's shores again, and pushes the hamlet
    /// island out or back where its radius or the city's has crossed
    /// a step: every site and the island move by one vector.
    fn reshore(&mut self) {
        self.gather();
        let by = self.hamlet_goal() - self.hamlet_centre();
        if by == Vec2::ZERO {
            return;
        }
        for site in &mut self.sites {
            site.at += by;
            site.home += by;
        }
        self.islands[IslandId::Hamlet as usize].centre += by;
        self.index();
        self.gather();
    }

    /// Sets each island's shores to its centre and what stands on
    /// it.
    fn gather(&mut self) {
        let towns = self
            .settled
            .iter()
            .filter_map(|&i| self.sites.get(i))
            .map(|site| site.at);
        let hamlet = std::iter::once(self.hamlet_centre())
            .chain(towns)
            .collect::<Vec<_>>();
        let city =
            std::iter::once(self.island(IslandId::City).centre)
                .chain(self.city.iter().copied())
                .collect::<Vec<_>>();
        self.islands[IslandId::City as usize].shores = city;
        self.islands[IslandId::Hamlet as usize].shores = hamlet;
    }

    /// The island whose land `p` lies nearest.
    pub fn island_at(&self, p: Vec2) -> IslandId {
        self.frame(p).isle
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
