//! Trees: a few shared meshes placed many times, the leaves fallen
//! about them, and far off one plain mesh for all the trees of a
//! tile. Where they stand follows from the seed and keeps clear of
//! every town site.

pub(crate) mod plants;

use std::f32::consts::TAU;

use bevy::mesh::MeshTag;
use bevy::prelude::*;
use hamlet_vfx::leaves::{Stroke, tint};

use super::land::Land;
use super::noise::{key, unit};
use crate::meshes::{Meshing, Rng, smoothstep};
use crate::palette::Palette;

/// Kinds of tree there are: broadleaves first, the green ones, then
/// one in autumn and one in blossom, then conifers, the snowy one
/// last.
pub(crate) const KINDS: usize = 9;
const BROADLEAVES: usize = 6;
const AUTUMN: usize = 4;
pub(crate) const BLOSSOM: usize = 5;
/// Meshes of each kind, alike in kind and each its own shape.
pub(crate) const VARIANTS: usize = 4;
/// Tree meshes there are, each kind's variants in a row.
pub(crate) const FORMS: usize = KINDS * VARIANTS;

/// The tree mesh of `variant` of `kind`, among [`FORMS`].
pub(crate) fn form(kind: usize, variant: usize) -> usize {
    kind * VARIANTS + variant % VARIANTS
}

#[derive(Clone, Copy)]
pub struct Tree {
    pub at: Vec3,
    pub kind: usize,
    pub variant: usize,
    pub scale: f32,
    pub yaw: f32,
}

impl Tree {
    /// Its mesh, among [`FORMS`].
    pub(crate) fn form(&self) -> usize {
        form(self.kind, self.variant)
    }

    /// Its tag, which tints it in the leaves' shader.
    pub(crate) fn tag(&self) -> MeshTag {
        MeshTag::new(tag(self.at))
    }
}

/// The tag of a tree standing at `at`: never 0, which is no tint.
pub(crate) fn tag(at: Vec3) -> u32 {
    key(&[at.x.to_bits(), at.z.to_bits(), 5]) | 1 << 31
}

/// The mesh of `form` of [`FORMS`], about three and a half units
/// tall, standing on the origin. Its leaves are cards painted with
/// strokes over a darker core, so that no sky shows through the
/// crown; what is not a card lies at UV zero, on solid paint.
pub fn mesh(pal: &Palette, form: usize) -> Mesh {
    let (kind, variant) = (form / VARIANTS, form % VARIANTS);
    let mut rng = Rng::seeded(&format!("tree {kind} {variant}"));
    let mut meshing = Meshing::new();
    if kind < BROADLEAVES {
        broadleaf(&mut meshing, &mut rng, pal, kind);
    } else {
        conifer(&mut meshing, &mut rng, pal, kind);
    }
    meshing.build()
}

/// A card painted with `stroke`: its top edge's middle at `top`, the
/// stroke running down along `down` and as wide as `across`. It faces
/// `across` crossed with `down`, and is lit from that side.
fn card(
    meshing: &mut Meshing,
    top: Vec3,
    down: Vec3,
    across: Vec3,
    stroke: Stroke,
    colour: Vec3,
) {
    shaded_card(meshing, top, down, across, stroke, [colour; 2]);
}

/// A [`card`] coloured `colours[0]` along its top edge and
/// `colours[1]` along its bottom one.
fn shaded_card(
    meshing: &mut Meshing,
    top: Vec3,
    down: Vec3,
    across: Vec3,
    stroke: Stroke,
    colours: [Vec3; 2],
) {
    let (lo, hi) = stroke.cell();
    let n = across.cross(down).normalize_or_zero();
    let half = across * 0.5;
    let [high, low] = colours;
    let a = meshing.vuv(top - half, n, high, lo);
    let b = meshing.vuv(top + half, n, high, Vec2::new(hi.x, lo.y));
    let c = meshing.vuv(top + half + down, n, low, hi);
    let d =
        meshing.vuv(top - half + down, n, low, Vec2::new(lo.x, hi.y));
    meshing.quad(a, b, c, d);
}

