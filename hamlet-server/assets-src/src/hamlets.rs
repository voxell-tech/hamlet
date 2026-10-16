//! The hamlets' meshes: houses in other styles, built in the frame a
//! hamlet's mesh is authored in (see docs/looks.md): ground at y 0,
//! the front and its door facing +Z, walls about 1 from the middle
//! each way, one storey with its walls about 1.9 high.

use bevy::prelude::*;
use hamlet_world::dress::Glb;
use hamlet_world::meshes::Mb;

/// A house as a hamlet's mesh holds it: its walls and windows, and
/// the spots the world hangs its door lamp, banner, smoke and
/// lanterns from.
pub struct House {
    pub walls: Mesh,
    pub panes: Mesh,
    pub door: Vec3,
    pub lamp: Vec3,
    pub banner: Vec3,
    pub chimney: Vec3,
    pub eaves: Vec3,
}

impl House {
    /// The `.glb` a hamlet wears.
    pub fn glb(&self) -> Vec<u8> {
        let at = Transform::from_translation;
        Glb::new()
            .mesh("walls", &self.walls, Transform::IDENTITY)
            .mesh("panes", &self.panes, Transform::IDENTITY)
            .spot("door", at(self.door))
            .spot("lamp", at(self.lamp))
            .spot("banner", at(self.banner))
            .spot("chimney", at(self.chimney))
            .spot("eaves", at(self.eaves))
            .write()
    }
}

/// An sRGB `#rrggbb` as the linear colour meshes are built in.
pub fn c(hex: &str) -> Vec3 {
    LinearRgba::from(Srgba::hex(hex).unwrap_or(Srgba::WHITE))
        .to_vec3()
}

/// The house of the asset `id`, if it is one.
pub fn of(id: &str) -> Option<House> {
    match id {
        "hamlet-mesh-minka" => Some(minka()),
        "hamlet-mesh-stave" => Some(stave()),
        "hamlet-mesh-adobe" => Some(adobe()),
        _ => None,
    }
}

/// A roof's plane from an eave to the ridge: a thick slab between
/// two lines along X, at heights and depths `(eave_y, eave_z)` and
/// `(ridge_y, ridge_z)`, `wide` from the middle each way.
fn pitch(
    mb: &mut Mb,
    (eave_y, eave_z): (f32, f32),
    (ridge_y, ridge_z): (f32, f32),
    (wide, thick): (f32, f32),
    colour: Vec3,
) {
    let (from, to) = (
        Vec3::new(0.0, eave_y, eave_z),
        Vec3::new(0.0, ridge_y, ridge_z),
    );
    let along = (to - from).normalize();
    // Turned about X so that its own Z runs from the eave to the
    // ridge.
    let rot = Quat::from_rotation_x((-along.y).atan2(along.z));
    let along = to - from;
    mb.rbox(
        (from + to) / 2.0,
        Vec3::new(wide, thick, along.length() / 2.0),
        0.03,
        rot,
        colour,
        0.15,
    );
}

/// A hip end of a roof: a plane from an eave at `x` up to the ridge
/// at `ridge_x`, `deep` from the middle each way.
fn hip(
    mb: &mut Mb,
    (eave_y, eave_x): (f32, f32),
    (ridge_y, ridge_x): (f32, f32),
    (deep, thick): (f32, f32),
    colour: Vec3,
) {
    let (from, to) = (
        Vec3::new(eave_x, eave_y, 0.0),
        Vec3::new(ridge_x, ridge_y, 0.0),
    );
    // Turned about Z so that its own X runs from the eave to the
    // ridge.
    let along = to - from;
    let rot = Quat::from_rotation_z(along.y.atan2(along.x));
    mb.rbox(
        (from + to) / 2.0,
        Vec3::new(along.length() / 2.0, thick, deep),
        0.03,
        rot,
        colour,
        0.15,
    );
}

