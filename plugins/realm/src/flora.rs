//! Trees: a few shared meshes placed many times, and far off one
//! plain mesh for all the trees of a tile. Where they stand follows
//! from the seed and keeps clear of every town site.

use std::f32::consts::TAU;

use glam::{Vec2, Vec3};

use crate::land::Land;
use crate::mesh::{Mb, Rng, smoothstep};
use crate::noise::{key, unit};
use crate::palette::Palette;

/// Tree meshes there are: broadleaves first, then conifers.
pub(crate) const KINDS: usize = 8;
const BROADLEAVES: usize = 5;

pub struct Tree {
    pub at: Vec3,
    pub kind: usize,
    pub scale: f32,
    pub yaw: f32,
}

/// The mesh of one kind of tree, about three units tall, standing on
/// the origin.
pub fn mesh(pal: &Palette, kind: usize) -> Mb {
    let mut rng = Rng::seeded(&format!("tree kind {kind}"));
    let mut mb = Mb::new();
    let bark = pal.bark;
    if kind < BROADLEAVES {
        let height = rng.range(1.5, 2.0);
        let bend = Vec3::new(rng.sym(0.2), 0.0, rng.sym(0.2));
        let top = Vec3::Y * height + bend;
        mb.tube(-Vec3::Y * 0.4, top, 0.2, 0.11, 7, bark, false);
        let (light, shade) = if kind == BROADLEAVES - 1 {
            (pal.autumn, pal.autumn_shade)
        } else {
            (
                pal.foliage.lerp(pal.grass_tip, kind as f32 * 0.12),
                pal.foliage_shade,
            )
        };
        let lumps = 5;
        for i in 0..lumps {
            let a = i as f32 / (lumps - 1) as f32 * TAU + rng.f();
            let out = if i == 0 { 0.0 } else { 0.72 };
            let c = top
                + Vec3::new(
                    a.cos() * out,
                    if i == 0 { 0.75 } else { rng.range(0.0, 0.45) },
                    a.sin() * out,
                );
            let r = rng.range(0.8, 1.02);
            let tint = rng.value(0.06);
            mb.blob(
                c,
                Vec3::new(r, r * 0.86, r),
                6,
                0.2,
                rng.f() * 40.0,
                move |d| {
                    shade.lerp(light, smoothstep(-0.55, 0.7, d.y))
                        * tint
                },
            );
        }
    } else {
        let snowy = kind == KINDS - 1;
        let (dark, mid) = (pal.conifer, pal.foliage_shade);
        mb.tube(
            -Vec3::Y * 0.4,
            Vec3::Y * 1.0,
            0.14,
            0.1,
            6,
            bark,
            false,
        );
        let tiers = 3;
        for t in 0..tiers {
            let k = t as f32 / tiers as f32;
            let base = 0.55 + k * 2.1;
            let r = 1.05 * (1.0 - k * 0.62) * rng.range(0.9, 1.1);
            let h = 1.45 - k * 0.3;
            let tip = if snowy { pal.conifer_snow } else { mid };
            mb.lathe(
                Vec3::Y * base,
                &[(r, 0.0), (r * 0.55, h * 0.45), (0.0, h)],
                8,
                move |ring, seg| {
                    let c = if ring == 0 { dark } else { tip };
                    c * (0.94 + 0.1 * ((seg * 5 % 3) as f32 / 3.0))
                },
            );
        }
    }
    mb
}

/// The columns of the grid trees are scattered on that the trees
/// between `lo` and `hi` stand in.
pub fn columns(
    land: &Land,
    lo: Vec2,
    hi: Vec2,
) -> std::ops::RangeInclusive<i32> {
    let (cell, origin) =
        (land.set.land.tree_cell, land.set.land.tree_origin);
    let first = ((lo - origin) / cell).floor() - 1.0;
    let last = ((hi - origin) / cell).floor();
    first.x as i32..=last.x as i32
}

