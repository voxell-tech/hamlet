//! The entities a wizard is drawn with, and how a pose places them.

use glam::{Quat, Vec3};
use hamlet_engine_sdk::scene::{
    Entity, Light, Material, Mesh, PointLight,
};

use super::{HEAD, Pose};
use crate::frame::Frame;

/// The parts a pose places, in the order of [`Rig::placed`].
const PLACED: usize = 12;

/// The entities of a wizard's rig, under its root.
pub struct Rig {
    /// The parent of every part, which the mascot is moved by.
    pub root: Entity,
    /// The parent of the figure's parts, which takes its squash.
    figure: Entity,
    cloak: Entity,
    head: Entity,
    eyes: Entity,
    hat: Entity,
    hands: [Entity; 2],
    feet: [Entity; 2],
    item: Entity,
    letter: Entity,
    pub wick: Entity,
    pub lamp: Entity,
    /// The frame each placed part was last given.
    placed: [Option<Frame>; PLACED],
    /// Whether the figure is drawn.
    seen: bool,
}

impl Rig {
    /// The parts, with no meshes yet, each answering the pointer as
    /// `key`.
    pub fn make(key: u32) -> Result<Self, String> {
        let root = Entity::make(None, key)?;
        let part = |parent: &Entity| Entity::make(Some(parent), key);
        let figure = part(&root)?;
        let cloak = part(&figure)?;
        let head = part(&figure)?;
        let (eyes, hat) = (part(&head)?, part(&head)?);
        let hands = [part(&figure)?, part(&figure)?];
        let feet = [part(&figure)?, part(&figure)?];
        let item = part(&hands[1])?;
        let letter = part(&figure)?;
        let wick = part(&root)?;
        wick.set_shadows(false, true);
        let lamp = part(&root)?;
        let rig = Self {
            root,
            figure,
            cloak,
            head,
            eyes,
            hat,
            hands,
            feet,
            item,
            letter,
            wick,
            lamp,
            placed: [None; PLACED],
            seen: true,
        };
        for (part, _) in rig.dressed() {
            part.watch(true, true);
        }
        Ok(rig)
    }

    /// Each part that has a mesh of `Parts`, in the order of
    /// `Parts::each`. The two hands share one and the two feet
    /// another.
    pub fn dressed(&self) -> [(&Entity, usize); 10] {
        [
            (&self.cloak, 0),
            (&self.head, 1),
            (&self.eyes, 2),
            (&self.hat, 3),
            (&self.hands[0], 4),
            (&self.hands[1], 4),
            (&self.feet[0], 5),
            (&self.feet[1], 5),
            (&self.item, 6),
            (&self.letter, 7),
        ]
    }

    /// Gives every part its mesh, of `meshes` in the order of
    /// `Parts::each`, and the wick its flame.
    pub fn dress(
        &self,
        meshes: &[&Mesh],
        matte: &Material,
        flame: (&Mesh, &Material),
    ) {
        for (part, which) in self.dressed() {
            part.draw(meshes[which], matte);
        }
        self.wick.draw(flame.0, flame.1);
    }

    /// Makes the lamp the light of the wick, of `colour`, `lumens`
    /// bright and reaching `range`.
    pub fn light(&self, colour: Vec3, lumens: f32, range: f32) {
        let _ = self.lamp.set_light(Some(Light::Point(PointLight {
            color: (colour.x, colour.y, colour.z, 1.0),
            intensity: lumens,
            range,
            radius: 0.05,
            shadows: false,
        })));
    }

    /// The parts a pose places, in the order of `placed`.
    fn parts(&self) -> [&Entity; PLACED] {
        [
            &self.figure,
            &self.head,
            &self.eyes,
            &self.hat,
            &self.hands[0],
            &self.hands[1],
            &self.feet[0],
            &self.feet[1],
            &self.item,
            &self.letter,
            &self.wick,
            &self.lamp,
        ]
    }

    /// Puts every part where `pose` has it, at `size` times the
    /// size it is built at, with the wick over `crest`. A part is
    /// moved only when its frame changed.
    pub fn place(&mut self, pose: &Pose, size: f32, crest: Vec3) {
        // A figure drawn up to nothing is not drawn at all.
        let seen = pose.girth >= 0.02;
        if seen != self.seen {
            self.figure.set_visible(seen);
            self.seen = seen;
        }
        let girth = pose.girth.max(0.02);
        let open = 1.0 - 0.92 * pose.blink.clamp(0.0, 1.0);
        let foot = |side: usize, off: Vec3| {
            let x = if side == 0 { -0.13 } else { 0.13 };
            Frame::at(Vec3::new(x, off.y, off.z))
        };
        let frames = [
            Frame::at(Vec3::Y * pose.lift * size)
                .with_turn(
                    Quat::from_rotation_x(pose.lean)
                        * Quat::from_rotation_z(pose.roll),
                )
                .with_size(
                    Vec3::new(girth, pose.height, girth) * size,
                ),
            Frame::at(Vec3::Y * HEAD).with_turn(
                Quat::from_rotation_y(pose.head.x)
                    * Quat::from_rotation_x(pose.head.y)
                    * Quat::from_rotation_z(pose.head.z),
            ),
            Frame::at(Vec3::Y * 0.025)
                .with_size(Vec3::new(1.0, open, 1.0)),
            Frame::at(Vec3::Y * pose.hat.x)
                .with_turn(Quat::from_rotation_x(-0.1 - pose.hat.y)),
            Frame::at(pose.hands[0]),
            Frame::at(pose.hands[1]),
            foot(0, pose.feet[0]),
            foot(1, pose.feet[1]),
            Frame::IDENTITY
                .with_turn(Quat::from_rotation_x(pose.item)),
            Frame::at(Vec3::new(0.0, 0.76, 0.42))
                .with_turn(Quat::from_rotation_x(-0.6))
                .with_size(Vec3::splat(pose.letter.clamp(0.0, 1.0))),
            Frame::at(
                (crest
                    + pose.wick
                    + Vec3::Y * (pose.lift + pose.hat.x))
                    * size,
            )
            .with_size(Vec3::splat(pose.glow.max(0.0) * size)),
            // The lamp stands for the wick's light, before the chest
            // so that it falls on the face.
            Frame::at(Vec3::new(0.0, size, 0.55 * size)),
        ];
        for (i, frame) in frames.into_iter().enumerate() {
            if self.placed[i] != Some(frame) {
                self.parts()[i].set_transform(frame.scene());
                self.placed[i] = Some(frame);
            }
        }
    }
}
