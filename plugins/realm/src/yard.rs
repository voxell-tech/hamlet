//! The work yard beside a house (docs/working-alone.md): a paved
//! strip with a desk, a rack of scrolls and a small engine, and a
//! robot's stations at them. In the yard's frame X runs along the
//! wall and Z out from it.

use std::f32::consts::{FRAC_PI_2, TAU};

use glam::{Affine3A, Quat, Vec2, Vec3};

use crate::frame::Frame;
use crate::mesh::{Mb, Rng};
use crate::palette::Palette;

/// Length of the beat a robot paces.
pub const BEAT: f32 = 2.2;
/// Seconds between two rings of steam from a running engine.
pub const RING_EVERY: f32 = 1.6;
/// Distance of a robot's seat behind its standing place.
pub const SEAT_BACK: f32 = 0.3;

/// The place about its house a chore is done at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Station {
    Porch,
    /// The beat it walks along the outer edge of the yard.
    Beat,
    Desk,
    /// At the desk, turned to face out of the yard.
    Rack,
    Engine,
    /// The open corner of the yard.
    Open,
    /// Wherever it is.
    Here,
}

/// The work yard beside a house: its paving and where its furniture
/// stands. Places are in the frame of whoever works at them, the
/// robot at the origin facing +X.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YardSettings {
    /// Height of the paving above the house's ground.
    pub terrace: f32,
    /// The paving's extent along the wall and out from it.
    pub along: (f32, f32),
    pub depth: f32,
    /// The near end of the beat a robot paces.
    pub beat_from: f32,
    /// The desk's middle, before the robot that stands at it.
    pub desk: Vec2,
    pub rack: Vec2,
    pub boiler: Vec2,
    /// The hub of the engine's wheel, before the robot that works
    /// it, from the paving up.
    pub wheel: Vec3,
    /// Height of the mouth of the engine's stack above the paving.
    pub stack: f32,
}

/// The yard of one house, placed about the house's middle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Yard {
    /// -1 or 1: the side of the house it is on.
    pub side: f32,
    /// Distance from the house's middle to the wall it lies along.
    out: f32,
    /// Its slide along the wall toward the front.
    slide: f32,
    /// Distance from the house's middle to its front wall.
    front: f32,
    /// The way the house's door is turned from +Z.
    yaw: f32,
    /// The robot works facing the wall, where working along it
    /// would put the furniture between it and the camera.
    across: bool,
    /// The sizes and places it was laid out by.
    pub shape: YardSettings,
}

/// The town's methods on [`Affine3A`].
pub trait AffineExt {
    /// Its place, turn and size, it having no shear.
    fn to_frame(self) -> Frame;
}

impl AffineExt for Affine3A {
    fn to_frame(self) -> Frame {
        let (size, turn, at) = self.to_scale_rotation_translation();
        Frame { at, turn, size }
    }
}

impl Yard {
    /// The yard of a house with walls `half` from its middle and
    /// its door turned `yaw` from +Z. It takes the side that faces
    /// +Z, where the sun and the default camera are.
    pub fn new(
        half: Vec2,
        yaw: f32,
        hall: bool,
        wings: u8,
        shape: YardSettings,
    ) -> Self {
        let (sin, cos) = yaw.sin_cos();
        let side = if sin.abs() > 0.3 {
            -sin.signum()
        } else {
            -cos.signum()
        };
        // Turn of the wall, and of the far end of the yard, toward
        // the camera.
        let (wall, end) = (-side * sin, -side * cos);
        Self {
            side,
            // The second house merged in stands on this side.
            out: half.x + if wings > 1 { 1.35 } else { 0.0 },
            // A hall's tower takes the back of its left wall.
            slide: if hall && side < 0.0 { 1.35 } else { 0.0 },
            front: half.y,
            yaw,
            across: wall < 0.7 && end > 0.0,
            shape,
        }
    }

    /// From the yard's frame to the house's.
    pub fn frame(&self) -> Affine3A {
        Affine3A::from_rotation_translation(
            Quat::from_rotation_y(self.side * FRAC_PI_2),
            Vec3::new(self.side * self.out, 0.0, self.slide),
        )
    }

    /// A robot's place and turn in the yard, as a frame of its own.
    fn group(&self, at: Vec2, turn: f32) -> Affine3A {
        Affine3A::from_rotation_translation(
            Quat::from_rotation_y(turn),
            Vec3::new(at.x, 0.0, at.y),
        )
    }

    /// From the desk's frame to the yard's.
    fn desk(&self) -> Affine3A {
        if self.across {
            self.group(Vec2::new(0.55, 0.9), FRAC_PI_2)
        } else {
            self.group(Vec2::new(-0.95, 0.5), 0.0)
        }
    }

