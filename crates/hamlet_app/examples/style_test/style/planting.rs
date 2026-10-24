//! The style test's fence, trees, bushes, grass and flowers.

use std::f32::consts::TAU;

use bevy::prelude::*;
use hamlet_assets::tint::hex;
use hamlet_realm::meshes::{Meshing, Rng, noise2, smoothstep};
use hamlet_realm::palette::Palette;

use super::scene::{Built, Terrain};

pub(super) fn fence(
    meshing: &mut Meshing,
    t: &Terrain,
    pal: &Palette,
) {
    let mut rng = Rng::seeded("fence along the south verge");
    let mut tops: Vec<Vec3> = Vec::new();
    let mut s = 0.4;
    let bridge_s = t.path.nearest(t.bridge_c.x, t.bridge_c.z).1;
    while s < bridge_s - t.at.bridge_half - 0.9 {
        let (p, tan) = t.path.at(s);
        let n = Vec3::new(-tan.z, 0.0, tan.x);
        let foot = p + n * (1.05 + rng.sym(0.05));
        let y = t.ground_y(foot.x, foot.z);
        let h = 0.62 * rng.value(0.1);
        let rot = rng.wobble(4.0);
        let col =
            pal.timber.lerp(hex("#8C7A5A"), 0.35) * rng.value(0.1);
        let c = Vec3::new(foot.x, y + h * 0.5 - 0.04, foot.z);
        meshing.rbox(
            c,
            Vec3::new(0.05, h * 0.5 + 0.04, 0.05),
            0.02,
            rot * Quat::from_rotation_y(tan.x.atan2(tan.z)),
            col,
            0.4,
        );
        tops.push(Vec3::new(foot.x, y, foot.z));
        s += 1.0 * rng.value(0.15);
    }
    for (i, w) in tops.windows(2).enumerate() {
        for (k, h) in [0.22f32, 0.44].into_iter().enumerate() {
            // One rail has gone missing.
            if i == 3 && k == 1 {
                continue;
            }
            let (a, c) = (
                w[0] + Vec3::Y * (h + rng.sym(0.03)),
                w[1] + Vec3::Y * (h + rng.sym(0.03)),
            );
            let d = c - a;
            let rot = Quat::from_rotation_arc(Vec3::X, d.normalize());
            let col = pal.timber.lerp(hex("#8C7A5A"), 0.45)
                * rng.value(0.1);
            meshing.rbox(
                (a + c) * 0.5 + Vec3::Z * 0.05,
                Vec3::new(d.length() * 0.5 + 0.06, 0.035, 0.018),
                0.012,
                rot,
                col,
                0.2,
            );
        }
    }
}

fn tree(
    b: &mut Built,
    rng: &mut Rng,
    pal: &Palette,
    foot: Vec3,
    height: f32,
    crown: f32,
    autumn: bool,
) {
    let bark = hex("#6B5540");
    let bend = Vec3::new(rng.sym(0.25), 0.0, rng.sym(0.25));
    let (p0, p1, p2) = (
        foot - Vec3::Y * 0.2,
        foot + Vec3::Y * height * 0.45 + bend,
        foot + Vec3::Y * height + bend * 0.4,
    );
    b.solid.tube(
        p0,
        p1,
        height * 0.085,
        height * 0.06,
        9,
        bark,
        false,
    );
    b.solid.tube(
        p1,
        p2,
        height * 0.06,
        height * 0.035,
        9,
        bark * 1.08,
        false,
    );
    let (light, shade) = if autumn {
        (hex("#E8A45C"), hex("#C9713A"))
    } else {
        (pal.foliage, pal.foliage_shade)
    };
    let lumps = 7;
    for i in 0..lumps {
        let a = i as f32 / (lumps - 1) as f32 * TAU + rng.f();
        let out = if i == 0 { 0.0 } else { crown * 0.62 };
        let c = p2
            + Vec3::new(
                a.cos() * out,
                rng.range(-0.1, 0.45) * crown
                    + if i == 0 { crown * 0.45 } else { 0.0 },
                a.sin() * out,
            );
        let r = crown * rng.range(0.62, 0.85);
        let tint = rng.value(0.07);
        let col = move |d: Vec3| {
            shade.lerp(light, smoothstep(-0.5, 0.75, d.y)) * tint
        };
        b.leaf.blob(
            c,
            Vec3::new(r, r * 0.85, r),
            7,
            0.16,
            rng.f() * 40.0,
            col,
        );
        // Leaves stand off the lump so the outline is not a ball.
        for _ in 0..90 {
            let d = Vec3::new(
                rng.sym(1.0),
                rng.range(-0.55, 1.0),
                rng.sym(1.0),
            )
            .normalize_or_zero();
            let p = c + d
                * Vec3::new(r, r * 0.85, r)
                * rng.range(0.96, 1.08);
            let size = rng.range(0.11, 0.2) * (crown / 1.4).max(0.6);
            let (u, w) = d.any_orthonormal_pair();
            let spin = rng.f() * TAU;
            let (u, w) = (
                u * spin.cos() + w * spin.sin(),
                w * spin.cos() - u * spin.sin(),
            );
            let lift = (d + Vec3::Y * 0.5).normalize();
            let leaf = shade.lerp(
                light,
                smoothstep(-0.4, 0.7, d.y) * rng.range(0.7, 1.25),
            ) * tint;
            let v = [
                p - u * size,
                p + w * size * 0.55 + d * size * 0.3,
                p + u * size + d * size * 0.45,
                p - w * size * 0.55 + d * size * 0.3,
            ]
            .map(|q| b.leaf.v(q, lift, leaf));
            b.leaf.quad(v[0], v[1], v[2], v[3]);
        }
    }
}

