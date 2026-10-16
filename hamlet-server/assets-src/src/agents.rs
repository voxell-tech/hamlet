//! The agents' meshes: robots of other shapes, each part built in its
//! joint's frame (see "Agent meshes" in docs/looks.md). Parts a mesh
//! does not name keep the robot's own.

use bevy::prelude::*;
use hamlet_world::dress::Glb;
use hamlet_world::meshes::Mb;

use crate::hamlets::c;

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
