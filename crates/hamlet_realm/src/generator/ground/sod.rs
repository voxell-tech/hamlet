//! The ground mesh of one tile.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use super::{DetailSettings, Tile, is_watered};
use crate::generator::land::Land;
use crate::meshes::{Meshing, smoothstep};

/// The makings of a tile's ground mesh, besides the land and the
/// tiles drawn with it.
#[derive(Clone, Copy, Debug)]
pub struct Recipe {
    pub tile: Tile,
    pub(super) cells: usize,
    /// The width of one cell.
    step: f32,
    /// A coarse tile cannot hold a river's bed, so a river that
    /// runs through it is this much wider on each side. One beside
    /// it is not: it would spread over the tile's edge, where the
    /// tile that holds the river draws it at its own width.
    wide: f32,
    sink: f32,
    /// [`DetailSettings::tile_sink`] and the largest size of tile,
    /// for the recipes of the tiles beside this one.
    sinks: (f32, f32),
    top: u8,
}

impl Recipe {
    pub fn of(
        land: &Land,
        detail: &DetailSettings,
        tile: Tile,
    ) -> Self {
        let top = detail.tile_levels.max(1) - 1;
        Self::new(land, detail.tile_sink, top, tile)
    }

    fn new(
        land: &Land,
        sinks: (f32, f32),
        top: u8,
        tile: Tile,
    ) -> Self {
        let chunk = land.set.land.chunk;
        let side = tile.side(chunk);
        let cells = land.cells(tile.middle(chunk), side);
        let step = side / cells as f32;
        let wide = if is_watered(land, tile) {
            ((step - 3.0) * 0.5).max(0.0)
        } else {
            0.0
        };
        let sink = match tile.level {
            0 => 0.0,
            level => {
                (sinks.0 * (1u32 << (level - 1)) as f32).min(sinks.1)
            }
        };
        Self {
            tile,
            cells,
            step,
            wide,
            sink,
            sinks,
            top,
        }
    }

    /// True when this tile's cells are wider than `other`'s, or as
    /// wide on a larger tile: the two then meet at this one's edge.
    fn is_coarser(&self, other: &Recipe) -> bool {
        let same = (self.step - other.step).abs() < other.step * 1e-3;
        if same {
            self.tile.level > other.tile.level
        } else {
            self.step > other.step
        }
    }

    /// The recipe of another tile drawn with this one.
    fn other(&self, land: &Land, tile: Tile) -> Self {
        Self::new(land, self.sinks, self.top, tile)
    }

    /// The place of the tile's point `(i, j)`.
    fn point(&self, land: &Land, i: usize, j: usize) -> Vec2 {
        self.tile.corner(land.set.land.chunk)
            + Vec2::new(i as f32, j as f32) * self.step
    }

    /// The height a point at `y` is drawn at. Low ground stays
    /// where it is, or the water would spread.
    fn sunk(&self, land: &Land, y: f32) -> f32 {
        let water = land.set.land.water_y;
        y - self.sink * smoothstep(water + 0.3, water + 1.5, y)
    }

    /// The depth the skirt hangs to under a point of the edge at `y`,
    /// on a tile `side` wide: under the lowest dry ground, since the
    /// tile beside it may be of any size and lie far lower on a
    /// slope, and under the edge itself where that is lower still.
    pub(super) fn hem(&self, land: &Land, side: f32, y: f32) -> f32 {
        let floor = land.set.land.water_y - 1.0;
        (y - (1.5 + side * 0.03 + self.sink)).min(floor)
    }
}

/// The ground of a tile as it comes out of a background task.
pub struct Sod {
    pub mesh: Mesh,
    /// True when any of it lies under water.
    pub wet: bool,
}

/// How far a coarse tile is lowered at each of its points, so that
/// its flat triangles lie under the ground as it is drawn up close
/// and nothing standing on that ground is ever covered by a tile
/// seen from far. A point drops only by as much as the triangles
/// about it would rise over the ground between their corners, so a
/// slope stays where it is and tiles of two sizes meet at nearly the
/// same height.
struct Under {
    /// The highest each point may lie, row by row. Empty for the
    /// finest tiles, which are the ground.
    cap: Vec<f32>,
    /// The first point it holds, and how many along `j`.
    from: (usize, usize),
    side: usize,
    bank: f32,
}

