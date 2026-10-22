//! The hamlets' meshes: houses in other styles, built in the frame a
//! hamlet's mesh is authored in (see docs/looks.md): ground at y 0,
//! the front and its door facing +Z, walls about 1 from the middle
//! each way, one storey with its walls about 1.9 high.

use bevy::math::{Affine3A, Quat, Vec2, Vec3};
use hamlet_engine_sdk::scene::MeshData;
use looks::wardrobe::rgb;
use realm::bake::{Mb, to_linear};

use crate::glb::Glb;

/// A house as a hamlet's mesh holds it: its walls and windows, and
/// the spots the world hangs its door lamp, banner, smoke and
/// lanterns from.
pub struct House {
    pub walls: MeshData,
    pub panes: MeshData,
    pub door: Vec3,
    pub lamp: Vec3,
    pub banner: Vec3,
    pub chimney: Vec3,
    pub eaves: Vec3,
}

impl House {
    /// The `.glb` a hamlet wears.
    pub fn glb(&self) -> Vec<u8> {
        Glb::new()
            .mesh("walls", &self.walls)
            .mesh("panes", &self.panes)
            .spot("door", self.door)
            .spot("lamp", self.lamp)
            .spot("banner", self.banner)
            .spot("chimney", self.chimney)
            .spot("eaves", self.eaves)
            .write()
    }
}

/// An sRGB `#rrggbb` as the linear colour meshes are built in.
pub fn c(hex: &str) -> Vec3 {
    to_linear(Vec3::from(rgb(hex).unwrap_or([1.0; 3])))
}

/// The house of the asset `id`, if it is one.
pub fn of(id: &str) -> Option<House> {
    match id {
        "hamlet-mesh-minka" => Some(minka()),
        "hamlet-mesh-stave" => Some(stave()),
        "hamlet-mesh-adobe" => Some(adobe()),
        "hamlet-mesh-mushroom" => Some(mushroom()),
        "hamlet-mesh-yurt" => Some(yurt()),
        "hamlet-mesh-lighthouse" => Some(lighthouse()),
        _ => None,
    }
}

/// A round fleck lying on a surface at `at`, whose outward normal is
/// `up` and whose radius of curvature is `bend`, sunk so that its rim
/// stays under the surface.
pub fn fleck(
    mb: &mut Mb,
    at: Vec3,
    up: Vec3,
    (radius, thick, bend): (f32, f32, f32),
    colour: Vec3,
) {
    let up = up.normalize();
    // The surface falls away under the rim, and a turned surface is
    // facetted inside its true curve.
    let sink =
        radius * radius / (2.0 * bend) + 0.01 * bend + 0.25 * thick;
    let was = mb.xf;
    mb.xf = was
        * Affine3A::from_rotation_translation(
            Quat::from_rotation_arc(Vec3::Y, up),
            at - up * sink,
        );
    mb.blob(
        Vec3::ZERO,
        Vec3::new(radius, thick, radius),
        4,
        0.0,
        0.0,
        |_| colour,
    );
    mb.xf = was;
}

/// A toadstool's cap over `base`: half an ellipsoid `wide` about and
/// `tall`, its rim curled under, its gills beneath rising to `stalk`
/// at `lift`, and `flecks` white flecks on it.
pub fn toadstool(
    mb: &mut Mb,
    base: Vec3,
    (wide, tall): (f32, f32),
    (stalk, lift): (f32, f32),
    flecks: usize,
    (red, gills, white): (Vec3, Vec3, Vec3),
) {
    let mut profile = vec![(stalk, lift), (wide * 0.9, -tall * 0.1)];
    profile.extend((-1..=6).map(|k| {
        let a = k as f32 * std::f32::consts::PI / 12.0;
        (if k == 6 { 0.0 } else { wide * a.cos() }, tall * a.sin())
    }));
    mb.lathe(base, &profile, 24, |ring, seg| match ring {
        0 if seg % 2 == 0 => gills,
        0 => gills * 0.85,
        1 => red * 0.75,
        _ => red,
    });
    for k in 0..flecks {
        let turn = k as f32 * 2.4;
        let a = 0.25 + 1.05 * (k as f32 + 0.5) / flecks as f32;
        let (cos, sin) = (a.cos(), a.sin());
        let at = base
            + Vec3::new(
                wide * cos * turn.cos(),
                tall * sin,
                wide * cos * turn.sin(),
            );
        let up = Vec3::new(
            cos * turn.cos() / wide,
            sin / tall,
            cos * turn.sin() / wide,
        );
        let radius = wide * (0.1 + 0.015 * (k * 7 % 4) as f32);
        fleck(
            mb,
            at,
            up,
            (radius, radius * 0.35, wide.min(tall)),
            white,
        );
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
        walls: mb.data(),
        panes: panes.data(),
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
        walls: mb.data(),
        panes: panes.data(),
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
        walls: mb.data(),
        panes: panes.data(),
        door: Vec3::new(0.15, 0.0, wz + 0.06),
        lamp: Vec3::new(0.55, 1.15, wz + 0.04),
        banner: Vec3::new(-0.3, up + 0.02, -0.2),
        chimney,
        eaves: Vec3::new(0.0, top - 0.05, wz + 0.1),
    }
}

