//! The corridors roads between towns keep to, and the way through
//! them from one town to another.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::f32::consts::TAU;

use glam::Vec2;

use crate::build::mesh::smoothstep;
use crate::world::land::{
    Cell, HEX_DIRS, HOLD_CLEAR, LandData, hex_ring,
};
use crate::world::noise::perlin;

/// A point where corridors meet: between two cells, or where three
/// touch. The cells are in order.
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug,
)]
enum Node {
    Mid([Cell; 2]),
    Corner([Cell; 3]),
}

impl Node {
    fn mid(a: Cell, b: Cell) -> Self {
        let mut cells = [a, b];
        cells.sort();
        Node::Mid(cells)
    }

    fn corner(a: Cell, b: Cell, c: Cell) -> Self {
        let mut cells = [a, b, c];
        cells.sort();
        Node::Corner(cells)
    }

    fn cells(&self) -> &[Cell] {
        match self {
            Node::Mid(cells) => cells,
            Node::Corner(cells) => cells,
        }
    }

    /// The nodes a corridor runs to from here.
    fn links(self) -> Vec<Node> {
        match self {
            Node::Mid([a, b]) => HEX_DIRS
                .iter()
                .map(|&(dq, dr)| (a.0 + dq, a.1 + dr))
                .filter(|&n| {
                    n != b && hex_ring(n.0 - b.0, n.1 - b.1) == 1
                })
                .map(|n| Node::corner(a, b, n))
                .collect(),
            Node::Corner([a, b, c]) => {
                vec![
                    Node::mid(a, b),
                    Node::mid(b, c),
                    Node::mid(a, c),
                ]
            }
        }
    }
}

/// The nodes around one cell: the middle of each side and each
/// corner.
fn around(c: Cell) -> impl Iterator<Item = Node> {
    let ring = HEX_DIRS.map(|(dq, dr)| (c.0 + dq, c.1 + dr));
    (0..6).flat_map(move |i| {
        [
            Node::mid(c, ring[i]),
            Node::corner(c, ring[i], ring[(i + 1) % 6]),
        ]
    })
}

/// Distance from `p` to the segment from `a` to `b`.
fn segment_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6))
        .clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

impl LandData {
    /// Bends a corridor point so no corridor runs straight. It fades
    /// out toward towns, where routes dodge houses instead.
    pub(crate) fn warp(&self, p: Vec2) -> Vec2 {
        let ds = self.site_dist(p);
        let fade = smoothstep(
            self.set.town.gate_radius + 1.0,
            self.set.town.gate_radius + 12.0,
            ds,
        );
        p + Vec2::new(
            perlin(p * 0.021, self.seed + 90),
            perlin(p * 0.021, self.seed + 91),
        ) * 6.5
            * fade
    }

    /// The point roads leave the town at `site` by, toward `toward`.
    fn gate(&self, site: usize, toward: Vec2) -> Vec2 {
        let c = self.sites[site].at;
        let d = toward - c;
        c + d.normalize_or_zero()
            * self.set.town.gate_radius.min(d.length() * 0.92)
    }

    /// A search for the corridor between the towns at `from` and
    /// `to`, taken a few crossings at a time.
    pub(crate) fn search(&self, from: usize, to: usize) -> Search {
        // A corridor is found from the lower site, so that the two
        // ways along it are the same points.
        let back = from > to;
        let (from, to) = if back { (to, from) } else { (from, to) };
        let mut search = Search {
            from,
            to,
            back,
            goal: self.sites[to].at,
            ends: HashSet::new(),
            reached: HashMap::new(),
            open: BinaryHeap::new(),
            homes: HashMap::new(),
            at: HashMap::new(),
            looked: 0,
        };
        search.ends = self
            .ways_out(to, &mut search.homes)
            .into_iter()
            .map(|(node, _)| node)
            .collect();
        for (node, length) in self.ways_out(from, &mut search.homes) {
            search.reached.insert(node, (length, None));
            let p = search.place(self, node);
            search
                .open
                .push(rank(length + p.distance(search.goal), node));
        }
        search
    }