/// Points spread evenly over a sphere, `n` of them, turned by `turn`.
fn spread(n: usize, turn: f32) -> impl Iterator<Item = Vec3> {
    const GOLDEN: f32 = 2.399_963;
    (0..n).map(move |i| {
        let y = 1.0 - 2.0 * (i as f32 + 0.5) / n as f32;
        let r = (1.0 - y * y).max(0.0).sqrt();
        let a = i as f32 * GOLDEN + turn;
        Vec3::new(a.cos() * r, y, a.sin() * r)
    })
}

/// A broadleaf: a trunk that forks into the crown, and the crown as
/// clumps of dabs over darker cores, lighter and warmer to the top.
fn broadleaf(
    meshing: &mut Meshing,
    rng: &mut Rng,
    pal: &Palette,
    kind: usize,
) {
    let (light, shade) = leaves(pal, kind);
    let sunny = light * Vec3::new(1.12, 1.1, 0.82);
    // The crown's half width and half height: round, egg or tall.
    let wide = rng.range(1.0, 1.3);
    let tall = wide * rng.range(0.85, 1.3);
    let stem = rng.range(1.15, 1.6);
    let lean = Vec3::new(rng.sym(0.25), 0.0, rng.sym(0.25));
    let fork = Vec3::Y * stem + lean;
    let middle = fork + Vec3::Y * tall * 0.7;
    meshing.tube(
        -Vec3::Y * 0.4,
        fork,
        0.18,
        0.11,
        7,
        pal.bark,
        false,
    );

    let lumps = 4 + (rng.f() * 4.0) as usize;
    let clumps: Vec<(Vec3, f32)> = (0..lumps)
        .map(|i| {
            if i == 0 {
                let up = Vec3::Y * tall * 0.35;
                return (middle + up, wide * rng.range(0.62, 0.72));
            }
            let a = rng.f() * TAU;
            let y = rng.range(-0.55, 0.6);
            let r = (1.0 - y * y).sqrt();
            let d = Vec3::new(a.cos() * r, y, a.sin() * r);
            let at = middle
                + d * Vec3::new(wide, tall, wide)
                    * rng.range(0.42, 0.58);
            (at, wide * rng.range(0.5, 0.66))
        })
        .collect();
    // Branches up into a few clumps, to show between the leaves.
    for &(at, _) in clumps.iter().skip(1).take(3) {
        let from = fork - Vec3::Y * rng.range(0.0, 0.3);
        let to = from.lerp(at, 0.8);
        meshing.tube(from, to, 0.07, 0.035, 5, pal.bark, false);
    }

    let crown = meshing.vertices();
    let (low, high) = (middle.y - tall, middle.y + tall);
    for (k, &(at, r)) in clumps.iter().enumerate() {
        let core = shade.lerp(light, 0.15) * 0.8;
        meshing.blob(
            at,
            Vec3::new(r, r * 0.9, r) * 0.82,
            5,
            0.15,
            rng.f() * 40.0,
            move |_| core,
        );
        let n = ((40.0 * r * r) as usize).max(10);
        for d in spread(n, rng.f() * TAU) {
            let p = at + d * r * rng.range(0.92, 1.08);
            // Hidden inside another clump.
            let buried =
                clumps.iter().enumerate().any(|(j, &(c, rc))| {
                    j != k && p.distance(c) < rc * 0.86
                });
            if buried || (d.y < -0.55 && rng.f() < 0.5) {
                continue;
            }
            let face = (d + Vec3::Y * 0.25).normalize();
            let (u, w) = face.any_orthonormal_pair();
            let turn = rng.f() * TAU;
            let along = u * turn.cos() + w * turn.sin();
            // So that the card faces out, as `card` takes its face.
            let side = along.cross(face);
            let size = rng.range(0.5, 0.68) * wide;
            let stroke = if rng.f() < 0.6 {
                Stroke::Dab
            } else {
                Stroke::Blot
            };
            let up = smoothstep(low + 0.1, high - 0.2, p.y);
            let mut colour = shade.lerp(light, up);
            if d.y > 0.35 && rng.f() < 0.35 {
                colour = sunny;
            }
            card(
                meshing,
                p - along * size * 0.5,
                along * size,
                side * size * 1.1,
                stroke,
                colour * rng.value(0.07),
            );
        }
    }
    meshing.round(crown, middle, 0.75);
}