/// A Japanese minka: a stone plinth, a dark timber frame over pale
/// plaster, a veranda along its front, paper screens, and a deep,
/// thick thatched hip roof with a dark ridge.
pub fn minka() -> House {
    let (mut mb, mut panes) = (Mb::new(), Mb::new());
    let stone = c("#8c8a80");
    let timber = c("#3a2a1e");
    let plaster = c("#ece4d0");
    let deck = c("#6e5038");
    let thatch = c("#b39457");
    let ridge = c("#2e2a24");
    // The plinth and the veranda along the front.
    mb.rbox(
        Vec3::new(0.0, 0.12, -0.05),
        Vec3::new(0.95, 0.12, 0.85),
        0.04,
        Quat::IDENTITY,
        stone,
        0.3,
    );
    mb.rbox(
        Vec3::new(0.0, 0.3, 0.88),
        Vec3::new(0.95, 0.04, 0.16),
        0.01,
        Quat::IDENTITY,
        deck,
        0.2,
    );
    // The walls, and posts at the corners and between.
    let (wx, wz, top) = (0.85, 0.72, 1.7);
    mb.rbox(
        Vec3::new(0.0, (0.24 + top) / 2.0, -0.05),
        Vec3::new(wx, (top - 0.24) / 2.0, wz),
        0.01,
        Quat::IDENTITY,
        plaster,
        0.1,
    );
    for x in [-wx, -wx / 3.0, wx / 3.0, wx] {
        for z in [-wz - 0.05, wz - 0.05] {
            mb.rbox(
                Vec3::new(x, (0.24 + top) / 2.0, z),
                Vec3::new(0.05, (top - 0.24) / 2.0, 0.05),
                0.01,
                Quat::IDENTITY,
                timber,
                0.1,
            );
        }
    }
    // A beam round the top, and one along the middle.
    for y in [top - 0.04, 0.95] {
        mb.rbox(
            Vec3::new(0.0, y, wz - 0.03),
            Vec3::new(wx + 0.05, 0.04, 0.03),
            0.01,
            Quat::IDENTITY,
            timber,
            0.0,
        );
        mb.rbox(
            Vec3::new(0.0, y, -wz - 0.07),
            Vec3::new(wx + 0.05, 0.04, 0.03),
            0.01,
            Quat::IDENTITY,
            timber,
            0.0,
        );
    }
    // Sliding paper screens either side of the door, lit at night.
    for x in [-0.55, 0.55] {
        panes.rbox(
            Vec3::new(x, 0.95, wz - 0.03),
            Vec3::new(0.2, 0.36, 0.02),
            0.0,
            Quat::IDENTITY,
            c("#f7efd8"),
            0.0,
        );
        for k in [-1.0f32, 0.0, 1.0] {
            mb.rbox(
                Vec3::new(x + k * 0.1, 0.95, wz - 0.01),
                Vec3::new(0.008, 0.36, 0.01),
                0.0,
                Quat::IDENTITY,
                timber,
                0.0,
            );
        }
    }
    // The door: a dark sliding door in the middle.
    mb.rbox(
        Vec3::new(0.0, 0.78, wz - 0.02),
        Vec3::new(0.2, 0.5, 0.025),
        0.01,
        Quat::IDENTITY,
        timber * 0.8,
        0.0,
    );
    // The thatch: front and back the whole width, a low hipped skirt
    // at each end under the gables: an irimoya roof.
    let (eave, ridge_y, over) = (1.45, 2.75, 1.12);
    pitch(&mut mb, (eave, over), (ridge_y, 0.0), (1.1, 0.16), thatch);
    pitch(
        &mut mb,
        (eave, -over - 0.1),
        (ridge_y, -0.05),
        (1.1, 0.16),
        thatch,
    );
    for side in [1.0f32, -1.0] {
        hip(
            &mut mb,
            (eave, 1.22 * side),
            (eave + 0.55, 0.95 * side),
            (1.0, 0.14),
            thatch,
        );
        // The gable above the skirt, filled with dark timber.
        for step in 0..5 {
            let h = eave + 0.5 + step as f32 * 0.17;
            let half = 0.95 * (1.0 - (step as f32 + 0.5) / 5.5);
            mb.rbox(
                Vec3::new(0.92 * side, h + 0.085, -0.03),
                Vec3::new(0.04, 0.085, half),
                0.01,
                Quat::IDENTITY,
                timber,
                0.0,
            );
        }
    }
    // The dark ridge on top, its ends turned up.
    mb.rbox(
        Vec3::new(0.0, ridge_y + 0.08, -0.03),
        Vec3::new(1.1, 0.08, 0.1),
        0.03,
        Quat::IDENTITY,
        ridge,
        0.0,
    );
    for x in [-1.1f32, 1.1] {
        mb.rbox(
            Vec3::new(x, ridge_y + 0.16, -0.03),
            Vec3::new(0.06, 0.12, 0.1),
            0.02,
            Quat::from_rotation_z(x.signum() * -0.4),
            ridge,
            0.0,
        );
    }
    House {
        walls: mb.build(),
        panes: panes.build(),
        door: Vec3::new(0.0, 0.3, wz + 0.25),
        lamp: Vec3::new(0.32, 1.15, wz + 0.05),
        banner: Vec3::new(0.65, ridge_y - 0.1, -0.03),
        // The smoke of the hearth leaves by the ridge's end.
        chimney: Vec3::new(-0.9, ridge_y + 0.2, -0.03),
        eaves: Vec3::new(0.0, eave - 0.05, over - 0.05),
    }
}