/// The trees that stand between `lo` and `hi` in the grid's column
/// `cx`, put in `out`: one in `every` of them, each grown to cover
/// the ground of those left out.
pub fn column_trees(
    land: &Land,
    lo: Vec2,
    hi: Vec2,
    every: u64,
    cx: i32,
    out: &mut Vec<Tree>,
) {
    let (cell, origin) =
        (land.set.land.tree_cell, land.set.land.tree_origin);
    let first = ((lo - origin) / cell).floor() - 1.0;
    let last = ((hi - origin) / cell).floor();
    let grown = (every as f32).sqrt();
    for cz in first.y as i32..=last.y as i32 {
        let h = key(&[land.seed, 71, cx as u64, cz as u64]);
        if every > 1 && !key(&[h, 8]).is_multiple_of(every) {
            continue;
        }
        let roll = |salt: u64| unit(key(&[h, salt]));
        let p = Vec2::new(
            (cx as f32 + roll(0)) * cell + origin,
            (cz as f32 + roll(1)) * cell + origin,
        );
        if p.cmplt(lo).any() || p.cmpge(hi).any() {
            continue;
        }
        if land.site_dist(p) < land.set.town.radius + 1.5
            || land.holds(p, 1.5)
        {
            continue;
        }
        let y = land.height(p);
        if y < land.set.land.water_y + 0.7 {
            continue;
        }
        let high = smoothstep(5.0, 11.0, y);
        let chance = if high > 0.5 {
            let steep = 1.0 - land.normal(p).y;
            0.75 * smoothstep(0.2, 0.08, steep)
                * smoothstep(36.0, 24.0, y)
        } else {
            0.07 + 0.88 * land.forest(p)
        };
        if roll(2) > chance {
            continue;
        }
        let conifer = roll(3) < high.max(0.12 * land.forest(p));
        let kind = if conifer {
            let snowy = y > 22.0 && roll(4) < 0.6;
            if snowy {
                KINDS - 1
            } else {
                BROADLEAVES + (roll(4) * 2.0) as usize
            }
        } else if roll(4) < 0.1 {
            BROADLEAVES - 1
        } else {
            (roll(5) * (BROADLEAVES - 1) as f32) as usize
        };
        out.push(Tree {
            at: Vec3::new(p.x, y, p.y),
            kind,
            scale: (0.85 + 0.5 * roll(6)) * grown,
            yaw: roll(7) * TAU,
        });
    }
}

/// The light and the shade of a kind of tree's leaves.
fn leaves(pal: &Palette, kind: usize) -> (Vec3, Vec3) {
    if kind == KINDS - 1 {
        (pal.conifer_snow, pal.conifer)
    } else if kind >= BROADLEAVES {
        (pal.foliage_shade, pal.conifer)
    } else if kind == BROADLEAVES - 1 {
        (pal.autumn, pal.autumn_shade)
    } else {
        (
            pal.foliage.lerp(pal.grass_tip, kind as f32 * 0.12),
            pal.foliage_shade,
        )
    }
}

/// Many trees as one mesh, for where they are too far to tell
/// apart: each a crown with no trunk, of a few faces, or with
/// `crowns` off a mere speck.
pub(crate) fn canopy(
    pal: &Palette,
    trees: &[Tree],
    crowns: bool,
) -> Mb {
    let mut mb = Mb::new();
    for tree in trees {
        let (light, shade) = leaves(pal, tree.kind);
        let s = tree.scale;
        let conifer = tree.kind >= BROADLEAVES;
        // The tip's height over the ground, then rings of the crown
        // from the top down, each a radius and a height.
        let (tip, rings, sides): (f32, &[(f32, f32)], usize) =
            match (conifer, crowns) {
                (true, true) => {
                    (3.75, &[(0.55, 2.0), (1.05, 0.6)], 5)
                }
                (true, false) => (3.6, &[(1.0, 0.6)], 4),
                (false, true) => {
                    (3.45, &[(1.5, 2.35), (0.95, 1.4)], 5)
                }
                (false, false) => (3.4, &[(1.5, 1.7)], 4),
            };
        let top = mb.v(tree.at + Vec3::Y * tip * s, Vec3::Y, light);
        let mut above = vec![top; sides];
        for (k, &(radius, height)) in rings.iter().enumerate() {
            let shown = (k + 1) as f32 / rings.len() as f32;
            let colour = light.lerp(shade, shown);
            let ring = (0..sides)
                .map(|i| {
                    let turn =
                        tree.yaw + i as f32 / sides as f32 * TAU;
                    let out = Vec3::new(turn.cos(), 0.0, turn.sin());
                    mb.v(
                        tree.at
                            + (out * radius + Vec3::Y * height) * s,
                        (out + Vec3::Y * 0.35).normalize(),
                        colour,
                    )
                })
                .collect::<Vec<_>>();
            for i in 0..sides {
                let j = (i + 1) % sides;
                if k == 0 {
                    mb.tri(top, ring[j], ring[i]);
                } else {
                    mb.quad(above[i], above[j], ring[j], ring[i]);
                }
            }
            above = ring;
        }
    }
    mb
}