/// A conifer: a thin trunk, skirts of dark needles in tiers, and
/// long strokes hanging from each skirt past its edge, olive at the
/// top and dark below.
fn conifer(
    meshing: &mut Meshing,
    rng: &mut Rng,
    pal: &Palette,
    kind: usize,
) {
    let snowy = kind == KINDS - 1;
    let (light, dark) = leaves(pal, kind);
    let top_colour = light.lerp(pal.grass_tip, 0.35);
    let wide = rng.range(0.9, 1.15);
    let height = rng.range(3.4, 3.9);
    let skirt = rng.range(0.5, 0.75);
    meshing.tube(
        -Vec3::Y * 0.4,
        Vec3::Y * height * 0.8,
        0.14,
        0.05,
        6,
        pal.bark,
        false,
    );

    let crown = meshing.vertices();
    let tiers = 4 + (rng.f() * 2.0) as usize;
    let span = height - skirt;
    for t in 0..tiers {
        let k = t as f32 / tiers as f32;
        let base = skirt + k * span * 0.8;
        let r =
            1.15 * wide * (1.0 - k * 0.78) * rng.range(0.92, 1.08);
        let h = span / tiers as f32 * 1.8;
        let colour = dark.lerp(top_colour, k);
        let core = dark * 0.75;
        meshing.lathe(
            Vec3::Y * base,
            &[(r * 0.85, 0.0), (r * 0.45, h * 0.55), (0.0, h)],
            8,
            move |_, _| core,
        );
        // Two rows of strokes: the long ones from halfway up out past
        // the edge, and shorter ones above them.
        for (row, (from, to, length)) in
            [(0.55, 1.15, 1.0), (0.2, 0.7, 0.75)]
                .into_iter()
                .enumerate()
        {
            let count = ((TAU * r * to / 0.3) as usize).max(6);
            let turn = rng.f() * TAU;
            for i in 0..count {
                let a = turn
                    + i as f32 / count as f32 * TAU
                    + rng.sym(0.15);
                let out = Vec3::new(a.cos(), 0.0, a.sin());
                let rise = if row == 0 { 0.45 } else { 0.85 };
                let top =
                    Vec3::Y * (base + h * rise) + out * r * from;
                let end = Vec3::Y * (base - h * 0.12 * length)
                    + out * r * to * rng.range(0.95, 1.1);
                let down = (end - top) * rng.range(0.9, 1.1);
                let across = Vec3::new(-a.sin(), 0.0, a.cos())
                    * rng.range(0.28, 0.4)
                    * wide;
                let mut c =
                    colour.lerp(top_colour, 0.25 * row as f32);
                if snowy && row == 1 {
                    c = c.lerp(pal.conifer_snow, 0.7);
                }
                card(
                    meshing,
                    top,
                    down,
                    across,
                    Stroke::Hang,
                    c * rng.value(0.08),
                );
            }
        }
    }
    // The tip: a few fans hung from its point.
    for i in 0..3 {
        let a = i as f32 / 3.0 * TAU + rng.f();
        let out = Vec3::new(a.cos(), 0.0, a.sin());
        let top = Vec3::Y * (height + 0.1);
        let down = -Vec3::Y * 0.7 + out * 0.2;
        let across = Vec3::new(-a.sin(), 0.0, a.cos()) * 0.45;
        let c = if snowy { pal.conifer_snow } else { top_colour };
        card(meshing, top, down, across, Stroke::Fan, c);
    }
    meshing.round(crown, Vec3::Y * height * 0.45, 0.5);
}