    /// The corridor between the towns at `from` and `to` through the
    /// crossings `nodes`: from one gate to the other, sampled every
    /// few units and bent by `Land::warp`. Shared stretches of two
    /// corridors are the same points.
    fn corridor(
        &self,
        from: usize,
        to: usize,
        nodes: &[Vec2],
    ) -> Vec<Vec2> {
        let (a, b) = (self.sites[from].at, self.sites[to].at);
        let mut way = vec![
            self.gate(from, nodes.first().copied().unwrap_or(b)),
        ];
        way.extend_from_slice(nodes);
        way.push(self.gate(to, nodes.last().copied().unwrap_or(a)));
        let mut pts = Vec::new();
        for w in way.windows(2) {
            let n = (w[0].distance(w[1]) / 3.0).ceil().max(1.0);
            for k in 0..n as usize {
                pts.push(w[0].lerp(w[1], k as f32 / n));
            }
        }
        pts.push(*way.last().unwrap());
        // Round the corners. Straight runs are unchanged, so
        // corridors that share a stretch still agree on it.
        for _ in 0..2 {
            let mut cut = vec![pts[0]];
            for w in pts.windows(2) {
                cut.push(w[0].lerp(w[1], 0.25));
                cut.push(w[0].lerp(w[1], 0.75));
            }
            cut.push(*pts.last().unwrap());
            pts = cut;
        }
        let last = *pts.last().unwrap();
        let mut out = pts
            .into_iter()
            .step_by(2)
            .map(|p| self.warp(p))
            .collect::<Vec<_>>();
        if out.last() != Some(&self.warp(last)) {
            out.push(self.warp(last));
        }
        self.round_holds(out)
    }

    /// `way` with each stretch that crosses held ground taken
    /// round it instead, by the shorter side. The ends stay.
    fn round_holds(&self, mut way: Vec<Vec2>) -> Vec<Vec2> {
        for hold in &self.holds {
            let clear = hold.radius + HOLD_CLEAR;
            let within = |p: &Vec2| p.distance(hold.at) < clear;
            // The first and the last point on it, the ends aside.
            let inner = 1..way.len().saturating_sub(1);
            let Some(first) =
                inner.clone().find(|&i| within(&way[i]))
            else {
                continue;
            };
            let last =
                inner.rev().find(|&i| within(&way[i])).unwrap();
            let angle = |p: Vec2| (p - hold.at).to_angle();
            let (from, to) =
                (angle(way[first - 1]), angle(way[last + 1]));
            // The turn from one to the other, the shorter way.
            let turn =
                (to - from + TAU * 0.5).rem_euclid(TAU) - TAU * 0.5;
            let steps = (turn.abs() * clear / 3.0).ceil().max(2.0);
            let arc = (0..=steps as usize).map(|k| {
                let at = from + turn * (k as f32 / steps);
                hold.at + Vec2::from_angle(at) * clear
            });
            way.splice(first..=last, arc);
        }
        way
    }

    /// The place of a node: the middle of its cells' sites, clear of
    /// the river.
    fn node_at(
        &self,
        node: Node,
        homes: &mut HashMap<Cell, Vec2>,
    ) -> Vec2 {
        let cells = node.cells();
        let sum = cells
            .iter()
            .map(|&c| {
                *homes
                    .entry(c)
                    .or_insert_with(|| self.hex_at(c.0, c.1))
            })
            .sum::<Vec2>();
        self.clear_of_river(sum / cells.len() as f32, 10.0)
    }

    /// The nodes a town's roads may leave it for, each with the
    /// length of the way there. A town on its own cell leaves by the
    /// middle of each of the cell's sides. One that was moved leaves
    /// for the nearest nodes it has a clear way to.
    fn ways_out(
        &self,
        site: usize,
        homes: &mut HashMap<Cell, Vec2>,
    ) -> Vec<(Node, f32)> {
        let here = &self.sites[site];
        if !here.moved() {
            let c = here.cell;
            return HEX_DIRS
                .iter()
                .map(|&(dq, dr)| Node::mid(c, (c.0 + dq, c.1 + dr)))
                .map(|node| {
                    (
                        node,
                        here.at.distance(self.node_at(node, homes)),
                    )
                })
                .collect();
        }
        let town = &self.set.town;
        let c = self.cell_of(here.at);
        let mut cells = vec![c];
        cells.extend(HEX_DIRS.map(|(dq, dr)| (c.0 + dq, c.1 + dr)));
        let mut out = cells
            .into_iter()
            .flat_map(around)
            .map(|node| (node, self.node_at(node, homes)))
            .filter(|&(_, at)| {
                here.at.distance(at) > town.gate_radius + 6.0
                    && !self.blocked(at)
                    && !self.sites.iter().enumerate().any(|(i, s)| {
                        i != site
                            && segment_dist(s.at, here.at, at)
                                < town.radius + 2.0
                    })
            })
            .map(|(node, at)| (node, here.at.distance(at)))
            .collect::<Vec<_>>();
        out.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        out.dedup_by_key(|way| way.0);
        out.truncate(4);
        out
    }

