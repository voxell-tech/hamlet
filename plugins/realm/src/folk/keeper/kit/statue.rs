//! A robot standing still, as one mesh for each of its looks, for a
//! picture of a town that does not move.

use glam::{Affine3A, Quat, Vec3};
use hamlet_engine_sdk::scene::Pbr;

use crate::build::frame::Frame;
use crate::build::mesh::Mb;
use crate::folk::keeper::kit::body::{KEY_AT, LAMP_DROP};
use crate::folk::keeper::kit::hats::SPIRE;
use crate::folk::keeper::kit::{Piece, Shape};
use crate::folk::keeper::lot::footing;
use crate::folk::keeper::pose::{
    FACE, HOOK, Pose, SHOULDER, rx, ry, rz,
};
use crate::folk::keeper::rig::lights;
use crate::folk::keeper::who::Who;
use crate::society::AgentId;
use crate::world::land::Land;
use crate::world::layout::Plot;
use crate::world::palette::Palette;

/// A robot's meshes by look: paint, wood and the rest in the matte
/// look with the paint in their colours, brass, and the glass of its
/// lights.
#[derive(Default)]
pub struct Statue {
    pub matte: Mb,
    pub brass: Mb,
    pub lights: Mb,
}

impl Statue {
    /// The robot of `agent`, which leads its town when `leader`,
    /// standing on the porch of `spot` at `scale` times its size.
    pub fn on_porch(
        land: &Land,
        agent: AgentId,
        leader: bool,
        spot: &Plot,
        scale: f32,
    ) -> Self {
        let porch = spot.porch();
        let at = Affine3A::from_scale_rotation_translation(
            Vec3::splat(scale),
            Quat::from_rotation_y(spot.yaw),
            Vec3::new(porch.x, footing(land, porch), porch.y),
        );
        Self::of(&land.pal, Who::of(agent, leader), at)
    }

    /// The robot `who` standing at ease at `at`, posed as the rig
    /// poses it.
    pub fn of(pal: &Palette, who: Who, at: Affine3A) -> Self {
        let mut statue = Self::default();
        let p = Pose::stand();
        let placed = p.place(who.size);
        let paint = pal.paints[who.paint as usize];
        let rig = at * placed.rig.affine();
        let body = rig * placed.body.affine();
        let head = body * placed.head.affine();
        let hat = head * placed.hat.affine();
        let pole = rig * placed.pole.affine();
        let lamp =
            pole * Frame::at(HOOK).with_turn(placed.lamp).affine();
        statue.piece(pal, Shape::Torso, body, paint);
        statue.piece(pal, Shape::Hat(who.hat), hat, paint);
        for (leg, ahead) in [(placed.leg_l, 0.0), (placed.leg_r, 0.9)]
        {
            let leg = rig * leg.affine();
            statue.piece(pal, Shape::Leg, leg, Vec3::ONE);
            let wheel = Frame::at(placed.axle).with_turn(rx(ahead));
            statue.brass(pal, Shape::Wheels, leg * wheel.affine());
        }
        let arms = [
            Frame::at(SHOULDER * Vec3::new(-1.0, 1.0, 1.0))
                .with_turn(p.arm_l),
            Frame::at(SHOULDER).with_turn(p.arm_r),
        ];
        for arm in arms {
            statue.piece(
                pal,
                Shape::Arm,
                body * arm.affine(),
                Vec3::ONE,
            );
        }
        statue.piece(pal, Shape::Emblem(who.emblem), body, Vec3::ONE);
        let key = Frame::at(KEY_AT).with_turn(rz(p.key));
        statue.brass(pal, Shape::Key, body * key.affine());
        statue.brass(pal, Shape::Head, head);
        let face = head * Frame::at(Vec3::Y * FACE).affine();
        statue.lights.put(
            &Shape::Glass.build(pal).wood,
            face,
            Vec3::ONE,
        );
        for eye in [placed.eye_l, placed.eye_r] {
            statue.piece(
                pal,
                Shape::Eye,
                head * eye.affine(),
                Vec3::ONE,
            );
        }
        if who.leader {
            statue.piece(pal, Shape::Mantle, body, Vec3::ONE);
            let flag = Frame::at(Vec3::Y * (SPIRE - 0.02))
                .with_turn(ry(p.flag) * rx(-0.12));
            statue.piece(
                pal,
                Shape::Pennant,
                hat * flag.affine(),
                Vec3::ONE,
            );
        }
        statue.piece(pal, Shape::Pole, pole, Vec3::ONE);
        statue.brass(pal, Shape::Lamp, lamp);
        let glass = lamp * Frame::at(LAMP_DROP).affine();
        statue.lights.put(
            &Shape::LampGlass.build(pal).wood,
            glass,
            Vec3::ONE,
        );
        statue
    }

    /// The look of a robot's lights as it stands at ease.
    pub fn lights_look() -> Pbr {
        lights(Pose::stand().glow, None)
    }

    /// Adds `shape` at `at`: its paint or wood times `tint`, and its
    /// brass.
    fn piece(
        &mut self,
        pal: &Palette,
        shape: Shape,
        at: Affine3A,
        tint: Vec3,
    ) {
        let Piece { wood, brass } = shape.build(pal);
        self.matte.put(&wood, at, tint);
        self.brass.put(&brass, at, Vec3::ONE);
    }

    /// Adds `shape`, all of it brass, at `at`.
    fn brass(&mut self, pal: &Palette, shape: Shape, at: Affine3A) {
        self.brass.put(&shape.build(pal).wood, at, Vec3::ONE);
    }
}