/// A Nordic stave cabin: tarred log walls on a stone footing, a steep
/// shingled roof whose gables end in carved heads, and a stone
/// chimney at the back.
pub fn stave() -> House {
    let (mut mb, mut panes) = (Mb::new(), Mb::new());
    let stone = c("#7a7a74");
    let logs = c("#4a3322");
    let dark = c("#2b1f17");
    let shingle = c("#5a4636");
    let red = c("#8a2b20");
    mb.rbox(
        Vec3::new(0.0, 0.1, 0.0),
        Vec3::new(0.95, 0.1, 0.85),
        0.03,
        Quat::IDENTITY,
        stone,
        0.3,
    );
    // The logs, laid one on another, each a little proud.
    let (wx, wz, top) = (0.85, 0.75, 1.55);
    let mut y = 0.2;
    let mut k = 0;
    while y < top {
        let shade = if k % 2 == 0 { logs } else { logs * 0.85 };
        mb.tube(
            Vec3::new(-wx - 0.06, y + 0.07, wz),
            Vec3::new(wx + 0.06, y + 0.07, wz),
            0.075,
            0.075,
            6,
            shade,
            true,
        );
        mb.tube(
            Vec3::new(-wx - 0.06, y + 0.07, -wz),
            Vec3::new(wx + 0.06, y + 0.07, -wz),
            0.075,
            0.075,
            6,
            shade,
            true,
        );
        mb.tube(
            Vec3::new(wx, y + 0.07, -wz - 0.06),
            Vec3::new(wx, y + 0.07, wz + 0.06),
            0.075,
            0.075,
            6,
            shade,
            true,
        );
        mb.tube(
            Vec3::new(-wx, y + 0.07, -wz - 0.06),
            Vec3::new(-wx, y + 0.07, wz + 0.06),
            0.075,
            0.075,
            6,
            shade,
            true,
        );
        y += 0.14;
        k += 1;
    }
    // The gable ends at either side, filled under the roof with logs
    // that grow shorter as they rise.
    let (ridge_y, eave, over) = (3.05, 1.45, 1.12);
    let mut h = top;
    while h < ridge_y - 0.15 {
        // The roof's underside, as deep from the middle as it is at
        // this height, less the slab.
        let half =
            over * (ridge_y - h - 0.12) / (ridge_y - eave) - 0.08;
        if half > 0.05 {
            for x in [wx, -wx] {
                mb.rbox(
                    Vec3::new(x, h + 0.07, 0.0),
                    Vec3::new(0.07, 0.07, half.min(wz)),
                    0.02,
                    Quat::IDENTITY,
                    logs * 0.9,
                    0.0,
                );
            }
        }
        h += 0.14;
    }
    // A door with a red frame, and small square windows.
    mb.rbox(
        Vec3::new(0.0, 0.7, wz + 0.07),
        Vec3::new(0.24, 0.52, 0.03),
        0.02,
        Quat::IDENTITY,
        red,
        0.0,
    );
    mb.rbox(
        Vec3::new(0.0, 0.66, wz + 0.09),
        Vec3::new(0.18, 0.46, 0.03),
        0.02,
        Quat::IDENTITY,
        dark,
        0.0,
    );
    for x in [-0.55, 0.55] {
        mb.rbox(
            Vec3::new(x, 1.0, wz + 0.07),
            Vec3::new(0.17, 0.17, 0.03),
            0.01,
            Quat::IDENTITY,
            red,
            0.0,
        );
        panes.rbox(
            Vec3::new(x, 1.0, wz + 0.09),
            Vec3::new(0.12, 0.12, 0.02),
            0.0,
            Quat::IDENTITY,
            c("#ffd98a"),
            0.0,
        );
    }
    // The steep roof, its eaves well past the walls.
    pitch(
        &mut mb,
        (eave, over),
        (ridge_y, 0.0),
        (1.05, 0.08),
        shingle,
    );
    pitch(
        &mut mb,
        (eave, -over),
        (ridge_y, 0.0),
        (1.05, 0.08),
        shingle,
    );
    // The carved heads where the barge boards cross over the ridge.
    for x in [-1.08f32, 1.08] {
        for z in [-0.18f32, 0.18] {
            mb.rbox(
                Vec3::new(x, ridge_y + 0.22, z),
                Vec3::new(0.03, 0.26, 0.04),
                0.02,
                Quat::from_rotation_x(z.signum() * 0.5),
                dark,
                0.0,
            );
        }
        mb.rbox(
            Vec3::new(x, ridge_y + 0.46, 0.0),
            Vec3::new(0.03, 0.06, 0.12),
            0.02,
            Quat::IDENTITY,
            red,
            0.0,
        );
    }
    // The stone chimney at the back.
    let chimney = Vec3::new(0.5, ridge_y + 0.15, -0.45);
    mb.rbox(
        Vec3::new(chimney.x, (chimney.y + 1.2) / 2.0, chimney.z),
        Vec3::new(0.14, (chimney.y - 1.2) / 2.0, 0.14),
        0.03,
        Quat::IDENTITY,
        stone,
        0.2,
    );
    House {
        walls: mb.build(),
        panes: panes.build(),
        door: Vec3::new(0.0, 0.2, wz + 0.12),
        lamp: Vec3::new(0.38, 1.1, wz + 0.12),
        banner: Vec3::new(-1.0, ridge_y - 0.2, 0.0),
        chimney,
        eaves: Vec3::new(0.0, eave - 0.05, over - 0.05),
    }
}