    /// From the engine's frame to the yard's.
    fn engine(&self) -> Affine3A {
        if self.across {
            self.group(Vec2::new(-0.8, 1.12), FRAC_PI_2)
        } else {
            self.group(Vec2::new(0.22, 0.52), 0.0)
        }
    }

    /// Reach of the beat out from the wall.
    fn lane(&self) -> f32 {
        if self.across { 0.9 } else { 1.02 }
    }

    /// The hub of the engine's wheel, in the engine's frame.
    fn hub(&self) -> Vec3 {
        self.shape.wheel + Vec3::Y * self.shape.terrace
    }

    /// The mouth of the engine's stack, in the engine's frame.
    fn stack(&self) -> Vec3 {
        let boiler = self.shape.boiler;
        Vec3::new(
            boiler.x,
            self.shape.terrace + self.shape.stack,
            boiler.y,
        )
    }

    /// The place of the desk's mesh, in the house's frame.
    pub fn desk_at(&self) -> Frame {
        (self.frame() * self.desk()).to_frame()
    }

    /// The place of the engine's mesh, in the house's frame.
    pub fn engine_at(&self) -> Frame {
        (self.frame() * self.engine()).to_frame()
    }

    /// A point of the yard, in the house's frame.
    fn point(&self, p: Vec2) -> Vec3 {
        self.frame().transform_point3(Vec3::new(
            p.x,
            self.shape.terrace,
            p.y,
        ))
    }

    /// The spot of a robot at `station` and the way it faces, in the
    /// house's frame. `beat` is its place on its beat.
    pub fn stand(&self, station: Station, beat: f32) -> (Vec3, f32) {
        // Toward the camera, whichever way the house is turned.
        let sunward = -self.yaw;
        let work =
            self.along() + if self.across { FRAC_PI_2 } else { 0.0 };
        let origin = |group: Affine3A| {
            let p = group.translation;
            self.point(Vec2::new(p.x, p.z))
        };
        match station {
            Station::Beat => (
                self.point(Vec2::new(
                    self.shape.beat_from + BEAT * beat,
                    self.lane(),
                )),
                sunward,
            ),
            Station::Desk => (origin(self.desk()), work),
            Station::Rack => (origin(self.desk()), sunward),
            Station::Engine => (origin(self.engine()), work),
            Station::Open => (
                self.point(if self.across {
                    Vec2::new(-0.1, 0.95)
                } else {
                    Vec2::new(0.5, 0.98)
                }),
                sunward,
            ),
            Station::Porch | Station::Here => (Vec3::ZERO, 0.0),
        }
    }

    /// The way a robot on the beat faces when it walks toward the
    /// far end, in the house's frame.
    pub fn along(&self) -> f32 {
        self.side * FRAC_PI_2 + FRAC_PI_2
    }

    /// True when a point in the house's frame is on the paving.
    pub fn holds(&self, p: Vec3) -> bool {
        let q = self.frame().inverse().transform_point3(p);
        (self.shape.along.0..=self.shape.along.1).contains(&q.x)
            && (0.0..=self.shape.depth).contains(&q.z)
    }

    /// The corner a robot turns round the front of the house, in
    /// the house's frame.
    pub fn gate(&self) -> Vec3 {
        Vec3::new(
            self.side * (self.out + self.lane()),
            0.0,
            // Clear of the paving where it is slid forward.
            (self.front + 0.6)
                .max(self.slide + self.shape.along.1 + 0.35),
        )
    }

    /// The points to walk through from `from` to `to`, both in the
    /// house's frame: down the beat, clear of the furniture, and
    /// round the front corner of the house when either end is off
    /// the paving. Heights are the caller's to set.
    pub fn way(&self, from: Vec3, to: Vec3) -> Vec<Vec3> {
        let inverse = self.frame().inverse();
        let lane = |p: Vec3| {
            let q = inverse.transform_point3(p);
            self.point(Vec2::new(
                q.x.clamp(self.shape.along.0, self.shape.along.1),
                self.lane(),
            ))
        };
        let gate = self.gate();
        // Paving slid past the front wall is gone round, by a point
        // straight out from the front wall.
        let out =
            Vec3::new(self.side * (self.out - 0.5), 0.0, gate.z);
        let round = self.slide > 0.0;
        let mut way = vec![from];
        match (self.holds(from), self.holds(to)) {
            (true, true) => way.extend([lane(from), lane(to)]),
            (true, false) => {
                way.extend([lane(from), gate]);
                way.extend(round.then_some(out));
            }
            (false, true) => {
                way.extend(round.then_some(out));
                way.extend([gate, lane(to)]);
            }
            (false, false) => {}
        }
        way.dedup_by(|b, a| a.distance(*b) < 0.08);
        // The ends stay where they were asked for.
        if way.len() > 1 && way[way.len() - 1].distance(to) < 0.08 {
            way.pop();
        }
        way.push(to);
        way
    }