impl Under {
    /// Samples to a cell at most. A tile is `cells` of these wide.
    const FINEST: usize = 4;

    /// The drop of the tile's points from `from` to `to`, both
    /// among them.
    fn of(
        land: &Land,
        recipe: &Recipe,
        from: (usize, usize),
        to: (usize, usize),
    ) -> Self {
        // Ground at the water's edge is left alone, or a river
        // would spread over its banks.
        let bank = land.set.land.water_y + 0.3;
        let side = to.1 - from.1 + 1;
        if recipe.tile.level == 0 {
            return Self {
                cap: Vec::new(),
                from,
                side,
                bank,
            };
        }
        let k = (1usize << recipe.tile.level).min(Self::FINEST);
        // One cell more on every side, so that a point on the edge
        // drops as it does on the tile beside.
        let across = (to.0 - from.0 + 2, to.1 - from.1 + 2);
        let n = (across.0 * k + 1, across.1 * k + 1);
        let fine = recipe.step / k as f32;
        let start = recipe.point(land, from.0, from.1)
            - Vec2::splat(recipe.step);
        let mut field = vec![0.0f32; n.0 * n.1];
        for i in 0..n.0 {
            for j in 0..n.1 {
                let p = start + Vec2::new(i as f32, j as f32) * fine;
                field[i * n.1 + j] =
                    land.height_wide(p, recipe.wide).max(bank);
            }
        }
        let corner = |i: usize, j: usize| field[i * k * n.1 + j * k];
        let mut cap = vec![f32::MAX; (across.0 - 1) * side];
        let mut samples = Vec::with_capacity((k + 1) * (k + 1));
        for i in 0..across.0 {
            for j in 0..across.1 {
                let (h00, h01, h10, h11) = (
                    corner(i, j),
                    corner(i, j + 1),
                    corner(i + 1, j),
                    corner(i + 1, j + 1),
                );
                // Each sample of the cell: how far the cell's two
                // triangles and the ground lie over its lowest
                // ground there.
                samples.clear();
                let lowest = (0..=k)
                    .flat_map(|a| (0..=k).map(move |b| (a, b)))
                    .map(|(a, b)| {
                        field[(i * k + a) * n.1 + j * k + b]
                    })
                    .fold(f32::MAX, f32::min);
                for a in 0..=k {
                    for b in 0..=k {
                        let y = field[(i * k + a) * n.1 + j * k + b];
                        let (fx, fy) = (
                            a as f32 / k as f32,
                            b as f32 / k as f32,
                        );
                        // A quad is cut from its first corner to
                        // the one across.
                        let flat = if fy >= fx {
                            h00 + (h11 - h01) * fx + (h01 - h00) * fy
                        } else {
                            h00 + (h10 - h00) * fx + (h11 - h10) * fy
                        };
                        samples.push((flat - lowest, y - lowest));
                    }
                }
                // With its corners brought `flatten` of the way
                // down to the cell's lowest ground, the triangles
                // still rise this far over the ground.
                let over = |flatten: f32| {
                    samples
                        .iter()
                        .map(|(flat, y)| flat * (1.0 - flatten) - y)
                        .fold(0.0f32, f32::max)
                };
                // Dropping the corners by that keeps the triangles
                // under the ground. Where it would take the lowest
                // corner under the bank, the corners are flattened
                // until it does not: the cell's lowest ground is
                // never under the bank. A finger under it is let
                // by, so that the rounding of the samples does not
                // decide.
                let low = h00.min(h01).min(h10).min(h11) - lowest;
                let dry = |flatten: f32| {
                    lowest + low * (1.0 - flatten) - over(flatten)
                        >= bank - 0.01
                };
                let mut flatten = 0.0;
                if !dry(0.0) {
                    let mut wet = 0.0;
                    flatten = 1.0;
                    for _ in 0..12 {
                        let mid = (wet + flatten) * 0.5;
                        if dry(mid) {
                            flatten = mid;
                        } else {
                            wet = mid;
                        }
                    }
                }
                let drop = over(flatten);
                for (di, dj, h) in [
                    (0, 0, h00),
                    (0, 1, h01),
                    (1, 0, h10),
                    (1, 1, h11),
                ] {
                    // The points it holds are one cell in.
                    let (vi, vj) = (i + di, j + dj);
                    if !(1..across.0).contains(&vi)
                        || !(1..across.1).contains(&vj)
                    {
                        continue;
                    }
                    let at = (vi - 1) * side + vj - 1;
                    let most = lowest
                        + (h - lowest) * (1.0 - flatten)
                        - drop;
                    cap[at] = cap[at].min(most);
                }
            }
        }
        Self {
            cap,
            from,
            side,
            bank,
        }
    }

