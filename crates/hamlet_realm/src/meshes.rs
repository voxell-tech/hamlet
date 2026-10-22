//! A small mesh builder: chamfered boxes, tubes, lathes and blobs
//! with vertex colours, plus the seeded randomness that gives each
//! thing its own face.

use std::f32::consts::{PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// A deterministic generator seeded from a thing's stable name.
pub struct Rng(u64);

impl Rng {
    pub fn seeded(name: &str) -> Self {
        let mut h = 0xcbf29ce484222325u64;
        for b in name.bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
        Self(h | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    /// Uniform in 0..1.
    pub fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }

    /// Uniform in -a..a.
    pub fn sym(&mut self, a: f32) -> f32 {
        self.range(-a, a)
    }

    /// A value multiplier of 1 plus or minus `a`.
    pub fn value(&mut self, a: f32) -> f32 {
        1.0 + self.sym(a)
    }

    /// A small random rotation, up to `deg` degrees about each axis.
    pub fn wobble(&mut self, deg: f32) -> Quat {
        let r = deg.to_radians();
        Quat::from_euler(
            EulerRot::XYZ,
            self.sym(r),
            self.sym(r),
            self.sym(r),
        )
    }
}

fn hash2(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x85ebca6b)
        ^ (y as u32).wrapping_mul(0xc2b2ae35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a2d39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Smooth value noise in 0..1.
pub fn noise2(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let (ux, uy) =
        (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (xi, yi) = (xi as i32, yi as i32);
    let a = hash2(xi, yi);
    let b = hash2(xi + 1, yi);
    let c = hash2(xi, yi + 1);
    let d = hash2(xi + 1, yi + 1);
    a + (b - a) * ux + (c - a) * uy + (a - b - c + d) * ux * uy
}

/// Three octaves of value noise, in 0..1.
pub fn fbm(x: f32, y: f32) -> f32 {
    (noise2(x, y)
        + 0.5 * noise2(x * 2.03 + 11.0, y * 2.03 - 7.0)
        + 0.25 * noise2(x * 4.1 - 3.0, y * 4.1 + 5.0))
        / 1.75
}

/// The value `t` of the way from `a` to `b`.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A smooth hump over `from..to`, 0 outside it.
pub fn hump(x: f32, from: f32, to: f32) -> f32 {
    if x <= from || x >= to {
        return 0.0;
    }
    0.5 - 0.5 * (TAU * (x - from) / (to - from)).cos()
}

pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Accumulates triangles with normals, vertex colours and UVs.
#[derive(Default)]
pub struct Mb {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    uv: Vec<[f32; 2]>,
    idx: Vec<u32>,
    /// Applied to everything pushed after it is set.
    pub xf: Affine3A,
}

use bevy::math::Affine3A;

impl Mb {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }

    pub fn tris(&self) -> usize {
        self.idx.len() / 3
    }

    /// True when any vertex was given a UV.
    fn has_uvs(&self) -> bool {
        self.uv.iter().any(|uv| *uv != [0.0; 2])
    }

    /// True when every index fits in 16 bits.
    fn is_small(&self) -> bool {
        self.pos.len() <= u16::MAX as usize + 1
    }

    /// Bytes of vertex and index data the built mesh holds.
    pub fn bytes(&self) -> usize {
        let vertex = if self.has_uvs() { 48 } else { 40 };
        let index = if self.is_small() { 2 } else { 4 };
        self.pos.len() * vertex + self.idx.len() * index
    }

    pub fn at(&mut self, xf: Affine3A) -> &mut Self {
        self.xf = xf;
        self
    }

    pub fn v(&mut self, p: Vec3, n: Vec3, c: Vec3) -> u32 {
        self.vuv(p, n, c, Vec2::ZERO)
    }

    pub fn vuv(
        &mut self,
        p: Vec3,
        n: Vec3,
        c: Vec3,
        uv: Vec2,
    ) -> u32 {
        let p = self.xf.transform_point3(p);
        let n = self.xf.transform_vector3(n).normalize_or_zero();
        self.pos.push(p.to_array());
        self.nor.push(n.to_array());
        self.col.push([c.x, c.y, c.z, 1.0]);
        self.uv.push(uv.to_array());
        self.pos.len() as u32 - 1
    }

    /// A triangle wound so that it faces the way its vertex normals
    /// point.
    pub fn tri(&mut self, a: u32, b: u32, c: u32) {
        let p = |i: u32| Vec3::from(self.pos[i as usize]);
        let n = |i: u32| Vec3::from(self.nor[i as usize]);
        let face = (p(b) - p(a)).cross(p(c) - p(a));
        if face.dot(n(a) + n(b) + n(c)) < 0.0 {
            self.idx.extend([a, c, b]);
        } else {
            self.idx.extend([a, b, c]);
        }
    }

    pub fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.tri(a, b, c);
        self.tri(a, c, d);
    }

    /// A flat quad whose normal faces `toward` (a rough outward
    /// direction).
    pub fn face(&mut self, p: [Vec3; 4], toward: Vec3, c: Vec3) {
        self.face_cols(p, toward, [c; 4]);
    }

    pub fn face_cols(
        &mut self,
        p: [Vec3; 4],
        toward: Vec3,
        c: [Vec3; 4],
    ) {
        let mut n =
            (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        if n.dot(toward) < 0.0 {
            n = -n;
        }
        let i: Vec<u32> =
            (0..4).map(|k| self.v(p[k], n, c[k])).collect();
        self.quad(i[0], i[1], i[2], i[3]);
    }

    /// A box with every edge chamfered. Bevel faces share the flat
    /// faces' normals, so the edge shades as if rounded. `ao`
    /// darkens the bottom.
    pub fn rbox(
        &mut self,
        c: Vec3,
        half: Vec3,
        bevel: f32,
        rot: Quat,
        col: Vec3,
        ao: f32,
    ) {
        let b = bevel.min(half.min_element() * 0.9);
        let mut ids = [[0u32; 3]; 8];
        for (corner, slot) in ids.iter_mut().enumerate() {
            let s = Vec3::new(
                if corner & 1 == 0 { -1.0 } else { 1.0 },
                if corner & 2 == 0 { -1.0 } else { 1.0 },
                if corner & 4 == 0 { -1.0 } else { 1.0 },
            );
            for (axis, id) in slot.iter_mut().enumerate() {
                let mut inset = half - Vec3::splat(b);
                inset[axis] = half[axis];
                let mut n = Vec3::ZERO;
                n[axis] = s[axis];
                let shade = if s.y < 0.0 { 1.0 - ao } else { 1.0 };
                *id = self.v(
                    c + rot * (s * inset),
                    rot * n,
                    col * shade,
                );
            }
        }
        for axis in 0..3 {
            let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
            for side in 0..2 {
                let base = side << axis;
                let q = [
                    base,
                    base | (1 << u),
                    base | (1 << u) | (1 << w),
                    base | (1 << w),
                ];
                self.quad(
                    ids[q[0]][axis],
                    ids[q[1]][axis],
                    ids[q[2]][axis],
                    ids[q[3]][axis],
                );
            }
            // The four edges that run along this axis.
            for k in 0..4 {
                let lo = ((k & 1) << u) | ((k >> 1) << w);
                let hi = lo | (1 << axis);
                self.quad(
                    ids[lo][u], ids[hi][u], ids[hi][w], ids[lo][w],
                );
            }
        }
        for corner in ids {
            self.tri(corner[0], corner[1], corner[2]);
        }
    }

    /// A round bar from `a` to `b`, optionally capped.
    pub fn tube(
        &mut self,
        a: Vec3,
        b: Vec3,
        ra: f32,
        rb: f32,
        segs: usize,
        col: Vec3,
        caps: bool,
    ) {
        let axis = (b - a).normalize_or_zero();
        let (u, w) = axis.any_orthonormal_pair();
        let slope = (ra - rb) / (b - a).length().max(1e-5);
        let mut ring = Vec::with_capacity(segs * 2);
        for i in 0..segs {
            let t = i as f32 / segs as f32 * TAU;
            let d = u * t.cos() + w * t.sin();
            let n = (d + axis * slope).normalize();
            ring.push((
                self.v(a + d * ra, n, col),
                self.v(b + d * rb, n, col),
            ));
        }
        for i in 0..segs {
            let (p, q) = (ring[i], ring[(i + 1) % segs]);
            self.quad(p.0, q.0, q.1, p.1);
        }
        if caps {
            for (centre, r, n) in [(a, ra, -axis), (b, rb, axis)] {
                let m = self.v(centre, n, col);
                let rim: Vec<u32> = (0..segs)
                    .map(|i| {
                        let t = i as f32 / segs as f32 * TAU;
                        self.v(
                            centre + (u * t.cos() + w * t.sin()) * r,
                            n,
                            col,
                        )
                    })
                    .collect();
                for i in 0..segs {
                    self.tri(m, rim[i], rim[(i + 1) % segs]);
                }
            }
        }
    }

    /// A surface of revolution about the Y axis through `base`. The
    /// profile is (radius, height) pairs from bottom to top.
    /// `col` gets (ring, segment).
    pub fn lathe(
        &mut self,
        base: Vec3,
        profile: &[(f32, f32)],
        segs: usize,
        col: impl Fn(usize, usize) -> Vec3,
    ) {
        let n2 = |i: usize| {
            let a = profile[i.saturating_sub(1)];
            let b = profile[(i + 1).min(profile.len() - 1)];
            Vec2::new(b.1 - a.1, a.0 - b.0).normalize_or_zero()
        };
        for r in 0..profile.len() - 1 {
            for s in 0..segs {
                let c = col(r, s);
                let mut ids = [0u32; 4];
                for (k, (ri, si)) in
                    [(r, s), (r, s + 1), (r + 1, s + 1), (r + 1, s)]
                        .into_iter()
                        .enumerate()
                {
                    let t = si as f32 / segs as f32 * TAU;
                    let (rad, y) = profile[ri];
                    let n = n2(ri);
                    let dir = Vec3::new(t.cos(), 0.0, t.sin());
                    let mut normal = dir * n.x + Vec3::Y * n.y;
                    if rad < 1e-4 {
                        normal = Vec3::Y * n.y.signum();
                    }
                    ids[k] = self.v(
                        base + dir * rad + Vec3::Y * y,
                        normal,
                        c,
                    );
                }
                if profile[r].0 < 1e-4 {
                    self.tri(ids[0], ids[2], ids[3]);
                } else if profile[r + 1].0 < 1e-4 {
                    self.tri(ids[0], ids[1], ids[2]);
                } else {
                    self.quad(ids[0], ids[1], ids[2], ids[3]);
                }
            }
        }
    }

    /// A lumpy ellipsoid. `col` gets the unit direction from the
    /// centre.
    pub fn blob(
        &mut self,
        c: Vec3,
        r: Vec3,
        rings: usize,
        wobble: f32,
        seed: f32,
        col: impl Fn(Vec3) -> Vec3,
    ) {
        let segs = rings * 2;
        let start = self.pos.len();
        let mut grid = vec![vec![0u32; segs + 1]; rings + 1];
        for (i, row) in grid.iter_mut().enumerate() {
            let phi = i as f32 / rings as f32 * PI;
            for (j, id) in row.iter_mut().enumerate() {
                let th = (j % segs) as f32 / segs as f32 * TAU;
                let d = Vec3::new(
                    phi.sin() * th.cos(),
                    phi.cos(),
                    phi.sin() * th.sin(),
                );
                let lump = if i == 0 || i == rings {
                    0.0
                } else {
                    fbm(
                        d.x * 2.3 + seed + d.y * 1.7,
                        d.z * 2.3 - seed + d.y * 1.3,
                    ) - 0.5
                };
                *id = self.v(
                    c + d * r * (1.0 + wobble * 2.0 * lump),
                    d,
                    col(d),
                );
            }
        }
        for i in 0..rings {
            for j in 0..segs {
                let (a, b, cc, d) = (
                    grid[i][j],
                    grid[i][j + 1],
                    grid[i + 1][j + 1],
                    grid[i + 1][j],
                );
                if i == 0 {
                    self.tri(a, cc, d);
                } else if i == rings - 1 {
                    self.tri(a, b, d);
                } else {
                    self.quad(a, b, cc, d);
                }
            }
        }
        // The squash and lumps bend the surface, so nudge normals to
        // match.
        for k in start..self.pos.len() {
            let n = Vec3::from(self.nor[k]);
            let local = self.xf.inverse().transform_vector3(n);
            let squashed = (local / r).normalize_or_zero();
            self.nor[k] = self
                .xf
                .transform_vector3(squashed)
                .normalize_or_zero()
                .to_array();
        }
    }

    pub fn torus(
        &mut self,
        c: Vec3,
        axis: Vec3,
        big: f32,
        small: f32,
        segs: usize,
        col: Vec3,
    ) {
        let axis = axis.normalize();
        let (u, w) = axis.any_orthonormal_pair();
        let sides = 8;
        let mut ids = vec![vec![0u32; sides]; segs];
        for (i, row) in ids.iter_mut().enumerate() {
            let t = i as f32 / segs as f32 * TAU;
            let out = u * t.cos() + w * t.sin();
            for (j, id) in row.iter_mut().enumerate() {
                let a = j as f32 / sides as f32 * TAU;
                let n = out * a.cos() + axis * a.sin();
                *id = self.v(c + out * big + n * small, n, col);
            }
        }
        for i in 0..segs {
            for j in 0..sides {
                let (i2, j2) = ((i + 1) % segs, (j + 1) % sides);
                self.quad(
                    ids[i][j],
                    ids[i2][j],
                    ids[i2][j2],
                    ids[i][j2],
                );
            }
        }
    }

    /// The mesh, without UVs if none were given and with short
    /// indices if they fit.
    pub fn build(mut self) -> Mesh {
        // A mesh of nothing is given one point-sized triangle, which
        // draws nothing. Bevy's mesh allocator gives a mesh without
        // vertices no room and then logs a use-after-free as it
        // copies it in.
        if self.pos.is_empty() {
            self.xf = Affine3A::IDENTITY;
            let at = self.v(Vec3::ZERO, Vec3::Y, Vec3::ZERO);
            self.pos.extend([[0.0; 3]; 2]);
            self.nor.extend([[0.0, 1.0, 0.0]; 2]);
            self.col.extend([self.col[0]; 2]);
            self.uv.extend([[0.0; 2]; 2]);
            self.idx = vec![at, at + 1, at + 2];
        }
        let (uvs, small) = (self.has_uvs(), self.is_small());
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nor)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col);
        if uvs {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv);
        }
        mesh.with_inserted_indices(if small {
            Indices::U16(
                self.idx.into_iter().map(|i| i as u16).collect(),
            )
        } else {
            Indices::U32(self.idx)
        })
    }
}

