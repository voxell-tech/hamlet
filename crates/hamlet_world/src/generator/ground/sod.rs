//! The ground mesh of one tile.

use bevy::prelude::*;

use super::{DetailSettings, Tile, watered};
use crate::generator::land::Land;
use crate::meshes::{Mb, smoothstep};

/// The makings of a tile's ground mesh, besides the land.
#[derive(Clone, Copy, Debug)]
pub struct Recipe {
    pub tile: Tile,
    pub(super) cells: usize,
    sink: f32,
}

impl Recipe {
    pub fn of(
        land: &Land,
        detail: &DetailSettings,
        tile: Tile,
    ) -> Self {
        let chunk = land.set.land.chunk;
        let cells = land.cells(tile.middle(chunk), tile.side(chunk));
        let sink = match tile.level {
            0 => 0.0,
            level => (detail.tile_sink.0
                * (1u32 << (level - 1)) as f32)
                .min(detail.tile_sink.1),
        };
        Self { tile, cells, sink }
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

/// The lowest the ground gets about each of the tile's points. It
/// keeps a coarse tile under the ground as it is drawn up close, so
/// that nothing standing on that ground is ever covered by a tile
/// seen from far.
struct Under {
    /// The lowest dry ground within a cell of each point, row by
    /// row. Empty for the finest tiles, which are the ground.
    low: Vec<f32>,
    side: usize,
    bank: f32,
}

impl Under {
    /// Samples to a cell at most. A tile is `cells` of these wide.
    const FINEST: usize = 4;

    fn of(
        land: &Land,
        recipe: &Recipe,
        lo: Vec2,
        step: f32,
        wide: f32,
    ) -> Self {
        // Ground at the water's edge is left alone, or a river
        // would spread over its banks.
        let bank = land.set.land.water_y + 0.3;
        let side = recipe.cells + 1;
        if recipe.tile.level == 0 {
            return Self {
                low: Vec::new(),
                side,
                bank,
            };
        }
        let k = (1usize << recipe.tile.level).min(Self::FINEST);
        // One cell more on every side.
        let n = (recipe.cells + 2) * k + 1;
        let fine = step / k as f32;
        let start = lo - Vec2::splat(step);
        let mut field = vec![0.0f32; n * n];
        for i in 0..n {
            for j in 0..n {
                let p = start + Vec2::new(i as f32, j as f32) * fine;
                field[i * n + j] =
                    land.height_wide(p, wide).max(bank);
            }
        }
        // The lowest within a cell either way, along one axis and
        // then the other.
        let lowest = |from: &[f32], stride: usize, run: usize| {
            let mut out = vec![0.0f32; n * n];
            for line in 0..n {
                for at in 0..n {
                    let (a, b) =
                        (at.saturating_sub(k), (at + k).min(n - 1));
                    out[line * run + at * stride] = (a..=b)
                        .map(|q| from[line * run + q * stride])
                        .fold(f32::MAX, f32::min);
                }
            }
            out
        };
        let rows = lowest(&field, 1, n);
        let both = lowest(&rows, n, 1);
        let low = (0..side * side)
            .map(|v| {
                let (i, j) = (v / side + 1, v % side + 1);
                both[i * k * n + j * k]
            })
            .collect();
        Self { low, side, bank }
    }

    /// The height of the tile's point `(i, j)`, whose own is `y`.
    fn at(&self, i: usize, j: usize, y: f32) -> f32 {
        if self.low.is_empty() || y < self.bank {
            return y;
        }
        y.min(self.low[i * self.side + j])
    }
}

/// The ground of one tile.
pub fn sod(land: &Land, recipe: Recipe) -> Sod {
    let chunk = land.set.land.chunk;
    let (lo, side) =
        (recipe.tile.corner(chunk), recipe.tile.side(chunk));
    let cells = recipe.cells;
    let step = side / cells as f32;
    let water = land.set.land.water_y;
    // A coarse tile cannot hold a river's bed, so its rivers are
    // wider, and its normals are those of the ground at its grain.
    // Only a river that runs through the tile is widened: one
    // beside it would spread over the tile's edge, where the tile
    // that holds the river draws it at its own width.
    let wide = if watered(land, recipe.tile) {
        ((step - 3.0) * 0.5).max(0.0)
    } else {
        0.0
    };
    let grain = if recipe.tile.level == 0 {
        0.4
    } else {
        step * 0.5
    };
    let mut mb = Mb::new();
    let mut ids = vec![vec![0u32; cells + 1]; cells + 1];
    let mut rim = Vec::new();
    let mut wet = false;
    let under = Under::of(land, &recipe, lo, step, wide);
    for (i, row) in ids.iter_mut().enumerate() {
        for (j, id) in row.iter_mut().enumerate() {
            let p = lo + Vec2::new(i as f32, j as f32) * step;
            let y = under.at(i, j, land.height_wide(p, wide));
            wet |= y < water + 0.15;
            let n = land.normal_wide(p, grain, wide);
            let colour = land.ground_colour(p, y, n);
            // Low ground stays where it is, or the water would
            // spread.
            let sunk = y - recipe.sink
                * smoothstep(water + 0.3, water + 1.5, y);
            let at = Vec3::new(p.x, sunk, p.y);
            *id = mb.v(at, n, colour);
            if i == 0 || j == 0 || i == cells || j == cells {
                rim.push(((i, j), at, n, colour));
            }
        }
    }
    for i in 0..cells {
        for j in 0..cells {
            mb.quad(
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
    let hem = |mb: &mut Mb, (i, j): (usize, usize)| {
        rim.iter()
            .find(|(at, ..)| *at == (i, j))
            .map(|&(_, at, n, colour)| {
                let low = recipe.hem(land, side, at.y);
                mb.v(Vec3::new(at.x, low, at.z), n, colour)
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
            let low = (hem(&mut mb, a), hem(&mut mb, b));
            mb.quad(top.0, top.1, low.1, low.0);
            mb.quad(top.1, top.0, low.0, low.1);
        }
    }
    Sod {
        mesh: mb.build(),
        wet,
    }
}