/// A mushroom house: a fat round stalk with an arched plank door and
/// round windows, under a big red cap flecked with white, a stove
/// pipe up through the cap and moss about its foot.
pub fn mushroom() -> House {
    let (mut mb, mut panes) = (Mb::new(), Mb::new());
    let stalk = c("#eee2c4");
    let red = c("#c4352b");
    let gills = c("#dcc49c");
    let white = c("#f7f2e6");
    let wood = c("#7a4b2c");
    let moss = c("#6f8a4a");
    let iron = c("#3b3a3c");
    let stone = c("#8c8a80");
    mb.lathe(
        Vec3::ZERO,
        &[
            (0.0, -0.1),
            (0.9, -0.1),
            (0.94, 0.25),
            (0.93, 0.85),
            (0.87, 1.3),
            (0.78, 1.75),
            (0.0, 1.75),
        ],
        24,
        |_, seg| if seg % 2 == 0 { stalk } else { stalk * 0.94 },
    );
    // Moss about the foot, clear of the door.
    for k in 0..9 {
        let a = 0.6 + k as f32 * (std::f32::consts::TAU - 1.2) / 8.0;
        mb.blob(
            Vec3::new(0.92 * a.sin(), 0.02, 0.92 * a.cos()),
            Vec3::new(0.17, 0.1, 0.13),
            4,
            0.3,
            k as f32,
            |_| moss,
        );
    }
    // The cap; its gills meet the stalk under it.
    let (cap, wide) = (Vec3::Y * 1.55, 1.2);
    toadstool(
        &mut mb,
        cap,
        (wide, 1.2),
        (0.78, 0.12),
        11,
        (red, gills, white),
    );
    let cap_y =
        |d: f32| cap.y + 1.2 * (1.0 - (d / wide).powi(2)).sqrt();
    // An arched door of planks in a darker frame, and a stone step.
    let dz = 0.93;
    for (half, z, depth, colour) in
        [(0.3, dz, 0.07, wood * 0.65), (0.23, dz + 0.03, 0.07, wood)]
    {
        mb.rbox(
            Vec3::new(0.0, 0.35, z),
            Vec3::new(half, 0.42, depth),
            0.02,
            Quat::IDENTITY,
            colour,
            0.0,
        );
        mb.tube(
            Vec3::new(0.0, 0.75, z - depth),
            Vec3::new(0.0, 0.75, z + depth),
            half,
            half,
            16,
            colour,
            true,
        );
    }
    for x in [-0.08, 0.08] {
        mb.rbox(
            Vec3::new(x, 0.5, dz + 0.1),
            Vec3::new(0.008, 0.45, 0.005),
            0.0,
            Quat::IDENTITY,
            wood * 0.6,
            0.0,
        );
    }
    ball(&mut mb, Vec3::new(0.15, 0.45, dz + 0.11), 0.025, iron);
    mb.rbox(
        Vec3::new(0.0, 0.03, dz + 0.17),
        Vec3::new(0.32, 0.05, 0.13),
        0.03,
        Quat::IDENTITY,
        stone,
        0.2,
    );
    // Round windows, front and back, each with a cross of wood.
    for (turn, y) in
        [(1.1f32, 0.85), (-1.1, 0.85), (2.4, 0.8), (-2.4, 0.8)]
    {
        let out = Vec3::new(turn.sin(), 0.0, turn.cos());
        let at = out * 0.925 + Vec3::Y * y;
        let rot = Quat::from_rotation_y(turn);
        mb.torus(at + out * 0.01, out, 0.17, 0.045, 16, wood);
        panes.tube(
            at - out * 0.02,
            at + out * 0.015,
            0.16,
            0.16,
            14,
            c("#ffd98a"),
            true,
        );
        for half in
            [Vec3::new(0.15, 0.01, 0.01), Vec3::new(0.01, 0.15, 0.01)]
        {
            mb.rbox(at + out * 0.03, half, 0.0, rot, wood, 0.0);
        }
    }
    // A stove pipe up through the cap.
    let (px, pz) = (0.42, -0.38);
    let through = cap_y(Vec2::new(px, pz).length());
    let chimney = Vec3::new(px, through + 0.4, pz);
    mb.tube(
        Vec3::new(px, through - 0.3, pz),
        chimney,
        0.065,
        0.065,
        10,
        iron,
        true,
    );
    mb.torus(
        Vec3::new(px, through, pz),
        Vec3::Y,
        0.075,
        0.02,
        10,
        iron,
    );
    mb.torus(chimney, Vec3::Y, 0.07, 0.02, 10, iron);
    House {
        walls: mb.data(),
        panes: panes.data(),
        door: Vec3::new(0.0, 0.08, dz + 0.17),
        lamp: Vec3::new(0.48, 1.0, 0.82),
        banner: Vec3::new(-0.45, cap_y(0.45) - 0.02, 0.0),
        chimney: chimney + Vec3::Y * 0.02,
        eaves: Vec3::new(0.0, cap.y - 0.32, 1.12),
    }
}