/// A desert adobe: rounded earthen walls, a flat roof behind a low
/// parapet, beam ends out of the wall, a smaller room stepped on top,
/// deep windows and a blue door, and a round clay chimney.
pub fn adobe() -> House {
    let (mut mb, mut panes) = (Mb::new(), Mb::new());
    let earth = c("#c98d5a");
    let shade = c("#a8703f");
    let beam = c("#5e4026");
    let blue = c("#2f6f9f");
    let (wx, wz, top) = (0.9, 0.8, 1.55);
    mb.rbox(
        Vec3::new(0.0, top / 2.0, 0.0),
        Vec3::new(wx, top / 2.0, wz),
        0.22,
        Quat::IDENTITY,
        earth,
        0.25,
    );
    // The parapet round the flat roof.
    for (c_, h) in [
        (
            Vec3::new(0.0, top + 0.1, wz - 0.05),
            Vec3::new(wx, 0.1, 0.05),
        ),
        (
            Vec3::new(0.0, top + 0.1, -wz + 0.05),
            Vec3::new(wx, 0.1, 0.05),
        ),
        (
            Vec3::new(wx - 0.05, top + 0.1, 0.0),
            Vec3::new(0.05, 0.1, wz),
        ),
        (
            Vec3::new(-wx + 0.05, top + 0.1, 0.0),
            Vec3::new(0.05, 0.1, wz),
        ),
    ] {
        mb.rbox(c_, h, 0.05, Quat::IDENTITY, earth, 0.0);
    }
    // The room on top, set back, with its own parapet.
    let up = top + 0.85;
    mb.rbox(
        Vec3::new(-0.3, (top + up) / 2.0, -0.2),
        Vec3::new(0.45, (up - top) / 2.0, 0.42),
        0.16,
        Quat::IDENTITY,
        shade,
        0.2,
    );
    // The beam ends, out of the front wall under the roof and the
    // room's.
    for k in 0..6 {
        let x = -wx + 0.18 + k as f32 * (2.0 * wx - 0.36) / 5.0;
        mb.tube(
            Vec3::new(x, top - 0.12, wz - 0.1),
            Vec3::new(x, top - 0.12, wz + 0.18),
            0.045,
            0.045,
            6,
            beam,
            true,
        );
    }
    for k in 0..3 {
        let x = -0.62 + k as f32 * 0.32;
        mb.tube(
            Vec3::new(x, up - 0.1, 0.12),
            Vec3::new(x, up - 0.1, 0.32),
            0.035,
            0.035,
            6,
            beam,
            true,
        );
    }
    // A blue door in a deep frame, and small deep windows.
    mb.rbox(
        Vec3::new(0.15, 0.6, wz + 0.01),
        Vec3::new(0.24, 0.6, 0.03),
        0.06,
        Quat::IDENTITY,
        shade,
        0.0,
    );
    mb.rbox(
        Vec3::new(0.15, 0.55, wz + 0.03),
        Vec3::new(0.18, 0.55, 0.03),
        0.02,
        Quat::IDENTITY,
        blue,
        0.0,
    );
    for x in [-0.5, 0.62] {
        mb.rbox(
            Vec3::new(x, 1.0, wz + 0.01),
            Vec3::new(0.15, 0.17, 0.03),
            0.04,
            Quat::IDENTITY,
            shade,
            0.0,
        );
        panes.rbox(
            Vec3::new(x, 1.0, wz + 0.03),
            Vec3::new(0.1, 0.12, 0.02),
            0.0,
            Quat::IDENTITY,
            c("#ffcf7a"),
            0.0,
        );
    }
    panes.rbox(
        Vec3::new(-0.3, up - 0.4, 0.23),
        Vec3::new(0.09, 0.11, 0.02),
        0.0,
        Quat::IDENTITY,
        c("#ffcf7a"),
        0.0,
    );
    // The round clay chimney on the roof's corner.
    let chimney = Vec3::new(0.55, top + 0.6, -0.45);
    mb.lathe(
        Vec3::new(chimney.x, top, chimney.z),
        &[
            (0.0, 0.0),
            (0.16, 0.0),
            (0.15, 0.45),
            (0.11, 0.6),
            (0.0, 0.6),
        ],
        10,
        |_, _| earth * 0.95,
    );
    House {
        walls: mb.build(),
        panes: panes.build(),
        door: Vec3::new(0.15, 0.0, wz + 0.06),
        lamp: Vec3::new(0.55, 1.15, wz + 0.04),
        banner: Vec3::new(-0.3, up + 0.02, -0.2),
        chimney,
        eaves: Vec3::new(0.0, top - 0.05, wz + 0.1),
    }
}
