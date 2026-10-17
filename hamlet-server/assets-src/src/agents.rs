//! The agents' meshes: robots of other shapes, each part built in its
//! joint's frame (see "Agent meshes" in docs/looks.md). Parts a mesh
//! does not name keep the robot's own.

use bevy::math::Affine3A;
use bevy::prelude::*;
use hamlet_world::dress::Glb;
use hamlet_world::meshes::Mb;

use crate::hamlets::{c, fleck, toadstool};

/// A robot's replacement parts, by the names the robot finds them by.
pub struct Body(pub Vec<(&'static str, Mesh)>);

impl Body {
    /// The `.glb` an agent wears.
    pub fn glb(&self) -> Vec<u8> {
        self.0
            .iter()
            .fold(Glb::new(), |glb, (name, mesh)| {
                glb.mesh(name, mesh, Transform::IDENTITY)
            })
            .write()
    }
}

/// The body of the asset `id`, if it is one.
pub fn of(id: &str) -> Option<Body> {
    match id {
        "agent-mesh-lantern" => Some(lantern()),
        "agent-mesh-teapot" => Some(teapot()),
        "agent-mesh-stilts" => Some(stilts()),
        "agent-mesh-mushroom" => Some(mushroom()),
        "agent-mesh-snowbot" => Some(snowbot()),
        "agent-mesh-knight" => Some(knight()),
        _ => None,
    }
}

/// A ball of `radius` turned about Y, `rings` from bottom to top,
/// standing on `foot`.
fn ball(
    mb: &mut Mb,
    foot: Vec3,
    radius: f32,
    rings: usize,
    colour: impl Fn(usize) -> Vec3,
) {
    let profile = (0..=rings)
        .map(|k| {
            let a = std::f32::consts::PI * k as f32 / rings as f32;
            (radius * a.sin(), radius * (1.0 - a.cos()))
        })
        .collect::<Vec<_>>();
    mb.lathe(foot, &profile, 14, |ring, _| colour(ring));
}

/// A robot whose head is a red paper lantern, lit from within, on a
/// dark lacquered body.
pub fn lantern() -> Body {
    let lacquer = c("#2a1c18");
    let gold = c("#d9a441");
    let paper = c("#e2483a");
    // The paper globe is the head, ribbed and red; through a slit
    // about its middle shows the light, which is the head's glass and
    // glows with the robot's own.
    let mut head = Mb::new();
    ball(&mut head, Vec3::new(0.0, 0.03, 0.0), 0.15, 12, |ring| {
        if ring % 2 == 0 { paper } else { paper * 0.8 }
    });
    head.tube(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.05, 0.0),
        0.07,
        0.08,
        12,
        lacquer,
        true,
    );
    head.tube(
        Vec3::new(0.0, 0.29, 0.0),
        Vec3::new(0.0, 0.33, 0.0),
        0.08,
        0.06,
        12,
        lacquer,
        true,
    );
    for k in 0..8 {
        let a = k as f32 / 8.0 * std::f32::consts::TAU;
        let out = Vec3::new(a.cos(), 0.0, a.sin());
        head.tube(
            out * 0.1 + Vec3::Y * 0.06,
            out * 0.152 + Vec3::Y * 0.18,
            0.005,
            0.005,
            4,
            gold,
            false,
        );
    }
    let mut glass = Mb::new();
    glass.torus(Vec3::ZERO, Vec3::Y, 0.152, 0.018, 20, c("#ffe0a0"));
    // A loop to carry it by, and a gold tassel, for a hat.
    let mut hat = Mb::new();
    hat.torus(
        Vec3::new(0.0, 0.12, 0.0),
        Vec3::Z,
        0.07,
        0.012,
        10,
        gold,
    );
    hat.tube(
        Vec3::new(0.0, 0.04, 0.0),
        Vec3::new(0.0, 0.06, 0.0),
        0.02,
        0.02,
        6,
        gold,
        true,
    );
    // A dark lacquered body with a gold band.
    let mut torso = Mb::new();
    torso.lathe(
        Vec3::ZERO,
        &[
            (0.0, 0.0),
            (0.2, 0.0),
            (0.24, 0.08),
            (0.24, 0.36),
            (0.2, 0.46),
            (0.08, 0.48),
            (0.0, 0.48),
        ],
        14,
        |ring, _| if ring == 3 { gold } else { lacquer },
    );
    torso.torus(
        Vec3::new(0.0, 0.22, 0.0),
        Vec3::Y,
        0.245,
        0.015,
        16,
        gold,
    );
    Body(vec![
        ("glass", glass.build()),
        ("head", head.build()),
        ("hat", hat.build()),
        ("torso", torso.build()),
    ])
}

/// A round, squat porcelain teapot of a robot: a spout in front, a
/// handle behind, a blue band about it and its lid for a hat.
pub fn teapot() -> Body {
    let porcelain = c("#f2efe6");
    let blue = c("#2f5fa8");
    let mut torso = Mb::new();
    torso.lathe(
        Vec3::ZERO,
        &[
            (0.0, 0.0),
            (0.18, 0.0),
            (0.27, 0.08),
            (0.3, 0.2),
            (0.29, 0.32),
            (0.22, 0.43),
            (0.12, 0.48),
            (0.0, 0.48),
        ],
        16,
        |ring, _| if ring == 3 { blue } else { porcelain },
    );
    // The spout, up and out of the front.
    torso.tube(
        Vec3::new(0.0, 0.16, 0.26),
        Vec3::new(0.0, 0.34, 0.42),
        0.06,
        0.03,
        8,
        porcelain,
        true,
    );
    // The handle, a loop behind.
    torso.torus(
        Vec3::new(0.0, 0.24, -0.3),
        Vec3::X,
        0.1,
        0.022,
        12,
        porcelain,
    );
    // The lid, with a blue knob, for a hat.
    let mut hat = Mb::new();
    hat.lathe(
        Vec3::new(0.0, -0.02, 0.0),
        &[
            (0.0, 0.0),
            (0.17, 0.0),
            (0.15, 0.05),
            (0.08, 0.09),
            (0.0, 0.1),
        ],
        14,
        |_, _| porcelain,
    );
    ball(&mut hat, Vec3::new(0.0, 0.08, 0.0), 0.035, 6, |_| blue);
    // A round porcelain head about the face.
    let mut head = Mb::new();
    ball(&mut head, Vec3::new(0.0, 0.03, 0.0), 0.15, 10, |ring| {
        if ring == 7 { blue } else { porcelain }
    });
    Body(vec![
        ("torso", torso.build()),
        ("hat", hat.build()),
        ("head", head.build()),
    ])
}

/// A tall, thin stilt walker: a narrow striped coat, long wooden
/// stilts with pegs for feet in place of wheels, and a stovepipe hat.
pub fn stilts() -> Body {
    let coat = c("#3d5a3a");
    let stripe = c("#e8d8a8");
    let wood = c("#8a6238");
    let felt = c("#2a2a30");
    let mut torso = Mb::new();
    torso.lathe(
        Vec3::ZERO,
        &[
            (0.0, 0.0),
            (0.12, 0.0),
            (0.16, 0.06),
            (0.17, 0.4),
            (0.14, 0.46),
            (0.0, 0.48),
        ],
        12,
        |_, _| coat,
    );
    for k in 0..5 {
        let y = 0.06 + k as f32 * 0.08;
        torso.torus(
            Vec3::new(0.0, y, 0.0),
            Vec3::Y,
            0.168,
            0.012,
            12,
            stripe,
        );
    }
    // The stilts: the thigh in the coat's green, the long pole below,
    // a strap where they meet.
    let mut leg = Mb::new();
    leg.tube(
        Vec3::new(0.0, 0.03, 0.0),
        Vec3::new(0.0, -0.12, 0.0),
        0.05,
        0.045,
        8,
        coat,
        true,
    );
    leg.tube(
        Vec3::new(0.0, -0.1, 0.0),
        Vec3::new(0.0, -0.31, 0.0),
        0.025,
        0.02,
        6,
        wood,
        true,
    );
    leg.torus(
        Vec3::new(0.0, -0.11, 0.0),
        Vec3::Y,
        0.045,
        0.01,
        8,
        stripe,
    );
    // Small pegs for feet: nothing to roll.
    let mut wheels = Mb::new();
    wheels.tube(
        Vec3::new(0.0, 0.0, -0.03),
        Vec3::new(0.0, 0.0, 0.05),
        0.02,
        0.02,
        6,
        wood * 0.8,
        true,
    );
    // Long thin arms in the coat's sleeves.
    let mut arm = Mb::new();
    arm.tube(
        Vec3::new(0.0, 0.03, 0.0),
        Vec3::new(0.0, -0.3, 0.0),
        0.035,
        0.03,
        8,
        coat,
        true,
    );
    ball(&mut arm, Vec3::new(0.0, -0.34, 0.0), 0.035, 6, |_| stripe);
    // The stovepipe hat, very tall, with a band.
    let mut hat = Mb::new();
    hat.tube(
        Vec3::new(0.0, -0.01, 0.0),
        Vec3::new(0.0, 0.01, 0.0),
        0.2,
        0.2,
        16,
        felt,
        true,
    );
    hat.tube(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.62, 0.0),
        0.12,
        0.13,
        16,
        felt,
        true,
    );
    hat.tube(
        Vec3::new(0.0, 0.06, 0.0),
        Vec3::new(0.0, 0.12, 0.0),
        0.125,
        0.126,
        16,
        stripe,
        false,
    );
    Body(vec![
        ("torso", torso.build()),
        ("leg", leg.build()),
        ("wheels", wheels.build()),
        ("arm", arm.build()),
        ("hat", hat.build()),
    ])
}

