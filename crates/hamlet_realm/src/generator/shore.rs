//! The field of the distance to the shore, which the water's foam
//! follows: baked from the land in the background whenever the island
//! grows or shrinks, and handed to the water's material.

#[cfg(test)]
mod tests;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use hamlet_vfx::water::{SHORE_REACH, WaterMaterial, shore_field};

use super::kit::Kit;
use super::land::Land;

/// Texels to a metre, at most.
const DENSITY: f32 = 2.0;
/// Texels along a side, at most.
const MOST: usize = 2048;
/// Metres the field reaches beyond the island on every side.
const MARGIN: f32 = 40.0;
/// Metres from land the open sea's water lies at least, and metres
/// from the open sea the sea runs on to its shores.
const WIDE: f32 = 5.0;
const COAST: f32 = 7.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Shore>()
        .add_systems(Update, (ask, take).chain());
}

/// The field being baked, and the points the island reached about
/// when it was asked for.
#[derive(Resource, Default)]
struct Shore {
    shores: Vec<Vec2>,
    baking: Option<Task<Baked>>,
}

/// A field baked in the background.
struct Baked {
    field: Vec4,
    image: Image,
}

/// Asks for the field anew when the island's shores change.
fn ask(land: Res<Land>, mut shore: ResMut<Shore>) {
    if !land.is_changed()
        || land.shores().eq(shore.shores.iter().copied())
    {
        return;
    }
    shore.shores = land.shores().collect();
    let land = land.clone();
    shore.baking = Some(
        AsyncComputeTaskPool::get().spawn(async move { bake(&land) }),
    );
}

/// Gives the water a field that is baked.
fn take(
    mut shore: ResMut<Shore>,
    kit: Option<Res<Kit>>,
    mut images: ResMut<Assets<Image>>,
    mut waters: ResMut<Assets<WaterMaterial>>,
) {
    let Some(task) = shore.baking.as_mut() else {
        return;
    };
    let Some(baked) = block_on(poll_once(task)) else {
        return;
    };
    shore.baking = None;
    let Some(kit) = kit else {
        return;
    };
    if let Some(mut water) = waters.get_mut(&kit.water) {
        water.extension.shore = images.add(baked.image);
        water.extension.water.field = baked.field;
    }
}

/// The field over the island as `land` has it.
fn bake(land: &Land) -> Baked {
    let set = &land.set.land;
    let reach = set.island + set.coast.1 + set.rim.1 + MARGIN;
    let (lo, hi) = land.shores().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(lo, hi), at| (lo.min(at - reach), hi.max(at + reach)),
    );
    let size = hi - lo;
    let density = DENSITY.min(MOST as f32 / size.max_element());
    let (w, h) = (
        (size.x * density).ceil() as usize,
        (size.y * density).ceil() as usize,
    );
    let at = |i: usize| {
        lo + (Vec2::new((i % w) as f32, (i / w) as f32) + 0.5)
            / density
    };
    let water = set.water_y;
    let dry = (0..w * h)
        .map(|i| land.height(at(i)) > water)
        .collect::<Vec<_>>();
    // Each texel's distance to the nearest of the other kind, in
    // texels, then as metres: above zero over water.
    let to_land = squared_distance(&dry, w, h);
    let to_water = squared_distance(
        &dry.iter().map(|d| !d).collect::<Vec<_>>(),
        w,
        h,
    );
    let mut metres = (0..w * h)
        .map(|i| {
            let d = if dry[i] {
                -(to_water[i].sqrt() - 0.5)
            } else {
                to_land[i].sqrt() - 0.5
            };
            (d / density).clamp(-SHORE_REACH, SHORE_REACH)
        })
        .collect::<Vec<_>>();
    // Smoothed once, so that the coast does not show its texels.
    metres = blur(&metres, w, h);
    let sea = blur(
        &open_sea(&to_land, &dry, w, h, density)
            .into_iter()
            .map(|sea| if sea { 1.0 } else { 0.0 })
            .collect::<Vec<_>>(),
        w,
        h,
    );
    let value =
        |x: usize, y: usize| metres[y.min(h - 1) * w + x.min(w - 1)];
    let points = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let slope = Vec2::new(
                value(x + 1, y) - value(x.saturating_sub(1), y),
                value(x, y + 1) - value(x, y.saturating_sub(1)),
            );
            let to_shore = -slope.normalize_or_zero();
            [metres[i], to_shore.x, to_shore.y, sea[i]]
        })
        .collect::<Vec<_>>();
    Baked {
        field: Vec4::new(
            lo.x,
            lo.y,
            w as f32 / density,
            h as f32 / density,
        ),
        image: shore_field(w as u32, h as u32, &points),
    }
}