    /// True when a moved town stands where a corridor would pass `p`.
    fn blocked(&self, p: Vec2) -> bool {
        self.sites.iter().any(|s| {
            s.moved() && s.at.distance(p) < self.set.town.radius + 6.0
        })
    }

    /// True when a moved town stands in the way from `a` to `b`.
    fn barred(&self, a: Vec2, b: Vec2) -> bool {
        self.sites.iter().any(|s| {
            s.moved()
                && segment_dist(s.at, a, b)
                    < self.set.town.radius + 2.0
        })
    }
}

/// The rank of a crossing to look at, the nearest by its way first.
/// Lengths are never negative, so their bits sort as they do.
fn rank(length: f32, node: Node) -> (Reverse<u32>, Reverse<Node>) {
    (Reverse(length.to_bits()), Reverse(node))
}

/// A search for the shortest way through the corridors between two
/// sites, and the corridor along it.
pub(crate) struct Search {
    from: usize,
    to: usize,
    /// True when the corridor is wanted from `to` to `from`.
    back: bool,
    goal: Vec2,
    /// The crossings the way may end at.
    ends: HashSet<Node>,
    /// Each crossing reached: the length of the way to it and the
    /// crossing before it.
    reached: HashMap<Node, (f32, Option<Node>)>,
    open: BinaryHeap<(Reverse<u32>, Reverse<Node>)>,
    /// The places of the cells' sites, and of the crossings.
    homes: HashMap<Cell, Vec2>,
    at: HashMap<Node, Vec2>,
    looked: usize,
}

impl Search {
    fn place(&mut self, land: &LandData, node: Node) -> Vec2 {
        if let Some(&p) = self.at.get(&node) {
            return p;
        }
        let p = land.node_at(node, &mut self.homes);
        self.at.insert(node, p);
        p
    }

    /// Looks at up to `most` more crossings. The corridor once the
    /// search is over: along the shortest way, or straight from gate
    /// to gate when none is found within the land's `search`.
    pub(crate) fn step(
        &mut self,
        land: &LandData,
        most: usize,
    ) -> Option<Vec<Vec2>> {
        for _ in 0..most {
            let Some((_, Reverse(node))) = self.open.pop() else {
                return Some(self.corridor(land, None));
            };
            if self.ends.contains(&node) {
                return Some(self.corridor(land, Some(node)));
            }
            self.looked += 1;
            if self.looked > land.set.road.search {
                return Some(self.corridor(land, None));
            }
            let so_far = self.reached.get(&node).map_or(0.0, |r| r.0);
            let p = self.place(land, node);
            for next in node.links() {
                let q = self.place(land, next);
                if land.blocked(q) || land.barred(p, q) {
                    continue;
                }
                // Roads keep to low ground where they can.
                let climb = 1.0
                    + land.set.road.climb
                        * land.mountain((p + q) * 0.5);
                let length = so_far + p.distance(q) * climb;
                if self
                    .reached
                    .get(&next)
                    .is_none_or(|r| length < r.0)
                {
                    self.reached.insert(next, (length, Some(node)));
                    self.open.push(rank(
                        length + q.distance(self.goal),
                        next,
                    ));
                }
            }
        }
        None
    }

    /// The corridor through the way that ends at `last`.
    fn corridor(
        &mut self,
        land: &LandData,
        last: Option<Node>,
    ) -> Vec<Vec2> {
        let mut path = Vec::new();
        let mut node = last;
        while let Some(at) = node {
            path.push(at);
            node = self.reached.get(&at).and_then(|r| r.1);
        }
        let nodes = path
            .into_iter()
            .rev()
            .map(|node| self.place(land, node))
            .collect::<Vec<_>>();
        let mut corridor = land.corridor(self.from, self.to, &nodes);
        if self.back {
            corridor.reverse();
        }
        corridor
    }
}