/// A toadstool of a robot: a red cap flecked with white for a hat,
/// gills beneath, and a pale, stout stalk of a body with a ring about
/// its top.
pub fn mushroom() -> Body {
    let stalk = c("#efe6d0");
    let mut torso = Mb::new();
    torso.lathe(
        Vec3::ZERO,
        &[
            (0.0, 0.0),
            (0.19, 0.0),
            (0.24, 0.05),
            (0.255, 0.13),
            (0.242, 0.22),
            (0.2, 0.33),
            (0.16, 0.42),
            (0.11, 0.46),
            (0.0, 0.46),
        ],
        16,
        |_, seg| if seg % 2 == 0 { stalk } else { stalk * 0.95 },
    );
    torso.torus(
        Vec3::new(0.0, 0.39, 0.0),
        Vec3::Y,
        0.168,
        0.03,
        16,
        c("#f8f3e8"),
    );
    // Pale stubby arms.
    let mut arm = Mb::new();
    ball(&mut arm, Vec3::new(0.0, -0.055, 0.0), 0.055, 6, |_| stalk);
    arm.tube(
        Vec3::new(0.0, -0.03, 0.0),
        Vec3::new(0.0, -0.27, 0.0),
        0.04,
        0.034,
        8,
        stalk * 0.96,
        false,
    );
    ball(&mut arm, Vec3::new(0.0, -0.35, 0.0), 0.05, 6, |_| {
        c("#d9c7a5")
    });
    // The cap, wider than the shoulders, its gills clear of the head.
    let mut hat = Mb::new();
    toadstool(
        &mut hat,
        Vec3::ZERO,
        (0.32, 0.24),
        (0.04, 0.06),
        9,
        (c("#d23c2f"), c("#e9d6b3"), c("#fbf6ea")),
    );
    Body(vec![
        ("torso", torso.build()),
        ("arm", arm.build()),
        ("hat", hat.build()),
    ])
}