    /// The height of the tile's point `(i, j)`, whose own is `y`.
    fn at(&self, i: usize, j: usize, y: f32) -> f32 {
        if self.cap.is_empty() || y < self.bank {
            return y;
        }
        let at = (i - self.from.0) * self.side + j - self.from.1;
        y.min(self.cap[at])
    }
}

/// Cuts the rim of a tile to the coarser tiles drawn beside it, so
/// that the two meet with no step: where tiles of two grains touch,
/// the finer takes its height from the edge of the coarser.
struct Stitch<'a> {
    land: &'a Land,
    drawn: &'a HashSet<Tile>,
    /// The heights found so far, by tile and point.
    known: HashMap<(Tile, usize, usize), f32>,
    /// The recipes of the tiles beside those met so far.
    beside: HashMap<(Tile, (i32, i32)), Option<Recipe>>,
}

impl<'a> Stitch<'a> {
    fn new(land: &'a Land, drawn: &'a HashSet<Tile>) -> Self {
        Self {
            land,
            drawn,
            known: HashMap::new(),
            beside: HashMap::new(),
        }
    }

    /// The coarsest of the tiles that touch the rim point `(i, j)`
    /// of `recipe`'s tile, unless that is the tile itself.
    fn master(
        &mut self,
        recipe: &Recipe,
        i: usize,
        j: usize,
    ) -> Option<Recipe> {
        let side = |at: usize| match at {
            0 => -1,
            at if at == recipe.cells => 1,
            _ => 0,
        };
        let (dx, dz) = (side(i), side(j));
        let (land, drawn) = (self.land, self.drawn);
        let mut best: Option<Recipe> = None;
        for by in [(dx, 0), (0, dz), (dx, dz)] {
            if by == (0, 0) {
                continue;
            }
            let other = *self
                .beside
                .entry((recipe.tile, by))
                .or_insert_with(|| {
                    recipe
                        .tile
                        .beside(drawn, recipe.top, by)
                        .map(|tile| recipe.other(land, tile))
                });
            if let Some(other) = other
                && other.is_coarser(best.as_ref().unwrap_or(recipe))
            {
                best = Some(other);
            }
        }
        best
    }

    /// The height the rim point `(i, j)` of `recipe`'s tile is
    /// drawn at.
    fn height(&mut self, recipe: Recipe, i: usize, j: usize) -> f32 {
        if let Some(y) = self.known.get(&(recipe.tile, i, j)) {
            return *y;
        }
        let y = match self.master(&recipe, i, j) {
            Some(master) => self.along(master, &recipe, i, j),
            None => {
                let land = self.land;
                let under = Under::of(land, &recipe, (i, j), (i, j));
                let p = recipe.point(land, i, j);
                let y = land.height_wide(p, recipe.wide);
                recipe.sunk(land, under.at(i, j, y))
            }
        };
        self.known.insert((recipe.tile, i, j), y);
        y
    }