/// A polyline sampled from a Catmull-Rom spline, with arc lengths.
pub struct Line {
    pub pts: Vec<Vec3>,
    pub len: Vec<f32>,
}

impl Line {
    pub fn spline(way: &[Vec2], step: f32) -> Self {
        let get = |i: isize| {
            way[i.clamp(0, way.len() as isize - 1) as usize]
        };
        let mut pts = Vec::new();
        for i in 0..way.len() as isize - 1 {
            let (p0, p1, p2, p3) =
                (get(i - 1), get(i), get(i + 1), get(i + 2));
            let n =
                ((p2 - p1).length() / step).ceil().max(1.0) as usize;
            for k in 0..n {
                let t = k as f32 / n as f32;
                let (t2, t3) = (t * t, t * t * t);
                let p = 0.5
                    * (2.0 * p1
                        + (p2 - p0) * t
                        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3);
                pts.push(Vec3::new(p.x, 0.0, p.y));
            }
        }
        let last = way[way.len() - 1];
        pts.push(Vec3::new(last.x, 0.0, last.y));
        let mut len = vec![0.0; pts.len()];
        for i in 1..pts.len() {
            len[i] = len[i - 1] + (pts[i] - pts[i - 1]).length();
        }
        Self { pts, len }
    }

    pub fn total(&self) -> f32 {
        *self.len.last().unwrap()
    }