/// A snowman of a robot: a snowball body and head, a red knitted
/// scarf, coal buttons, a carrot nose, a coal smile lit by its light,
/// twig arms in mittens and a battered top hat.
pub fn snowbot() -> Body {
    let snow = c("#f4f6fa");
    let shade = c("#dde5f0");
    let coal = c("#26232a");
    let wool = c("#c8382e");
    let knit = c("#f0e4c8");
    let twig = c("#5a3c26");
    let felt = c("#2a2a30");
    let (middle, round) = (Vec3::new(0.0, 0.23, 0.0), 0.24);
    let mut torso = Mb::new();
    ball(&mut torso, middle - Vec3::Y * round, round, 12, |ring| {
        if ring < 3 { shade } else { snow }
    });
    // The scarf about the neck, striped along its length, and its
    // end hanging over the chest.
    torso.lathe(
        Vec3::ZERO,
        &[
            (0.15, 0.37),
            (0.2, 0.385),
            (0.21, 0.43),
            (0.17, 0.475),
            (0.09, 0.485),
        ],
        16,
        |_, seg| if seg % 4 < 2 { wool } else { knit },
    );
    let side = 0.45f32;
    let on = |e: f32| {
        middle
            + Vec3::new(
                e.cos() * side.sin(),
                e.sin(),
                e.cos() * side.cos(),
            ) * (round + 0.018)
    };
    let ends = [0.78f32, 0.52, 0.26, 0.0];
    for (k, pair) in ends.windows(2).enumerate() {
        let (a, b) = (on(pair[0]), on(pair[1]));
        let mid = (a + b) / 2.0;
        let y = (a - b).normalize();
        let x = y.cross((mid - middle).normalize()).normalize();
        let z = x.cross(y);
        torso.rbox(
            mid,
            Vec3::new(0.05, (a - b).length() / 2.0 + 0.01, 0.014),
            0.008,
            Quat::from_mat3(&Mat3::from_cols(x, y, z)),
            if k % 2 == 0 { wool } else { knit },
            0.0,
        );
    }
    let tip = on(0.0);
    let across = Vec3::new(side.cos(), 0.0, -side.sin());
    for k in 0..4 {
        let at = tip + across * (k as f32 - 1.5) * 0.028;
        torso.tube(
            at,
            at - Vec3::Y * 0.04,
            0.007,
            0.005,
            4,
            wool,
            true,
        );
    }
    // Coal buttons, in the emblem's place.
    let mut emblem = Mb::new();
    for e in [0.45f32, 0.08, -0.29] {
        let up = Vec3::new(0.0, e.sin(), e.cos());
        fleck(
            &mut emblem,
            middle + up * round,
            up,
            (0.026, 0.02, round),
            coal,
        );
    }
    // The snowball head and its carrot nose.
    let face = Vec3::new(0.0, 0.17, 0.0);
    let mut head = Mb::new();
    ball(&mut head, face - Vec3::Y * 0.14, 0.14, 10, |_| snow);
    head.tube(
        face + Vec3::new(0.0, -0.02, 0.12),
        face + Vec3::new(0.0, -0.04, 0.27),
        0.026,
        0.004,
        8,
        c("#e87a22"),
        true,
    );
    // The smile, a curve of coal lit by the robot's light.
    let mut glass = Mb::new();
    for k in 0..5 {
        let turn = (k as f32 - 2.0) * 0.28;
        let e = -0.55 + 0.06 * (k as f32 - 2.0).powi(2);
        let at = Vec3::new(
            e.cos() * turn.sin(),
            e.sin(),
            e.cos() * turn.cos(),
        ) * 0.142;
        ball(&mut glass, at - Vec3::Y * 0.016, 0.016, 5, |_| {
            Vec3::ONE
        });
    }
    // Twig arms in red mittens, from a lump of snow at the shoulder.
    let mut arm = Mb::new();
    ball(&mut arm, Vec3::new(0.0, -0.05, 0.0), 0.05, 6, |_| snow);
    arm.tube(
        Vec3::new(0.0, -0.02, 0.0),
        Vec3::new(0.0, -0.27, 0.0),
        0.022,
        0.018,
        6,
        twig,
        false,
    );
    arm.tube(
        Vec3::new(0.0, -0.13, 0.0),
        Vec3::new(0.0, -0.19, 0.05),
        0.012,
        0.006,
        5,
        twig,
        true,
    );
    arm.blob(
        Vec3::new(0.0, -0.3, 0.0),
        Vec3::new(0.045, 0.055, 0.04),
        5,
        0.0,
        0.0,
        |_| wool,
    );
    arm.blob(
        Vec3::new(0.0, -0.285, 0.04),
        Vec3::new(0.016, 0.026, 0.016),
        4,
        0.0,
        0.0,
        |_| wool,
    );
    arm.torus(
        Vec3::new(0.0, -0.255, 0.0),
        Vec3::Y,
        0.04,
        0.012,
        10,
        knit,
    );
    // Stubby snow legs.
    let mut leg = Mb::new();
    leg.tube(
        Vec3::new(0.0, 0.04, 0.0),
        Vec3::new(0.0, -0.21, 0.0),
        0.065,
        0.06,
        10,
        snow,
        false,
    );
    leg.blob(
        Vec3::new(0.0, -0.25, 0.035),
        Vec3::new(0.088, 0.058, 0.125),
        6,
        0.0,
        0.0,
        |_| shade,
    );
    // A top hat, set at a slant, with a red band and holly berries.
    let mut hat = Mb::new();
    hat.xf = Affine3A::from_rotation_z(-0.12);
    hat.tube(
        Vec3::new(0.0, -0.035, 0.0),
        Vec3::new(0.0, -0.015, 0.0),
        0.17,
        0.17,
        16,
        felt,
        true,
    );
    hat.tube(
        Vec3::new(0.0, -0.03, 0.0),
        Vec3::new(0.0, 0.2, 0.0),
        0.105,
        0.115,
        16,
        felt,
        true,
    );
    hat.tube(
        Vec3::new(0.0, -0.01, 0.0),
        Vec3::new(0.0, 0.035, 0.0),
        0.108,
        0.11,
        16,
        wool,
        false,
    );
    for (x, z) in [(0.05, 0.1), (0.075, 0.085), (0.06, 0.075)] {
        ball(&mut hat, Vec3::new(x, 0.0, z), 0.016, 4, |_| {
            c("#d1202a")
        });
    }
    for turn in [0.3f32, 0.75] {
        let out = Vec3::new(turn.sin(), 0.0, turn.cos());
        hat.blob(
            out * 0.12 + Vec3::Y * 0.03,
            Vec3::new(0.03, 0.012, 0.018),
            4,
            0.0,
            0.0,
            |_| c("#2f6b3a"),
        );
    }
    Body(vec![
        ("torso", torso.build()),
        ("emblem", emblem.build()),
        ("head", head.build()),
        ("glass", glass.build()),
        ("arm", arm.build()),
        ("leg", leg.build()),
        ("hat", hat.build()),
    ])
}