fn bush(
    b: &mut Built,
    rng: &mut Rng,
    pal: &Palette,
    c: Vec3,
    r: f32,
) {
    let (light, shade) = (pal.foliage, pal.foliage_shade);
    for _ in 0..3 {
        let o =
            Vec3::new(rng.sym(r * 0.6), r * 0.35, rng.sym(r * 0.6));
        let rr = r * rng.range(0.6, 0.9);
        let tint = rng.value(0.08);
        b.leaf.blob(
            c + o,
            Vec3::new(rr, rr * 0.75, rr),
            6,
            0.2,
            rng.f() * 30.0,
            move |d| {
                shade.lerp(light, smoothstep(-0.4, 0.8, d.y)) * tint
            },
        );
        for _ in 0..40 {
            let d = Vec3::new(
                rng.sym(1.0),
                rng.range(-0.2, 1.0),
                rng.sym(1.0),
            )
            .normalize_or_zero();
            let p = c + o + d * Vec3::new(rr, rr * 0.75, rr) * 1.02;
            let size = rng.range(0.07, 0.12);
            let (u, w) = d.any_orthonormal_pair();
            let leaf = shade.lerp(
                light,
                smoothstep(-0.2, 0.8, d.y) * rng.range(0.8, 1.3),
            ) * tint;
            let lift = (d + Vec3::Y * 0.5).normalize();
            let v = [
                p - u * size,
                p + w * size * 0.6 + d * size * 0.3,
                p + u * size + d * size * 0.4,
                p - w * size * 0.6 + d * size * 0.3,
            ]
            .map(|q| b.leaf.v(q, lift, leaf));
            b.leaf.quad(v[0], v[1], v[2], v[3]);
        }
    }
}

