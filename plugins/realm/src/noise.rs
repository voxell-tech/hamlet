//! Seeded gradient noise and the hashes everything else is placed
//! with.

use std::f32::consts::TAU;

use glam::Vec2;

/// A well mixed 64-bit hash of `x`.
pub fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

/// A hash of several keys in order.
pub fn key(parts: &[u64]) -> u64 {
    parts.iter().fold(0x51ed270b0f1a2d3c, |h, &p| mix(h ^ p))
}

/// Uniform in 0..1 from a hash.
pub fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Uniform in -1..1 from a hash.
pub fn signed(h: u64) -> f32 {
    unit(h) * 2.0 - 1.0
}

fn gradient(ix: i32, iz: i32, seed: u64) -> Vec2 {
    let a =
        unit(key(&[seed, ix as u32 as u64, iz as u32 as u64])) * TAU;
    Vec2::new(a.cos(), a.sin())
}

/// Gradient noise in about -1..1, with a quintic fade so its slope is
/// continuous.
pub(crate) fn perlin(p: Vec2, seed: u64) -> f32 {
    let cell = p.floor();
    let f = p - cell;
    let (ix, iz) = (cell.x as i32, cell.y as i32);
    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (u, v) = (fade(f.x), fade(f.y));
    let dot = |dx: i32, dz: i32| {
        gradient(ix + dx, iz + dz, seed)
            .dot(f - Vec2::new(dx as f32, dz as f32))
    };
    let (a, b) = (dot(0, 0), dot(1, 0));
    let (c, d) = (dot(0, 1), dot(1, 1));
    let (top, bottom) = (a + (b - a) * u, c + (d - c) * u);
    (top + (bottom - top) * v) * 1.41
}

/// Fractal sum of `octaves` layers of `perlin`, in about -1..1.
pub fn fbm(p: Vec2, octaves: u32, seed: u64) -> f32 {
    let (mut sum, mut amp, mut freq, mut total) =
        (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * perlin(p * freq, seed.wrapping_add(o as u64));
        total += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / total
}

/// Ridged fractal noise in 0..1: sharp crests, each octave weighted
/// by the one before so detail gathers on the ridges.
pub(crate) fn ridged(p: Vec2, octaves: u32, seed: u64) -> f32 {
    let (mut sum, mut amp, mut freq, mut total) =
        (0.0, 1.0, 1.0, 0.0);
    let mut weight = 1.0;
    for o in 0..octaves {
        let n = perlin(p * freq, seed.wrapping_add(100 + o as u64));
        let ridge = (1.0 - n.abs()).powi(2) * weight;
        weight = (ridge * 2.0).clamp(0.0, 1.0);
        sum += amp * ridge;
        total += amp;
        amp *= 0.5;
        freq *= 2.1;
    }
    sum / total
}