/// A little knight: a steel breastplate under a blue tabard with a
/// gold cross, a great helm whose visor slit shows the robot's light,
/// a red plume, pauldrons, greaves and a shield on the left arm.
pub fn knight() -> Body {
    let steel = c("#aab2bb");
    let dark = c("#666e78");
    let cloth = c("#2f4f9a");
    let gold = c("#d9a441");
    let plume = c("#c8372d");
    let mut torso = Mb::new();
    torso.lathe(
        Vec3::ZERO,
        &[
            (0.0, 0.0),
            (0.17, 0.0),
            (0.215, 0.05),
            (0.245, 0.17),
            (0.24, 0.3),
            (0.2, 0.4),
            (0.12, 0.45),
            (0.0, 0.45),
        ],
        16,
        |_, _| steel,
    );
    for (y, r) in [(0.03, 0.2), (0.075, 0.222)] {
        torso.torus(Vec3::Y * y, Vec3::Y, r, 0.016, 18, dark);
    }
    // The gorget the helm sits on.
    torso.lathe(
        Vec3::ZERO,
        &[(0.13, 0.42), (0.135, 0.46), (0.09, 0.49), (0.0, 0.49)],
        14,
        |_, _| dark,
    );
    // The tabard, front and back, hanging from the chest in three
    // panels each, with a gold hem.
    let (top, hem) = ((0.3, 0.252), (-0.05, 0.266));
    // Leant so that its hem hangs a little out from the waist.
    let lean =
        Quat::from_rotation_x(-(hem.1 - top.1) / (top.0 - hem.0));
    for back in [false, true] {
        for k in [-1.0f32, 0.0, 1.0] {
            let turn = k * 0.52
                + if back { std::f32::consts::PI } else { 0.0 };
            let out = Vec3::new(turn.sin(), 0.0, turn.cos());
            let (a, b) = (
                out * top.1 + Vec3::Y * top.0,
                out * hem.1 + Vec3::Y * hem.0,
            );
            let rot = Quat::from_rotation_y(turn) * lean;
            let half_wide = top.1 * 0.27;
            torso.rbox(
                (a + b) / 2.0,
                Vec3::new(half_wide, (top.0 - hem.0) / 2.0, 0.01),
                0.004,
                rot,
                cloth,
                0.0,
            );
            torso.rbox(
                b + Vec3::Y * 0.012 + out * 0.004,
                Vec3::new(half_wide, 0.012, 0.01),
                0.003,
                rot,
                gold,
                0.0,
            );
        }
    }
    // A gold cross on the tabard, in the emblem's place.
    let mut emblem = Mb::new();
    for (at, half) in [
        (Vec3::new(0.0, 0.15, 0.0), Vec3::new(0.014, 0.085, 0.005)),
        (Vec3::new(0.0, 0.18, 0.0), Vec3::new(0.055, 0.014, 0.005)),
    ] {
        let z = top.1
            + (hem.1 - top.1) * (top.0 - at.y) / (top.0 - hem.0);
        emblem.rbox(
            at + Vec3::Z * (z + 0.012),
            half,
            0.003,
            lean,
            gold,
            0.0,
        );
    }
    // The great helm, a ridge down its face, bands either side of the
    // slit and breaths on its cheek.
    let mut head = Mb::new();
    head.lathe(
        Vec3::ZERO,
        &[
            (0.0, 0.02),
            (0.12, 0.02),
            (0.155, 0.04),
            (0.165, 0.12),
            (0.165, 0.22),
            (0.15, 0.28),
            (0.1, 0.315),
            (0.0, 0.325),
        ],
        16,
        |_, _| steel,
    );
    head.rbox(
        Vec3::new(0.0, 0.16, 0.163),
        Vec3::new(0.012, 0.1, 0.01),
        0.004,
        Quat::IDENTITY,
        steel * 1.08,
        0.0,
    );
    for y in [0.205, 0.145] {
        head.torus(Vec3::Y * y, Vec3::Y, 0.166, 0.011, 20, dark);
    }
    for (turn, y) in [
        (0.35f32, 0.1),
        (0.5, 0.1),
        (0.35, 0.07),
        (0.5, 0.07),
        (0.65, 0.085),
    ] {
        let out = Vec3::new(turn.sin(), 0.0, turn.cos());
        fleck(
            &mut head,
            out * 0.165 + Vec3::Y * y,
            out,
            (0.009, 0.006, 0.165),
            dark * 0.35,
        );
    }
    // The slit, lit from within.
    let mut glass = Mb::new();
    for k in -4..=4 {
        let turn = k as f32 * 0.17;
        glass.rbox(
            Vec3::new(turn.sin(), 0.0, turn.cos()) * 0.166
                + Vec3::Y * 0.005,
            Vec3::new(0.0145, 0.011, 0.006),
            0.0,
            Quat::from_rotation_y(turn),
            Vec3::ONE,
            0.0,
        );
    }
    // The plume, sweeping back from a gold socket.
    let mut hat = Mb::new();
    hat.tube(
        Vec3::new(0.0, 0.02, 0.0),
        Vec3::new(0.0, 0.07, 0.0),
        0.025,
        0.02,
        8,
        gold,
        true,
    );
    for (k, (at, size)) in [
        (Vec3::new(0.0, 0.09, 0.0), Vec3::new(0.04, 0.05, 0.04)),
        (Vec3::new(0.0, 0.15, -0.03), Vec3::new(0.045, 0.05, 0.05)),
        (Vec3::new(0.0, 0.18, -0.09), Vec3::new(0.045, 0.045, 0.055)),
        (Vec3::new(0.0, 0.17, -0.15), Vec3::new(0.04, 0.04, 0.05)),
        (Vec3::new(0.0, 0.13, -0.2), Vec3::new(0.032, 0.04, 0.04)),
        (Vec3::new(0.0, 0.08, -0.23), Vec3::new(0.025, 0.035, 0.03)),
    ]
    .into_iter()
    .enumerate()
    {
        hat.blob(at, size, 5, 0.2, k as f32, |_| plume);
    }
    // Greaves and dark sabatons.
    let mut leg = Mb::new();
    leg.tube(
        Vec3::new(0.0, 0.04, 0.0),
        Vec3::new(0.0, -0.21, 0.0),
        0.058,
        0.05,
        10,
        steel,
        false,
    );
    leg.blob(
        Vec3::new(0.0, -0.08, 0.045),
        Vec3::new(0.04, 0.04, 0.025),
        4,
        0.0,
        0.0,
        |_| steel * 1.08,
    );
    leg.torus(
        Vec3::new(0.0, -0.205, 0.0),
        Vec3::Y,
        0.052,
        0.012,
        10,
        dark,
    );
    leg.blob(
        Vec3::new(0.0, -0.25, 0.035),
        Vec3::new(0.085, 0.055, 0.125),
        6,
        0.0,
        0.0,
        |_| dark,
    );
    Body(vec![
        ("torso", torso.build()),
        ("emblem", emblem.build()),
        ("head", head.build()),
        ("glass", glass.build()),
        ("hat", hat.build()),
        ("arm", knight_arm(false).build()),
        ("arm_l", knight_arm(true).build()),
        ("leg", leg.build()),
    ])
}