/// Which texels of a `w` by `h` grid, `density` to a metre, are the
/// sea's, and not a river's or a pool's: the water open to the grid's
/// edges through water at least [`WIDE`] metres from land, which no
/// river is so wide as to hold, and all within [`COAST`] metres of
/// it, the land too, so that the sea runs right up its shores.
/// `to_land` is each texel's squared distance to land in texels, and
/// `dry` whether it is land.
fn open_sea(
    to_land: &[f32],
    dry: &[bool],
    w: usize,
    h: usize,
    density: f32,
) -> Vec<bool> {
    let wide = (WIDE * density).powi(2);
    let open = |i: usize| !dry[i] && to_land[i] >= wide;
    let neighbours = |i: usize| {
        let (x, y) = (i % w, i / w);
        [
            (x > 0).then(|| i - 1),
            (x + 1 < w).then(|| i + 1),
            (y > 0).then(|| i - w),
            (y + 1 < h).then(|| i + w),
        ]
        .into_iter()
        .flatten()
    };
    // Steps from the open sea, by flooding from its edges.
    let edge = |i: usize| {
        let (x, y) = (i % w, i / w);
        x == 0 || y == 0 || x + 1 == w || y + 1 == h
    };
    let mut queue = (0..w * h)
        .filter(|&i| edge(i) && open(i))
        .collect::<std::collections::VecDeque<_>>();
    let mut steps = vec![usize::MAX; w * h];
    for &i in &queue {
        steps[i] = 0;
    }
    while let Some(i) = queue.pop_front() {
        for n in neighbours(i) {
            if steps[n] == usize::MAX && open(n) {
                steps[n] = 0;
                queue.push_back(n);
            }
        }
    }
    // Then out from it to every texel near enough, land or water.
    let reach = (COAST * density).ceil() as usize;
    queue.extend((0..w * h).filter(|&i| steps[i] == 0));
    while let Some(i) = queue.pop_front() {
        let next = steps[i] + 1;
        if next > reach {
            continue;
        }
        for n in neighbours(i) {
            if steps[n] == usize::MAX {
                steps[n] = next;
                queue.push_back(n);
            }
        }
    }
    steps.into_iter().map(|s| s != usize::MAX).collect()
}

/// The squared distance in texels from each texel of a `w` by `h`
/// grid to the nearest that is `target`: the exact Euclidean distance
/// transform, by rows and then by columns.
fn squared_distance(target: &[bool], w: usize, h: usize) -> Vec<f32> {
    const FAR: f32 = 1e20;
    let mut grid = target
        .iter()
        .map(|&t| if t { 0.0 } else { FAR })
        .collect::<Vec<_>>();
    let mut line = Vec::new();
    for y in 0..h {
        line.clear();
        line.extend_from_slice(&grid[y * w..(y + 1) * w]);
        let done = transform(&line);
        grid[y * w..(y + 1) * w].copy_from_slice(&done);
    }
    for x in 0..w {
        line.clear();
        line.extend((0..h).map(|y| grid[y * w + x]));
        for (y, d) in transform(&line).into_iter().enumerate() {
            grid[y * w + x] = d;
        }
    }
    grid
}

/// The one-dimensional squared distance transform of `f`: for each
/// place, the least of `f[q] + (place - q)²` over every `q`, by the
/// lower envelope of the parabolas the places stand for.
fn transform(f: &[f32]) -> Vec<f32> {
    let n = f.len();
    let mut out = vec![0.0; n];
    let mut sites = vec![0usize; n];
    let mut bounds = vec![0f32; n + 1];
    let mut k = 0;
    bounds[0] = f32::MIN;
    bounds[1] = f32::MAX;
    let cross = |q: usize, p: usize| {
        let (q2, p2) = ((q * q) as f32, (p * p) as f32);
        ((f[q] + q2) - (f[p] + p2)) / (2.0 * (q as f32 - p as f32))
    };
    for q in 1..n {
        let mut s = cross(q, sites[k]);
        while s <= bounds[k] {
            k -= 1;
            s = cross(q, sites[k]);
        }
        k += 1;
        sites[k] = q;
        bounds[k] = s;
        bounds[k + 1] = f32::MAX;
    }
    k = 0;
    for (place, slot) in out.iter_mut().enumerate() {
        while bounds[k + 1] < place as f32 {
            k += 1;
        }
        let d = place as f32 - sites[k] as f32;
        *slot = d * d + f[sites[k]];
    }
    out
}

/// `values` on a `w` by `h` grid, each the mean of the nine about it.
fn blur(values: &[f32], w: usize, h: usize) -> Vec<f32> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let mut sum = 0.0;
            for dy in [-1i32, 0, 1] {
                for dx in [-1i32, 0, 1] {
                    let (nx, ny) = (
                        (x as i32 + dx).clamp(0, w as i32 - 1)
                            as usize,
                        (y as i32 + dy).clamp(0, h as i32 - 1)
                            as usize,
                    );
                    sum += values[ny * w + nx];
                }
            }
            sum / 9.0
        })
        .collect()
}