/// The trees that stand between `lo` and `hi`, one in `every` of
/// them, each grown to cover the ground of those left out.
pub fn trees(
    land: &Land,
    lo: Vec2,
    hi: Vec2,
    every: u32,
) -> Vec<Tree> {
    let (cell, origin) =
        (land.set.land.tree_cell, land.set.land.tree_origin);
    let first = ((lo - origin) / cell).floor() - 1.0;
    let last = ((hi - origin) / cell).floor();
    let grown = (every as f32).sqrt();
    let mut out = Vec::new();
    for cx in first.x as i32..=last.x as i32 {
        for cz in first.y as i32..=last.y as i32 {
            let h = key(&[land.seed, 71, cx as u32, cz as u32]);
            if every > 1 && !key(&[h, 8]).is_multiple_of(every) {
                continue;
            }
            let roll = |salt: u32| unit(key(&[h, salt]));
            let p = Vec2::new(
                (cx as f32 + roll(0)) * cell + origin,
                (cz as f32 + roll(1)) * cell + origin,
            );
            if p.cmplt(lo).any() || p.cmpge(hi).any() {
                continue;
            }
            if land.site_dist(p) < land.set.town.radius + 1.5
                || land.is_held(p, 1.5)
            {
                continue;
            }
            let y = land.height(p);
            if y < land.set.land.water_y + 0.7 {
                continue;
            }
            let high = smoothstep(5.0, 11.0, y);
            // None grow on a steep slope, low or high: from about 20
            // degrees they thin, and past about 33 there are none.
            let steep = 1.0 - land.normal(p).y;
            let footing = smoothstep(0.16, 0.06, steep);
            let chance = footing
                * if high > 0.5 {
                    0.75 * smoothstep(36.0, 24.0, y)
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
                AUTUMN
            } else if roll(8) < blossoms(land, p) {
                BLOSSOM
            } else {
                (roll(5) * AUTUMN as f32) as usize
            };
            out.push(Tree {
                at: Vec3::new(p.x, y, p.y),
                kind,
                variant: (roll(9) * VARIANTS as f32) as usize,
                scale: (0.85 + 0.5 * roll(6)) * grown,
                yaw: roll(7) * TAU,
            });
        }
    }
    out
}

/// The share of broadleaves in blossom at `p`: a few about the
/// hamlets, where people would have planted them, and fewer wild.
fn blossoms(land: &Land, p: Vec2) -> f32 {
    let out = land.site_dist(p) - land.set.town.radius;
    0.02 + 0.3 * smoothstep(40.0, 8.0, out)
}