/// A knight's arm: a pauldron over the shoulder, plate down to a
/// gauntlet, and on the left a heater shield strapped to its outside.
fn knight_arm(shield: bool) -> Mb {
    let steel = c("#aab2bb");
    let dark = c("#666e78");
    let mut arm = Mb::new();
    arm.lathe(
        Vec3::new(0.0, -0.07, 0.0),
        &[
            (0.0, 0.0),
            (0.095, 0.0),
            (0.1, 0.03),
            (0.09, 0.09),
            (0.055, 0.125),
            (0.0, 0.135),
        ],
        14,
        |_, _| steel,
    );
    arm.torus(
        Vec3::new(0.0, -0.085, 0.0),
        Vec3::Y,
        0.07,
        0.018,
        12,
        steel * 0.9,
    );
    arm.tube(
        Vec3::new(0.0, -0.08, 0.0),
        Vec3::new(0.0, -0.15, 0.0),
        0.042,
        0.04,
        8,
        dark,
        false,
    );
    ball(&mut arm, Vec3::new(0.0, -0.205, 0.0), 0.04, 6, |_| steel);
    arm.tube(
        Vec3::new(0.0, -0.18, 0.0),
        Vec3::new(0.0, -0.26, 0.0),
        0.04,
        0.046,
        8,
        steel,
        false,
    );
    arm.blob(
        Vec3::new(0.0, -0.3, 0.0),
        Vec3::new(0.05, 0.055, 0.05),
        5,
        0.0,
        0.0,
        |_| dark,
    );
    if shield {
        heater(&mut arm, Vec3::new(-0.065, -0.15, 0.0));
    }
    arm
}