    /// The height of the edge of `master`'s tile where the rim
    /// point `(i, j)` of `recipe`'s tile lies on it.
    fn along(
        &mut self,
        master: Recipe,
        recipe: &Recipe,
        i: usize,
        j: usize,
    ) -> f32 {
        // The point in the master's cells, from the tiles' places
        // and not from the ground's, so that far from the middle
        // it is as exact.
        let cells = master.cells as f64;
        let grow = f64::from(1u32 << recipe.tile.level)
            / f64::from(1u32 << master.tile.level);
        let place = |tile: i32, at: usize, of: i32| {
            let own =
                f64::from(tile) + at as f64 / recipe.cells as f64;
            ((own * grow - f64::from(of)) * cells).clamp(0.0, cells)
        };
        let x = place(recipe.tile.x, i, master.tile.x);
        let z = place(recipe.tile.z, j, master.tile.z);
        let on = |at: f64| (at - at.round()).abs() < 1e-6;
        let whole = |at: f64| at.round() as usize;
        // It lies on one of the master's edges: at a point of it or
        // between two.
        if on(x) && on(z) {
            self.height(master, whole(x), whole(z))
        } else if on(x) {
            let (a, t) = (z.floor(), z.fract() as f32);
            let from = self.height(master, whole(x), a as usize);
            let to = self.height(master, whole(x), a as usize + 1);
            from + (to - from) * t
        } else {
            let (a, t) = (x.floor(), x.fract() as f32);
            let from = self.height(master, a as usize, whole(z));
            let to = self.height(master, a as usize + 1, whole(z));
            from + (to - from) * t
        }
    }
}

/// The ground of one tile, its rim cut to the coarser of the
/// `drawn` tiles beside it.
pub fn sod(
    land: &Land,
    recipe: Recipe,
    drawn: &HashSet<Tile>,
) -> Sod {
    let cells = recipe.cells;
    let water = land.set.land.water_y;
    // A coarse tile's normals are those of the ground at its grain.
    let grain = if recipe.tile.level == 0 {
        0.4
    } else {
        recipe.step * 0.5
    };
    let mut meshing = Meshing::new();
    let mut ids = vec![vec![0u32; cells + 1]; cells + 1];
    let mut rim = Vec::new();
    let mut wet = false;
    let under = Under::of(land, &recipe, (0, 0), (cells, cells));
    let mut stitch = Stitch::new(land, drawn);
    for (i, row) in ids.iter_mut().enumerate() {
        for (j, id) in row.iter_mut().enumerate() {
            let p = recipe.point(land, i, j);
            let high = land.height_wide(p, recipe.wide);
            let y = under.at(i, j, high);
            wet |= y < water + 0.15;
            let n = land.normal_wide(p, grain, recipe.wide);
            // Coloured as the ground it stands for, not as the
            // height it is lowered to.
            let colour = land.ground_colour(p, high, n);
            let edge = i == 0 || j == 0 || i == cells || j == cells;
            let sunk = match edge
                .then(|| stitch.master(&recipe, i, j))
                .flatten()
            {
                Some(master) => stitch.along(master, &recipe, i, j),
                None => recipe.sunk(land, y),
            };
            let at = Vec3::new(p.x, sunk, p.y);
            *id = meshing.v(at, n, colour);
            if edge {
                rim.push(((i, j), at, n, colour));
            }
        }
    }
    for i in 0..cells {
        for j in 0..cells {
            meshing.quad(
                ids[i][j],
                ids[i][j + 1],
                ids[i + 1][j + 1],
                ids[i + 1][j],
            );
        }
    }
    // A skirt hides the seam against a neighbour of another
    // fineness. It is lit as the ground above it, so that it does
    // not show as a line, and faces both ways.
    let side = recipe.tile.side(land.set.land.chunk);
    let hem = |meshing: &mut Meshing, (i, j): (usize, usize)| {
        rim.iter()
            .find(|(at, ..)| *at == (i, j))
            .map(|&(_, at, n, colour)| {
                let low = recipe.hem(land, side, at.y);
                meshing.v(Vec3::new(at.x, low, at.z), n, colour)
            })
            .unwrap()
    };
    for k in 0..cells {
        for (a, b) in [
            ((k, 0), (k + 1, 0)),
            ((k, cells), (k + 1, cells)),
            ((0, k), (0, k + 1)),
            ((cells, k), (cells, k + 1)),
        ] {
            let top = (ids[a.0][a.1], ids[b.0][b.1]);
            let low = (hem(&mut meshing, a), hem(&mut meshing, b));
            meshing.quad(top.0, top.1, low.1, low.0);
            meshing.quad(top.1, top.0, low.0, low.1);
        }
    }
    Sod {
        mesh: meshing.build(),
        wet,
    }
}
