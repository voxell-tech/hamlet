//! Seeded gradient noise and the hashes everything else is placed
//! with.

use bevy::prelude::*;

/// A well mixed 32-bit hash of `x`: PCG's output step, a multiply and
/// add, a data-dependent shift and a multiply. It is 32-bit steps
/// alone, so a shader can run the same one.
pub fn mix(x: u32) -> u32 {
    let state =
        x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state)
        .wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// A hash of several keys in order.
pub fn key(parts: &[u32]) -> u32 {
    parts.iter().fold(0x51ed_270b, |h, &p| mix(h ^ p))
}

/// Uniform in 0..1 from a hash.
pub fn unit(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// Uniform in -1..1 from a hash.
pub fn signed(h: u32) -> f32 {
    unit(h) * 2.0 - 1.0
}

/// Sixteen unit directions, a sixteenth of a turn apart, that a
/// lattice point's hash picks a gradient from.
const DIRECTIONS: [Vec2; 16] = {
    const C: [f32; 4] =
        [1.0, 0.923_879_5, 0.707_106_77, 0.382_683_43];
    [
        Vec2::new(C[0], 0.0),
        Vec2::new(C[1], C[3]),
        Vec2::new(C[2], C[2]),
        Vec2::new(C[3], C[1]),
        Vec2::new(0.0, C[0]),
        Vec2::new(-C[3], C[1]),
        Vec2::new(-C[2], C[2]),
        Vec2::new(-C[1], C[3]),
        Vec2::new(-C[0], 0.0),
        Vec2::new(-C[1], -C[3]),
        Vec2::new(-C[2], -C[2]),
        Vec2::new(-C[3], -C[1]),
        Vec2::new(0.0, -C[0]),
        Vec2::new(C[3], -C[1]),
        Vec2::new(C[2], -C[2]),
        Vec2::new(C[1], -C[3]),
    ]
};

fn gradient(ix: i32, iz: i32, seed: u32) -> Vec2 {
    DIRECTIONS[(key(&[seed, ix as u32, iz as u32]) & 15) as usize]
}

/// Gradient noise in about -1..1, with a quintic fade so its slope is
/// continuous.
pub(crate) fn perlin(p: Vec2, seed: u32) -> f32 {
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
pub fn fbm(p: Vec2, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut total) =
        (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * perlin(p * freq, seed.wrapping_add(o));
        total += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / total
}