/// A heater shield facing -X about `at`: blue, rimmed in gold, a gold
/// cross on its face and steel behind.
fn heater(mb: &mut Mb, at: Vec3) {
    let (blue, gold, steel) =
        (c("#2f4f9a"), c("#d9a441"), c("#8d959e"));
    let outline = [
        (-0.1, 0.1),
        (0.1, 0.1),
        (0.1, 0.0),
        (0.085, -0.06),
        (0.055, -0.11),
        (0.0, -0.15),
        (-0.055, -0.11),
        (-0.085, -0.06),
        (-0.1, 0.0),
    ]
    .map(|(z, y)| Vec2::new(z, y));
    let (front, back) = (-0.013, 0.013);
    let point = |p: Vec2, x: f32| at + Vec3::new(x, p.y, p.x);
    for (x, n, colour) in
        [(front, -Vec3::X, blue), (back, Vec3::X, steel)]
    {
        let centre = mb.v(point(Vec2::new(0.0, -0.01), x), n, colour);
        let rim = outline.map(|p| mb.v(point(p, x), n, colour));
        for k in 0..rim.len() {
            mb.tri(centre, rim[k], rim[(k + 1) % rim.len()]);
        }
    }
    for k in 0..outline.len() {
        let (p, q) = (outline[k], outline[(k + 1) % outline.len()]);
        let out = (p + q) / 2.0 - Vec2::new(0.0, -0.01);
        let n = Vec3::new(0.0, out.y, out.x);
        mb.face(
            [
                point(p, front),
                point(q, front),
                point(q, back),
                point(p, back),
            ],
            n,
            gold,
        );
    }
    for (c_, half) in [
        (Vec2::new(0.0, -0.025), Vec2::new(0.012, 0.095)),
        (Vec2::new(0.0, 0.035), Vec2::new(0.07, 0.012)),
    ] {
        mb.rbox(
            point(c_, front - 0.004),
            Vec3::new(0.004, half.y, half.x),
            0.002,
            Quat::IDENTITY,
            gold,
            0.0,
        );
    }
}