    /// The place of the wheel when it has turned `turn` radians, in
    /// the engine's frame.
    pub fn wheel(&self, turn: f32) -> Frame {
        Frame::at(self.hub()).with_turn(Quat::from_rotation_z(-turn))
    }

    /// Ring `i` of the two a running engine sends up, `x` seconds
    /// after it started, in the house's frame. None while that ring
    /// is not out.
    pub fn ring(&self, i: usize, x: f32) -> Option<Frame> {
        // A ring lasts while the next one comes out.
        let life = RING_EVERY * 2.0;
        let age = (x - i as f32 * RING_EVERY).rem_euclid(life);
        if age > x {
            return None;
        }
        let a = age / life;
        let rise = a * (2.0 - a);
        let size = (0.6 + 1.3 * a)
            * (a / 0.06).min(1.0)
            * ((1.0 - a) / 0.14).min(1.0);
        // It leans out from the wall as it climbs, clear of the eave.
        let lean = 0.1 + 0.5 * a + 0.12 * a * (x * 0.7).sin();
        let at = self.frame().transform_point3(
            self.engine().transform_point3(self.stack())
                + Vec3::new(0.0, 0.12 + 1.9 * rise, lean),
        );
        Some(Frame::at(at).with_size(Vec3::splat(size)))
    }
}

/// The yard's paving, added to a house's mesh.
pub fn pave(mb: &mut Mb, rng: &mut Rng, yard: &Yard, pal: &Palette) {
    let shape = yard.shape;
    let before = mb.xf;
    let frame = before * yard.frame();
    mb.at(frame);
    let stone = pal.limestone;
    let shade = pal.limestone_shade;

    // Flagstones, each a little off level, sunk well into the ground
    // so a slope never shows under them.
    let (rows, cols) = (2, 5);
    let (w, d) = (
        (shape.along.1 - shape.along.0) / cols as f32,
        shape.depth / rows as f32,
    );
    for r in 0..rows {
        for c in 0..cols {
            let at = Vec2::new(
                shape.along.0 + (c as f32 + 0.5) * w,
                0.02 + (r as f32 + 0.5) * d,
            );
            mb.rbox(
                Vec3::new(
                    at.x,
                    shape.terrace - 0.3 - rng.range(0.0, 0.015),
                    at.y,
                ),
                Vec3::new(w * 0.5 - 0.025, 0.3, d * 0.5 - 0.025),
                0.03,
                rng.wobble(0.8),
                stone.lerp(shade, rng.f() * 0.45),
                0.3,
            );
        }
    }
    mb.at(before);
}

/// The desk, the rack of scrolls and the stool, in the desk's frame.
/// Every yard has the same.
pub fn desk(pal: &Palette, shape: &YardSettings) -> Mb {
    let mut mb = Mb::new();
    let rng = &mut Rng::seeded("yard desk");
    let floor = Vec3::Y * shape.terrace;

    // The desk: a sloped top on two trestles, a sheet and an inkpot.
    let desk = floor + Vec3::new(shape.desk.x, 0.0, shape.desk.y);
    let slope = Quat::from_rotation_z(0.26);
    for z in [-0.24f32, 0.24] {
        mb.rbox(
            desk + Vec3::new(0.02, 0.22, z),
            Vec3::new(0.15, 0.22, 0.03),
            0.015,
            rng.wobble(1.5),
            pal.timber * rng.value(0.08),
            0.3,
        );
    }
    mb.rbox(
        desk + Vec3::Y * 0.48,
        Vec3::new(0.23, 0.022, 0.32),
        0.012,
        slope,
        pal.timber_dark,
        0.0,
    );
    mb.rbox(
        desk + Vec3::new(-0.02, 0.505, 0.02),
        Vec3::new(0.13, 0.004, 0.17),
        0.002,
        slope,
        pal.parchment,
        0.0,
    );
    mb.lathe(
        desk + Vec3::new(0.13, 0.535, -0.22),
        &[(0.04, 0.0), (0.045, 0.05), (0.02, 0.07), (0.0, 0.07)],
        6,
        |_, _| pal.ink,
    );

    // The rack of scrolls at its elbow: its skills.
    let rack = floor + Vec3::new(shape.rack.x, 0.0, shape.rack.y);
    for x in [-0.3f32, 0.3] {
        mb.rbox(
            rack + Vec3::new(x, 0.3, 0.0),
            Vec3::new(0.025, 0.3, 0.085),
            0.012,
            Quat::IDENTITY,
            pal.timber,
            0.3,
        );
    }
    for (row, y) in [0.16f32, 0.38, 0.6].into_iter().enumerate() {
        mb.rbox(
            rack + Vec3::Y * y,
            Vec3::new(0.3, 0.015, 0.09),
            0.008,
            Quat::IDENTITY,
            pal.timber_dark,
            0.0,
        );
        for k in 0..3 - row {
            let x = -0.19 + k as f32 * 0.17 + rng.sym(0.02);
            mb.tube(
                rack + Vec3::new(x, y + 0.05, -0.08),
                rack + Vec3::new(x, y + 0.05, 0.1),
                0.035,
                0.035,
                7,
                pal.parchment * rng.value(0.07),
                true,
            );
        }
    }

    // The stool it sits on to read.
    let log = pal.timber;
    mb.lathe(
        floor - Vec3::X * SEAT_BACK,
        &[(0.15, 0.0), (0.13, 0.05), (0.14, 0.17), (0.0, 0.18)],
        9,
        |r, s| {
            if r == 2 {
                pal.seat
            } else {
                log * (1.0 + 0.1 * (s % 3) as f32)
            }
        },
    );
    mb
}