pub(super) fn planting(b: &mut Built, t: &Terrain, pal: &Palette) {
    let mut rng = Rng::seeded("meadow planting");
    let on_ground =
        |x: f32, z: f32| Vec3::new(x, t.ground_y(x, z), z);
    let clear = |x: f32, z: f32, margin: f32| {
        let stall = (x - t.at.stall.x).abs() < 2.0 + margin
            && (z - t.at.stall.z).abs() < 1.4 + margin;
        let step = (x - t.at.doorstep.x).abs() < 1.0 + margin
            && (z - t.at.doorstep.z).abs() < 0.8 + margin;
        let house = (x - t.at.cottage.x).abs() < 2.0 + margin
            && (z - t.at.cottage.z).abs() < 1.7 + margin;
        let post =
            Vec2::new(x - t.at.signal_post.x, z - t.at.signal_post.z)
                .length()
                < 0.3 + margin;
        let canal = t.canal.nearest(x, z).0 < 1.2 + margin;
        !(stall || step || house || post || canal)
            && t.dirt(x, z) < 0.25
    };

    for (x, z, h, r, autumn) in [
        (-8.6, -5.4, 3.3, 1.75, false),
        (-11.5, -1.5, 2.9, 1.5, false),
        (7.6, -7.4, 3.6, 1.9, false),
        (3.4, -9.6, 3.0, 1.6, true),
        (9.6, -2.6, 2.7, 1.4, false),
        (9.3, 4.8, 3.0, 1.65, false),
        (-9.4, 8.4, 2.8, 1.5, true),
        (-14.5, -8.5, 3.4, 1.8, false),
        (13.0, -9.5, 3.2, 1.7, false),
        (-4.5, -12.0, 3.4, 1.8, false),
        (15.0, 1.0, 3.0, 1.6, true),
        (-16.0, 3.0, 3.0, 1.6, false),
    ] {
        tree(b, &mut rng, pal, on_ground(x, z), h, r, autumn);
    }
    for (x, z, r) in [
        (1.2, -3.2, 0.55),
        (6.4, -4.2, 0.6),
        (-5.6, 0.4, 0.5),
        (0.4, 3.6, 0.5),
        (-3.4, -6.4, 0.6),
        (6.9, 1.8, 0.55),
        (-7.4, 6.4, 0.6),
        (1.4, -8.2, 0.5),
    ] {
        bush(b, &mut rng, pal, on_ground(x, z), r);
    }

    // Rocks: a few by the water and in the open.
    for _ in 0..16 {
        let (x, z) = (rng.range(-12.0, 10.0), rng.range(-10.0, 8.0));
        if !clear(x, z, -0.1) {
            continue;
        }
        let r = rng.range(0.12, 0.3);
        let col = pal.rock * rng.value(0.1);
        let shade = hex("#6E7064");
        b.solid.blob(
            on_ground(x, z) + Vec3::Y * r * 0.2,
            Vec3::new(r, r * 0.6, r * rng.range(0.7, 1.0)),
            4,
            0.3,
            rng.f() * 50.0,
            move |d| shade.lerp(col, smoothstep(-0.3, 0.6, d.y)),
        );
    }

    // Grass: denser by the path and the water, where feet and scythes
    // do not go.
    let tip = hex("#A9C96A");
    let mut placed = 0;
    while placed < 5200 {
        let (x, z) = (rng.range(-15.0, 12.5), rng.range(-12.0, 10.5));
        placed += 1;
        if !clear(x, z, -0.25) {
            continue;
        }
        let near_edge = smoothstep(2.2, 0.7, t.path.nearest(x, z).0)
            .max(smoothstep(2.6, 1.4, t.canal.nearest(x, z).0));
        if rng.f()
            > 0.3
                + 0.7 * near_edge.max(noise2(x * 0.5 + 7.0, z * 0.5))
        {
            continue;
        }
        let base = on_ground(x, z);
        let ground = t.grass_colour(pal, x, z);
        let blades = 3 + (rng.f() * 3.0) as usize;
        for _ in 0..blades {
            let a = rng.f() * TAU;
            let lean = Vec3::new(a.cos(), 0.0, a.sin());
            let h = rng.range(0.12, 0.26) * (0.8 + 0.6 * near_edge);
            let side = Vec3::new(-lean.z, 0.0, lean.x)
                * rng.range(0.03, 0.05);
            let foot = base + lean * rng.range(0.0, 0.07);
            let top =
                foot + Vec3::Y * h + lean * h * rng.range(0.15, 0.55);
            let n = Vec3::Y;
            let v0 = b.tuft.v(foot - side, n, ground * 0.95);
            let v1 = b.tuft.v(foot + side, n, ground * 0.95);
            let v2 = b.tuft.v(
                top,
                n,
                ground.lerp(tip, rng.range(0.3, 0.9)),
            );
            b.tuft.tri(v0, v1, v2);
        }
    }

    // Flowers in drifts: lupins in two blues and white daisies.
    let drifts = [
        (-5.3, 4.1, 0),
        (-1.2, 3.5, 1),
        (-0.7, 0.9, 2),
        (-5.1, 1.2, 0),
        (2.0, -1.9, 2),
        (5.8, -3.2, 1),
        (0.9, -5.2, 0),
        (-3.6, -4.9, 1),
        (6.3, 3.1, 0),
        (3.2, 4.6, 2),
        (-7.8, 1.2, 2),
        (-8.8, 6.2, 1),
        (5.9, 6.4, 0),
        (-2.2, 6.9, 2),
        (7.4, -0.6, 2),
        (-6.4, -2.6, 0),
        (1.9, 6.6, 1),
        (-11.0, 2.2, 1),
        (4.6, 0.9, 0),
        (-1.0, -3.4, 2),
    ];
    for (cx, cz, kind) in drifts {
        let n = 9 + (rng.f() * 8.0) as usize;
        for _ in 0..n {
            let (x, z) = (cx + rng.sym(0.8), cz + rng.sym(0.6));
            if !clear(x, z, -0.2) {
                continue;
            }
            let p = on_ground(x, z);
            let stem = pal.foliage_shade;
            match kind {
                0 | 1 => {
                    let col = if kind == 0 {
                        hex("#7B5BB5")
                    } else {
                        hex("#6F8FD8")
                    } * rng.value(0.12);
                    let h = rng.range(0.32, 0.55);
                    let lean =
                        Vec3::new(rng.sym(0.06), 0.0, rng.sym(0.06));
                    b.tuft.tube(
                        p,
                        p + Vec3::Y * h * 0.4 + lean,
                        0.012,
                        0.012,
                        4,
                        stem,
                        false,
                    );
                    b.tuft.tube(
                        p + Vec3::Y * h * 0.35 + lean,
                        p + Vec3::Y * h + lean * 2.0,
                        0.045,
                        0.008,
                        6,
                        col,
                        false,
                    );
                    b.tuft.tube(
                        p + Vec3::Y * h * 0.72 + lean * 1.6,
                        p + Vec3::Y * h * 1.02 + lean * 2.0,
                        0.026,
                        0.004,
                        5,
                        col.lerp(Vec3::ONE, 0.35),
                        false,
                    );
                }
                _ => {
                    let h = rng.range(0.16, 0.3);
                    b.tuft.tube(
                        p,
                        p + Vec3::Y * h,
                        0.01,
                        0.008,
                        4,
                        stem,
                        false,
                    );
                    let white = hex("#F4F0E2") * rng.value(0.05);
                    b.tuft.blob(
                        p + Vec3::Y * h,
                        Vec3::new(0.055, 0.035, 0.055),
                        3,
                        0.0,
                        0.0,
                        move |d| {
                            if d.y > 0.8 {
                                hex("#E8C24A")
                            } else {
                                white
                            }
                        },
                    );
                }
            }
        }
    }
}
