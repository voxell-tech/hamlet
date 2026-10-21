//! The ground mesh of one tile, built a row at a time.

use std::collections::{HashMap, HashSet};

use glam::{Vec2, Vec3};
use hamlet_engine_sdk::scene::MeshData;

use super::{DetailSettings, Tile, watered};
use crate::land::Land;
use crate::mesh::{Mb, smoothstep};

/// The makings of a tile's ground mesh, besides the land and the
/// tiles drawn with it.
#[derive(Clone, Copy, Debug)]
pub struct Recipe {
    pub tile: Tile,
    cells: usize,
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
        let wide = if watered(land, tile) {
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
    fn coarser(&self, other: &Recipe) -> bool {
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

    /// The place of the tile's point `(i, j)`, which may lie a cell
    /// outside it.
    fn point(&self, land: &Land, i: i32, j: i32) -> Vec2 {
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
    fn hem(&self, land: &Land, side: f32, y: f32) -> f32 {
        let floor = land.set.land.water_y - 1.0;
        (y - (1.5 + side * 0.03 + self.sink)).min(floor)
    }
}

/// Cuts the rim of a tile to the coarser tiles drawn beside it, so
/// that the two meet with no step: where tiles of two grains touch,
/// the finer takes its height from the edge of the coarser.
#[derive(Default)]
struct Stitch {
    /// The heights found so far, by tile and point.
    known: HashMap<(Tile, usize, usize), f32>,
    /// The recipes of the tiles beside those met so far.
    beside: HashMap<(Tile, (i32, i32)), Option<Recipe>>,
}

impl Stitch {
    /// The coarsest of the tiles that touch the rim point `(i, j)`
    /// of `recipe`'s tile, unless that is the tile itself.
    fn master(
        &mut self,
        land: &Land,
        drawn: &HashSet<Tile>,
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
                && other.coarser(best.as_ref().unwrap_or(recipe))
            {
                best = Some(other);
            }
        }
        best
    }

    /// The height the rim point `(i, j)` of `recipe`'s tile is
    /// drawn at.
    fn height(
        &mut self,
        land: &Land,
        drawn: &HashSet<Tile>,
        recipe: Recipe,
        i: usize,
        j: usize,
    ) -> f32 {
        if let Some(y) = self.known.get(&(recipe.tile, i, j)) {
            return *y;
        }
        let y = match self.master(land, drawn, &recipe, i, j) {
            Some(master) => {
                self.along(land, drawn, master, &recipe, i, j)
            }
            None => {
                let p = recipe.point(land, i as i32, j as i32);
                recipe.sunk(land, land.height_wide(p, recipe.wide))
            }
        };
        self.known.insert((recipe.tile, i, j), y);
        y
    }

    /// The height of the edge of `master`'s tile where the rim
    /// point `(i, j)` of `recipe`'s tile lies on it.
    fn along(
        &mut self,
        land: &Land,
        drawn: &HashSet<Tile>,
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
        let mut height = |i: usize, j: usize| {
            self.height(land, drawn, master, i, j)
        };
        // It lies on one of the master's edges: at a point of it or
        // between two.
        if on(x) && on(z) {
            height(whole(x), whole(z))
        } else if on(x) {
            let (a, t) = (z.floor(), z.fract() as f32);
            let from = height(whole(x), a as usize);
            let to = height(whole(x), a as usize + 1);
            from + (to - from) * t
        } else {
            let (a, t) = (x.floor(), x.fract() as f32);
            let from = height(a as usize, whole(z));
            let to = height(a as usize + 1, whole(z));
            from + (to - from) * t
        }
    }
}

/// A point of a tile's rim: its place in the tile, where it is
/// drawn, its normal and its colour.
type RimPoint = ((usize, usize), Vec3, Vec3, Vec3);

/// The ground of one tile being built: first the heights of its
/// points and a ring of points about them, a row at a time, then its
/// vertices a row at a time, then its triangles and skirt.
pub struct SodJob {
    recipe: Recipe,
    /// The tiles drawn with it, its rim cut to the coarser of them.
    drawn: HashSet<Tile>,
    /// The heights of the points from a cell outside the tile to a
    /// cell outside its far side, row by row, as many rows as found.
    heights: Vec<f32>,
    /// Rows of vertices made.
    rows: usize,
    mb: Mb,
    ids: Vec<u32>,
    rim: Vec<RimPoint>,
    wet: bool,
    stitch: Stitch,
    /// The ground built, while it waits for room to be handed over.
    ready: Option<Sod>,
}

/// A tile's ground, built.
pub struct Sod {
    pub mesh: MeshData,
    /// True when any of it lies under water.
    pub wet: bool,
}

impl SodJob {
    pub fn new(recipe: Recipe, drawn: &HashSet<Tile>) -> Self {
        Self {
            recipe,
            drawn: drawn.clone(),
            heights: Vec::new(),
            rows: 0,
            mb: Mb::new(),
            ids: Vec::new(),
            rim: Vec::new(),
            wet: false,
            stitch: Stitch::default(),
            ready: None,
        }
    }

    /// Keeps the ground built until there is room to hand it over.
    pub fn wait(&mut self, sod: Sod) {
        self.ready = Some(sod);
    }

    /// Points along a side of the heights found, the ring included.
    fn span(&self) -> usize {
        self.recipe.cells + 3
    }

    /// The height of the point `(i, j)`, which may lie a cell outside
    /// the tile.
    fn high(&self, i: i32, j: i32) -> f32 {
        let span = self.span();
        self.heights[(i + 1) as usize * span + (j + 1) as usize]
    }

    /// One row more of the tile's work; the ground once it is done.
    pub fn step(&mut self, land: &Land) -> Option<Sod> {
        if let Some(sod) = self.ready.take() {
            return Some(sod);
        }
        let span = self.span();
        if self.heights.len() < span * span {
            let i = (self.heights.len() / span) as i32 - 1;
            let recipe = self.recipe;
            self.heights.extend((0..span).map(|j| {
                let p = recipe.point(land, i, j as i32 - 1);
                land.height_wide(p, recipe.wide)
            }));
            return None;
        }
        if self.rows <= self.recipe.cells {
            self.row(land);
            return None;
        }
        Some(self.finish(land))
    }

    /// The vertices of the next row of points.
    fn row(&mut self, land: &Land) {
        let recipe = self.recipe;
        let cells = recipe.cells;
        let water = land.set.land.water_y;
        let i = self.rows;
        for j in 0..=cells {
            let (si, sj) = (i as i32, j as i32);
            let p = recipe.point(land, si, sj);
            let high = self.high(si, sj);
            self.wet |= high < water + 0.15;
            // The normal of the ground across the points beside it.
            let n = Vec3::new(
                self.high(si - 1, sj) - self.high(si + 1, sj),
                2.0 * recipe.step,
                self.high(si, sj - 1) - self.high(si, sj + 1),
            )
            .normalize();
            // Coloured as the ground it stands for, not as the
            // height it is lowered to.
            let colour = land.ground_colour(p, high, n);
            let edge = i == 0 || j == 0 || i == cells || j == cells;
            let master = edge
                .then(|| {
                    self.stitch.master(
                        land,
                        &self.drawn,
                        &recipe,
                        i,
                        j,
                    )
                })
                .flatten();
            let sunk = match master {
                Some(master) => self.stitch.along(
                    land,
                    &self.drawn,
                    master,
                    &recipe,
                    i,
                    j,
                ),
                None => recipe.sunk(land, high),
            };
            let at = Vec3::new(p.x, sunk, p.y);
            self.ids.push(self.mb.v(at, n, colour));
            if edge {
                self.rim.push(((i, j), at, n, colour));
            }
        }
        self.rows += 1;
    }

    /// The triangles of the tile, and the skirt under its rim.
    fn finish(&mut self, land: &Land) -> Sod {
        let recipe = self.recipe;
        let cells = recipe.cells;
        let mut mb = std::mem::take(&mut self.mb);
        let id = |i: usize, j: usize| self.ids[i * (cells + 1) + j];
        for i in 0..cells {
            for j in 0..cells {
                mb.quad(
                    id(i, j),
                    id(i, j + 1),
                    id(i + 1, j + 1),
                    id(i + 1, j),
                );
            }
        }
        // A skirt hides the seam against a neighbour of another
        // fineness. It is lit as the ground above it, so that it does
        // not show as a line, and faces both ways.
        let side = recipe.tile.side(land.set.land.chunk);
        let rim = &self.rim;
        let hem = |mb: &mut Mb, (i, j): (usize, usize)| {
            rim.iter()
                .find(|(at, ..)| *at == (i, j))
                .map(|&(_, at, n, colour)| {
                    let low = recipe.hem(land, side, at.y);
                    mb.v(Vec3::new(at.x, low, at.z), n, colour)
                })
                .unwrap_or_default()
        };
        for k in 0..cells {
            for (a, b) in [
                ((k, 0), (k + 1, 0)),
                ((k, cells), (k + 1, cells)),
                ((0, k), (0, k + 1)),
                ((cells, k), (cells, k + 1)),
            ] {
                let top = (id(a.0, a.1), id(b.0, b.1));
                let low = (hem(&mut mb, a), hem(&mut mb, b));
                mb.quad(top.0, top.1, low.1, low.0);
                mb.quad(top.1, top.0, low.0, low.1);
            }
        }
        Sod {
            mesh: mb.data(),
            wet: self.wet,
        }
    }
}