/// The engine without its wheel, in the engine's frame. Every yard
/// has the same.
pub fn engine(pal: &Palette, shape: &YardSettings) -> Mb {
    let mut mb = Mb::new();
    let (stone, shade) = (pal.limestone, pal.limestone_shade);
    let floor = Vec3::Y * shape.terrace;
    let hub = shape.wheel + floor;
    // The engine: a brass boiler on a stone foot, with a tall stack.
    let boiler =
        floor + Vec3::new(shape.boiler.x, 0.0, shape.boiler.y);
    mb.lathe(
        boiler,
        &[(0.3, 0.0), (0.3, 0.1), (0.25, 0.12)],
        10,
        |_, s| stone.lerp(shade, (s % 3) as f32 * 0.2),
    );
    let brass = pal.brass;
    mb.lathe(
        boiler + Vec3::Y * 0.12,
        &[
            (0.24, 0.0),
            (0.27, 0.1),
            (0.27, 0.5),
            (0.21, 0.64),
            (0.1, 0.7),
            (0.085, 1.16),
            (0.13, 1.19),
            (0.13, 1.24),
            (0.07, 1.24),
        ],
        12,
        |r, _| if r >= 4 { pal.bed } else { brass },
    );
    for y in [0.24f32, 0.6] {
        mb.torus(
            boiler + Vec3::Y * y,
            Vec3::Y,
            0.272,
            0.022,
            16,
            pal.bed,
        );
    }
    // The firebox door, and the axle the wheel turns on.
    mb.rbox(
        boiler + Vec3::new(-0.26, 0.32, 0.0),
        Vec3::new(0.02, 0.09, 0.09),
        0.01,
        Quat::IDENTITY,
        pal.bed,
        0.0,
    );
    mb.tube(
        Vec3::new(hub.x, hub.y, shape.boiler.y),
        hub + Vec3::Z * 0.03,
        0.03,
        0.03,
        6,
        pal.bed,
        true,
    );
    mb
}

/// The engine's wheel about its hub, with one dark knob on the rim
/// so its turning shows.
pub fn wheel(pal: &Palette) -> Mb {
    let mut mb = Mb::new();
    let r = 0.27;
    mb.torus(Vec3::ZERO, Vec3::Z, r, 0.03, 18, pal.brass);
    for i in 0..4 {
        let a = i as f32 / 4.0 * TAU;
        mb.tube(
            Vec3::ZERO,
            Vec3::new(a.cos(), a.sin(), 0.0) * r,
            0.016,
            0.016,
            5,
            pal.timber_dark,
            false,
        );
    }
    mb.blob(Vec3::ZERO, Vec3::splat(0.05), 4, 0.0, 0.0, |_| pal.bed);
    mb.blob(
        Vec3::new(r, 0.0, 0.03),
        Vec3::splat(0.05),
        4,
        0.0,
        0.0,
        |_| pal.ink,
    );
    mb
}

/// One ring of steam, lying flat.
pub fn ring() -> Mb {
    let mut mb = Mb::new();
    mb.torus(Vec3::ZERO, Vec3::Y, 0.22, 0.04, 16, Vec3::ONE);
    mb
}
