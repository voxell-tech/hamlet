//! Patches: the one grid every surface laid over the world is drawn
//! in, the sea's and the ground's. The world is cut into a quadtree
//! of squares fixed on the map; a square near the eye is cut into
//! four, so every patch drawn, near or far, is the same grid, and
//! each is about as large to the eye. Where a patch meets a larger
//! one, its points slide onto the larger one's grid as they near it,
//! so the two meet with no seam and no patch pops as it splits.
//!
//! The patches are chosen here, on the CPU, each frame; a shader
//! moves each point and slides it, as `patch.wesl` has it.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// Quads along a patch's side. Even, so that every other point lies
/// on the grid of the patch one size up.
pub const QUADS: u32 = 32;

/// A square is cut into four while the eye is nearer to it than its
/// side times this. At 1.5 or more, a patch never meets one more than
/// twice its size; more keeps the ground finer further off, so a
/// river crossing the grid aslant does not break into steps.
/// `SPLIT` in `patch.wesl`.
pub const SPLIT: f32 = 3.0;

/// A patch: its corner of least x and z on the map, and its side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patch {
    pub corner: Vec2,
    pub side: f32,
}

/// The patches that cover the map within `reach` of `eye` on the
/// plane `y` high, the smallest `finest` across: each square of the
/// quadtree that the eye is far enough from, or the smallest ones.
pub fn choose(
    eye: Vec3,
    y: f32,
    finest: f32,
    reach: f32,
) -> Vec<Patch> {
    let levels = (2.0 * reach / finest).log2().ceil().max(0.0) as i32;
    let root = finest * 2f32.powi(levels);
    let lo = ((eye.xz() - reach) / root).floor();
    let hi = ((eye.xz() + reach) / root).ceil();
    let mut chosen = Vec::new();
    for x in lo.x as i32..hi.x as i32 {
        for z in lo.y as i32..hi.y as i32 {
            let corner = Vec2::new(x as f32, z as f32) * root;
            cut(eye, y, finest, corner, root, &mut chosen);
        }
    }
    chosen
}

/// The square at `corner`, `side` across, as patches, cut into four
/// while the eye is near it.
fn cut(
    eye: Vec3,
    y: f32,
    finest: f32,
    corner: Vec2,
    side: f32,
    chosen: &mut Vec<Patch>,
) {
    if side > finest * 1.5
        && away(eye, y, corner, side) < SPLIT * side
    {
        let half = side * 0.5;
        for (dx, dz) in
            [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
        {
            let at = corner + Vec2::new(dx, dz) * half;
            cut(eye, y, finest, at, half, chosen);
        }
    } else {
        chosen.push(Patch { corner, side });
    }
}

/// How far `eye` is from the nearest point of the square at `corner`,
/// `side` across, on the plane `y` high.
pub fn away(eye: Vec3, y: f32, corner: Vec2, side: f32) -> f32 {
    let near = eye.xz().clamp(corner, corner + side);
    eye.distance(Vec3::new(near.x, y, near.y))
}

/// The grid a patch is drawn in: `quads` by `quads` quads over a unit
/// square from its corner of least x and z, to be scaled by its side
/// and moved to its corner. [`QUADS`] of them, unless the patch is
/// flat all over, when one will do. Its vertex colours are what a
/// shader fills in; white here.
pub fn grid(quads: u32) -> Mesh {
    let side = quads + 1;
    let mut at = Vec::with_capacity((side * side) as usize);
    for j in 0..side {
        for i in 0..side {
            at.push([
                i as f32 / quads as f32,
                0.0,
                j as f32 / quads as f32,
            ]);
        }
    }
    // Each quad is cut from its first corner to the one across, as
    // the realm's `Land::surface` cuts it, so the ground drawn up
    // close is the ground it stands things on.
    let mut indices =
        Vec::with_capacity((quads * quads * 6) as usize);
    for j in 0..quads {
        for i in 0..quads {
            let a = j * side + i;
            indices.extend([
                a,
                a + side,
                a + side + 1,
                a,
                a + side + 1,
                a + 1,
            ]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; at.len()];
    let colours = vec![[1.0; 4]; at.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, at)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
    .with_inserted_indices(Indices::U32(indices))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every patch meets its neighbours only where they are at most
    /// twice or half its size, so their points can slide to meet.
    #[test]
    fn neighbours_differ_by_one_size_at_most() {
        let eye = Vec3::new(13.0, 40.0, -7.0);
        let patches = choose(eye, 0.0, 8.0, 2000.0);
        for a in &patches {
            for b in &patches {
                let touch = a.corner.x <= b.corner.x + b.side
                    && b.corner.x <= a.corner.x + a.side
                    && a.corner.y <= b.corner.y + b.side
                    && b.corner.y <= a.corner.y + a.side;
                if touch {
                    let ratio =
                        a.side.max(b.side) / a.side.min(b.side);
                    assert!(ratio <= 2.0, "{a:?} meets {b:?}");
                }
            }
        }
    }
}