/// A steppe yurt: a round felt wall tied with ropes under a band of
/// lattice, a low conical roof whose ropes run to a wooden crown, a
/// painted wooden door, and a stove pipe up through the crown.
pub fn yurt() -> House {
    let (mut mb, mut panes) = (Mb::new(), Mb::new());
    let felt = c("#ebe3d0");
    let roof = c("#d8ccb2");
    let rope = c("#5a4632");
    let red = c("#b23a2c");
    let blue = c("#2f5f8f");
    let orange = c("#d9772b");
    let gold = c("#e0b34a");
    let wood = c("#6e4a2c");
    let iron = c("#34332f");
    let (r, top) = (0.95, 1.15);
    mb.tube(Vec3::Y * -0.08, Vec3::Y * top, r, r, 28, felt, false);
    for y in [0.32, 0.62] {
        mb.torus(Vec3::Y * y, Vec3::Y, r + 0.005, 0.014, 32, rope);
    }
    // A red band under the eave, crossed by a gold lattice.
    let (low, high) = (0.9, 1.1);
    mb.lathe(
        Vec3::ZERO,
        &[(r + 0.012, low), (r + 0.012, high)],
        28,
        |_, _| red,
    );
    for y in [low, high] {
        mb.torus(Vec3::Y * y, Vec3::Y, r + 0.014, 0.014, 32, gold);
    }
    for k in 0..28 {
        let turn = k as f32 / 28.0 * std::f32::consts::TAU;
        let out = Vec3::new(turn.sin(), 0.0, turn.cos());
        for lean in [0.75f32, -0.75] {
            mb.rbox(
                out * (r + 0.02) + Vec3::Y * (low + high) / 2.0,
                Vec3::new(0.12, 0.008, 0.005),
                0.0,
                Quat::from_rotation_y(turn)
                    * Quat::from_rotation_z(lean),
                gold,
                0.0,
            );
        }
    }
    // The roof, its rim a little past the wall, open at the crown.
    let (eave, rim, crown, peak) = (1.12, 1.1, 0.27, 1.78);
    mb.lathe(
        Vec3::ZERO,
        &[
            (0.9, top - 0.03),
            (1.08, eave - 0.06),
            (rim, eave),
            (crown, peak),
        ],
        28,
        |ring, _| if ring == 2 { roof } else { roof * 0.85 },
    );
    mb.torus(Vec3::Y * eave, Vec3::Y, rim, 0.025, 32, red);
    let slope = Vec2::new(peak - eave, rim - crown).normalize();
    for k in 0..10 {
        let turn = (k as f32 + 0.5) / 10.0 * std::f32::consts::TAU;
        let out = Vec3::new(turn.sin(), 0.0, turn.cos());
        let lift = out * slope.x * 0.012 + Vec3::Y * slope.y * 0.012;
        mb.tube(
            out * (crown + 0.03) + Vec3::Y * (peak - 0.02) + lift,
            out * (rim - 0.02) + Vec3::Y * (eave + 0.015) + lift,
            0.013,
            0.013,
            5,
            rope,
            true,
        );
    }
    // The crown: a wooden ring, lit between its spokes, under a cap.
    mb.torus(Vec3::Y * peak, Vec3::Y, crown, 0.04, 20, orange);
    panes.tube(
        Vec3::Y * (peak - 0.02),
        Vec3::Y * 1.92,
        0.24,
        0.24,
        16,
        c("#ffcf7a"),
        false,
    );
    for k in 0..8 {
        let turn = k as f32 / 8.0 * std::f32::consts::TAU;
        mb.rbox(
            Vec3::new(turn.sin() * 0.245, 1.86, turn.cos() * 0.245),
            Vec3::new(0.015, 0.07, 0.01),
            0.0,
            Quat::from_rotation_y(turn),
            wood,
            0.0,
        );
    }
    mb.lathe(
        Vec3::ZERO,
        &[
            (0.0, 1.9),
            (0.31, 1.9),
            (0.31, 1.94),
            (0.18, 2.03),
            (0.0, 2.06),
        ],
        16,
        |ring, _| if ring == 1 { orange } else { roof * 0.9 },
    );
    // A stove pipe up through the crown.
    let chimney = Vec3::new(0.1, 2.34, -0.05);
    mb.tube(
        Vec3::new(chimney.x, 1.6, chimney.z),
        chimney,
        0.045,
        0.045,
        8,
        iron,
        true,
    );
    mb.torus(chimney, Vec3::Y, 0.05, 0.015, 8, iron);
    // The painted door: an orange frame, two blue leaves with a gold
    // knot on each, and a wooden step.
    let dz = 0.94;
    mb.rbox(
        Vec3::new(0.0, 0.4, dz),
        Vec3::new(0.28, 0.48, 0.07),
        0.02,
        Quat::IDENTITY,
        orange,
        0.0,
    );
    for x in [-0.1f32, 0.1] {
        mb.rbox(
            Vec3::new(x, 0.4, dz + 0.05),
            Vec3::new(0.085, 0.36, 0.03),
            0.015,
            Quat::IDENTITY,
            blue,
            0.0,
        );
        mb.rbox(
            Vec3::new(x, 0.48, dz + 0.085),
            Vec3::new(0.035, 0.035, 0.006),
            0.0,
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
            gold,
            0.0,
        );
        mb.rbox(
            Vec3::new(x, 0.2, dz + 0.085),
            Vec3::new(0.05, 0.012, 0.006),
            0.0,
            Quat::IDENTITY,
            gold,
            0.0,
        );
    }
    mb.rbox(
        Vec3::new(0.0, 0.03, dz + 0.14),
        Vec3::new(0.32, 0.05, 0.1),
        0.02,
        Quat::IDENTITY,
        wood,
        0.2,
    );
    let roof_y =
        |d: f32| eave + (rim - d) / (rim - crown) * (peak - eave);
    House {
        walls: mb.data(),
        panes: panes.data(),
        door: Vec3::new(0.0, 0.08, dz + 0.14),
        lamp: Vec3::new(0.5, 0.78, 0.9),
        banner: Vec3::new(-0.55, roof_y(0.6) - 0.02, -0.25),
        chimney: chimney + Vec3::Y * 0.02,
        eaves: Vec3::new(0.0, eave - 0.08, rim - 0.04),
    }
}