    /// Position (y is 0) and unit tangent at arc length `s`.
    pub fn at(&self, s: f32) -> (Vec3, Vec3) {
        let s = s.clamp(0.0, self.total());
        let i = self
            .len
            .partition_point(|&l| l < s)
            .clamp(1, self.pts.len() - 1);
        let (a, b) = (self.pts[i - 1], self.pts[i]);
        let span = (self.len[i] - self.len[i - 1]).max(1e-6);
        (
            a.lerp(b, (s - self.len[i - 1]) / span),
            (b - a).normalize_or_zero(),
        )
    }

    /// Distance from a ground point to the line, and the arc length
    /// there.
    pub fn nearest(&self, x: f32, z: f32) -> (f32, f32) {
        let p = Vec3::new(x, 0.0, z);
        let mut best = (f32::MAX, 0.0);
        for i in 1..self.pts.len() {
            let (a, b) = (self.pts[i - 1], self.pts[i]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9))
                .clamp(0.0, 1.0);
            let d = (a + ab * t - p).length_squared();
            if d < best.0 {
                best = (d, self.len[i - 1] + t * ab.length());
            }
        }
        (best.0.sqrt(), best.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mesh_of_nothing_still_has_vertices() {
        let mesh = Mb::new().build();
        assert_eq!(mesh.count_vertices(), 3);
        assert!(mesh.get_vertex_buffer_size() > 0);
    }
}