/// The light and the shade of a kind of tree's leaves.
fn leaves(pal: &Palette, kind: usize) -> (Vec3, Vec3) {
    if kind == KINDS - 1 {
        (pal.conifer_snow, pal.conifer)
    } else if kind >= BROADLEAVES {
        (pal.foliage_shade, pal.conifer)
    } else if kind == BLOSSOM {
        (pal.blossom, pal.blossom_shade)
    } else if kind == AUTUMN {
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
) -> Mesh {
    let mut meshing = Meshing::new();
    for tree in trees {
        let (light, shade) = leaves(pal, tree.kind);
        let tinted = tint(tag(tree.at));
        let (light, shade) = (light * tinted, shade * tinted);
        let s = tree.scale;
        let conifer = tree.kind >= BROADLEAVES;
        // The tip's height over the ground, then rings of the crown
        // from the top down, each a radius and a height.
        let (tip, rings, sides): (f32, &[(f32, f32)], usize) =
            match (conifer, crowns) {
                (true, true) => {
                    (3.75, &[(0.4, 2.9), (0.8, 1.9), (1.05, 0.7)], 6)
                }
                (true, false) => (3.6, &[(1.0, 0.6)], 4),
                (false, true) => (
                    3.55,
                    &[
                        (0.95, 3.2),
                        (1.45, 2.55),
                        (1.4, 1.85),
                        (0.8, 1.35),
                    ],
                    7,
                ),
                (false, false) => (3.4, &[(1.5, 1.7)], 4),
            };
        // Faced out from the crown's middle, so it is lit round.
        let middle = tree.at + Vec3::Y * tip * 0.62 * s;
        let top =
            meshing.v(tree.at + Vec3::Y * tip * s, Vec3::Y, light);
        let mut above = vec![top; sides];
        for (k, &(radius, height)) in rings.iter().enumerate() {
            let shown = (k + 1) as f32 / rings.len() as f32;
            let colour = light.lerp(shade, shown);
            let ring = (0..sides)
                .map(|i| {
                    let turn =
                        tree.yaw + i as f32 / sides as f32 * TAU;
                    let out = Vec3::new(turn.cos(), 0.0, turn.sin());
                    let at = tree.at
                        + (out * radius + Vec3::Y * height) * s;
                    let face = (at - middle).normalize_or(Vec3::Y);
                    meshing.v(at, (face + out).normalize(), colour)
                })
                .collect::<Vec<_>>();
            for i in 0..sides {
                let j = (i + 1) % sides;
                if k == 0 {
                    meshing.tri(top, ring[j], ring[i]);
                } else {
                    meshing
                        .quad(above[i], above[j], ring[j], ring[i]);
                }
            }
            above = ring;
        }
    }
    meshing.build()
}

/// The leaves fallen about `trees`, lying on the land: a ring of
/// dabs about each tree a little past its crown, some kept and some
/// not, in its leaves' colour gone dull on the ground.
fn litter(meshing: &mut Meshing, land: &Land, trees: &[Tree]) {
    /// Places tried about each tree.
    const AROUND: usize = 12;
    for tree in trees {
        let (light, shade) = leaves(&land.pal, tree.kind);
        let tinted = tint(tag(tree.at));
        let h = key(&[tag(tree.at), 11]);
        let roll = |salt: u32| unit(key(&[h, salt]));
        let reach = tree.scale + 0.9;
        // Blossom drops its petals the more thickly.
        let bare = if tree.kind == BLOSSOM { 0.1 } else { 0.45 };
        for i in 0..AROUND {
            let salt = i as u32 * 8;
            if roll(salt) < bare {
                continue;
            }
            let turn = (i as f32 + 0.6 * roll(salt + 1))
                / AROUND as f32
                * TAU;
            let out = reach * (0.55 + 0.5 * roll(salt + 2));
            let p = tree.at.xz() + Vec2::from_angle(turn) * out;
            let y = land.height(p);
            if y < land.set.land.water_y + 0.15
                || land.is_held(p, 0.3)
            {
                continue;
            }
            let up = land.normal(p);
            let (u, w) = up.any_orthonormal_pair();
            let spin = roll(salt + 3) * TAU;
            let along = u * spin.cos() + w * spin.sin();
            // So that the leaf faces up, off the ground.
            let side = along.cross(up);
            let size = 0.35 + 0.25 * roll(salt + 4);
            let stroke = if roll(salt + 5) < 0.5 {
                Stroke::Dab
            } else {
                Stroke::Blot
            };
            let colour =
                shade.lerp(light, roll(salt + 6)) * tinted * 0.8;
            let at = Vec3::new(p.x, y + 0.03, p.y);
            card(
                meshing,
                at - along * size * 0.5,
                along * size,
                side * size,
                stroke,
                colour,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::testing;

    #[test]
    fn held_ground_is_level_and_has_no_trees() {
        let mut land = testing::land(1);
        // The wooded ground with the most trees about it.
        let (lo, hi) = (Vec2::splat(-400.0), Vec2::splat(400.0));
        let wild = trees(&land, lo, hi, 1);
        let near = |at: Vec2, trees: &[Tree]| {
            trees
                .iter()
                .filter(|t| t.at.xz().distance(at) < 9.0)
                .count()
        };
        let at = wild
            .iter()
            .map(|tree| tree.at.xz())
            .max_by_key(|at| near(*at, &wild))
            .unwrap();
        assert!(near(at, &wild) > 3);

        land.hold(&[(at, 9.0)]);
        assert_eq!(near(at, &trees(&land, lo, hi, 1)), 0);
        let level = land.height(at);
        for turn in 0..8 {
            let out = Vec2::from_angle(turn as f32 * 0.8) * 8.5;
            assert!((land.height(at + out) - level).abs() < 1e-4);
        }
        // Let go, the land is as it was.
        land.hold(&[]);
        assert_eq!(trees(&land, lo, hi, 1).len(), wild.len());
    }
}