/// A lighthouse cottage: a whitewashed cottage with a slate roof and
/// a blue door, beside a red and white striped tower whose lantern
/// room is lit.
pub fn lighthouse() -> House {
    let (mut mb, mut panes) = (Mb::new(), Mb::new());
    let white = c("#f1eee6");
    let red = c("#c23b2e");
    let slate = c("#4f5a66");
    let stone = c("#8d8a82");
    let blue = c("#2d5b86");
    let iron = c("#2f3236");
    let glow = c("#ffe39a");
    // The cottage, built about its own middle.
    let mid = Vec3::new(-0.3, 0.0, 0.0);
    mb.xf = Affine3A::from_translation(mid);
    let (wx, wz, top) = (0.88, 0.7, 1.28);
    mb.rbox(
        Vec3::new(0.0, 0.06, 0.0),
        Vec3::new(wx + 0.04, 0.14, wz + 0.04),
        0.03,
        Quat::IDENTITY,
        stone,
        0.3,
    );
    mb.rbox(
        Vec3::new(0.0, (top + 0.1) / 2.0, 0.0),
        Vec3::new(wx, (top - 0.1) / 2.0, wz),
        0.03,
        Quat::IDENTITY,
        white,
        0.1,
    );
    // The gables, under the roof.
    let apex = 1.85;
    for side in [-1.0f32, 1.0] {
        let n = Vec3::X * side;
        let corners = [
            Vec3::new(wx * side, top - 0.01, -wz),
            Vec3::new(wx * side, top - 0.01, wz),
            Vec3::new(wx * side, apex, 0.0),
        ];
        let ids = corners.map(|p| mb.v(p, n, white));
        mb.tri(ids[0], ids[1], ids[2]);
    }
    let (eave, over, ridge) = (1.12, 0.92, 1.9);
    pitch(&mut mb, (eave, over), (ridge, 0.0), (0.98, 0.07), slate);
    pitch(&mut mb, (eave, -over), (ridge, 0.0), (0.98, 0.07), slate);
    mb.rbox(
        Vec3::new(0.0, ridge + 0.05, 0.0),
        Vec3::new(0.99, 0.05, 0.08),
        0.02,
        Quat::IDENTITY,
        slate * 0.7,
        0.0,
    );
    // The door, blue in a stone frame, and a step.
    let door = -0.2;
    mb.rbox(
        Vec3::new(door, 0.5, wz + 0.02),
        Vec3::new(0.24, 0.52, 0.04),
        0.02,
        Quat::IDENTITY,
        stone,
        0.0,
    );
    mb.rbox(
        Vec3::new(door, 0.47, wz + 0.045),
        Vec3::new(0.18, 0.47, 0.03),
        0.015,
        Quat::IDENTITY,
        blue,
        0.0,
    );
    ball(
        &mut mb,
        Vec3::new(door + 0.12, 0.46, wz + 0.08),
        0.02,
        c("#d9a441"),
    );
    mb.rbox(
        Vec3::new(door, 0.03, wz + 0.15),
        Vec3::new(0.28, 0.05, 0.12),
        0.02,
        Quat::IDENTITY,
        stone,
        0.2,
    );
    // Windows either side, blue framed, with a cross and a sill.
    for x in [-0.68, 0.55] {
        mb.rbox(
            Vec3::new(x, 0.85, wz + 0.01),
            Vec3::new(0.17, 0.19, 0.03),
            0.01,
            Quat::IDENTITY,
            blue,
            0.0,
        );
        panes.rbox(
            Vec3::new(x, 0.85, wz + 0.035),
            Vec3::new(0.12, 0.14, 0.02),
            0.0,
            Quat::IDENTITY,
            glow,
            0.0,
        );
        for half in [
            Vec3::new(0.12, 0.012, 0.006),
            Vec3::new(0.012, 0.14, 0.006),
        ] {
            mb.rbox(
                Vec3::new(x, 0.85, wz + 0.06),
                half,
                0.0,
                Quat::IDENTITY,
                blue,
                0.0,
            );
        }
        mb.rbox(
            Vec3::new(x, 0.64, wz + 0.05),
            Vec3::new(0.2, 0.025, 0.06),
            0.01,
            Quat::IDENTITY,
            white * 0.9,
            0.0,
        );
    }
    // The chimney at the far gable.
    let stack = Vec3::new(-0.68, 2.3, -0.3);
    mb.rbox(
        Vec3::new(stack.x, (stack.y + 1.3) / 2.0, stack.z),
        Vec3::new(0.13, (stack.y - 1.3) / 2.0, 0.13),
        0.02,
        Quat::IDENTITY,
        stone,
        0.2,
    );
    mb.rbox(
        stack,
        Vec3::new(0.16, 0.04, 0.16),
        0.02,
        Quat::IDENTITY,
        stone * 0.8,
        0.0,
    );
    mb.xf = Affine3A::IDENTITY;
    // The tower, striped and narrowing, behind the cottage's end.
    let tower = Vec3::new(0.8, 0.0, -0.2);
    let (foot, deck) = (-0.08, 2.55);
    let bands = 6;
    let radius = |y: f32| 0.46 - 0.12 * (y - foot) / (deck - foot);
    let stripes = (0..=bands)
        .map(|k| {
            let y = foot + k as f32 * (deck - foot) / bands as f32;
            (radius(y), y)
        })
        .collect::<Vec<_>>();
    mb.lathe(tower, &stripes, 20, |ring, _| {
        if ring % 2 == 0 { white } else { red }
    });
    for y in [1.3, 2.05] {
        let face = tower + Vec3::new(0.0, y, radius(y));
        mb.rbox(
            face,
            Vec3::new(0.07, 0.1, 0.03),
            0.01,
            Quat::IDENTITY,
            iron,
            0.0,
        );
        panes.rbox(
            face + Vec3::Z * 0.02,
            Vec3::new(0.045, 0.075, 0.02),
            0.0,
            Quat::IDENTITY,
            glow,
            0.0,
        );
    }
    // The gallery about the lantern room, and its rail.
    mb.lathe(
        tower,
        &[(radius(deck) - 0.02, deck - 0.2), (0.5, deck)],
        20,
        |_, _| white * 0.9,
    );
    mb.tube(
        tower + Vec3::Y * deck,
        tower + Vec3::Y * (deck + 0.06),
        0.5,
        0.5,
        24,
        iron,
        true,
    );
    let rail = deck + 0.24;
    mb.torus(tower + Vec3::Y * rail, Vec3::Y, 0.48, 0.012, 24, iron);
    for k in 0..12 {
        let turn = k as f32 / 12.0 * std::f32::consts::TAU;
        let at =
            tower + Vec3::new(turn.sin(), 0.0, turn.cos()) * 0.48;
        mb.tube(
            at + Vec3::Y * deck,
            at + Vec3::Y * rail,
            0.01,
            0.01,
            4,
            iron,
            false,
        );
    }
    // The lantern room: lit glass between iron bars, under a red cap.
    let (sill, head) = (deck + 0.06, deck + 0.44);
    mb.tube(
        tower + Vec3::Y * sill,
        tower + Vec3::Y * (sill + 0.06),
        0.29,
        0.29,
        16,
        red,
        true,
    );
    panes.tube(
        tower + Vec3::Y * sill,
        tower + Vec3::Y * head,
        0.26,
        0.26,
        16,
        glow,
        false,
    );
    for k in 0..6 {
        let turn = k as f32 / 6.0 * std::f32::consts::TAU;
        let at =
            tower + Vec3::new(turn.sin(), 0.0, turn.cos()) * 0.265;
        mb.tube(
            at + Vec3::Y * sill,
            at + Vec3::Y * head,
            0.015,
            0.015,
            4,
            iron,
            false,
        );
    }
    mb.lathe(
        tower + Vec3::Y * head,
        &[
            (0.0, 0.0),
            (0.33, 0.0),
            (0.3, 0.05),
            (0.15, 0.2),
            (0.04, 0.3),
            (0.0, 0.31),
        ],
        16,
        |_, _| red,
    );
    ball(&mut mb, tower + Vec3::Y * (head + 0.28), 0.045, iron);
    House {
        walls: mb.data(),
        panes: panes.data(),
        door: mid + Vec3::new(door, 0.08, wz + 0.15),
        lamp: mid + Vec3::new(door + 0.38, 1.0, wz + 0.1),
        banner: mid + Vec3::new(-0.2, ridge + 0.05, 0.0),
        chimney: mid + stack + Vec3::Y * 0.06,
        eaves: mid + Vec3::new(0.0, eave - 0.05, over - 0.04),
    }
}

/// A ball of `radius` standing on `foot`.
fn ball(mb: &mut Mb, foot: Vec3, radius: f32, colour: Vec3) {
    mb.blob(
        foot + Vec3::Y * radius,
        Vec3::splat(radius),
        5,
        0.0,
        0.0,
        |_| colour,
    );
}
