//! A robot as the realm puts one together (`folk/keeper/rig.rs`): the
//! realm's kit of parts on its joints, in the realm's materials and
//! wearing a look as the realm dresses it, standing at ease and
//! playing a clip as the realm plays one.

use bevy::math::{Quat, Vec3};
use hamlet_engine_sdk::scene::{MeshData, Pbr};
use looks::wardrobe::{Clip, Skin};
use realm::bake::{
    FACE, Frame, Gesture, Glb, HAT_AT, HIP, HOOK, KEY_AT, LAMP_DROP,
    Mat, Mb, NECK, Palette, Part, Pose, SHOULDER, Shape, Stock,
    Swatches, WearSettings, Who, lit, matte, moved, relight, rz,
};

use crate::previews::{Drawn, Form, Node};

/// The seed of the robot every agent's look is shown on.
const SEED: u32 = 3;

/// A joint a pose moves.
#[derive(Clone, Copy)]
enum Slot {
    Rig,
    Body,
    Head,
    Hat,
    EyeL,
    EyeR,
    LegL,
    LegR,
    WheelL,
    WheelR,
    ArmL,
    ArmR,
    Pole,
    Lamp,
    Key,
}

/// A joint of the robot: the node it hangs from, its place at rest or
/// the slot a pose places it by, and the mesh, part and material it
/// draws.
struct Joint {
    parent: usize,
    at: Frame,
    slot: Option<Slot>,
    draws: Option<(usize, Part, Mat)>,
    shadows: bool,
}

/// An ordinary robot wearing a look.
pub struct Robot {
    who: Who,
    /// The look of its own paint.
    paint: Pbr,
    pub meshes: Vec<MeshData>,
    /// Node 0 is the robot's root, node `i + 1` joint `i`.
    joints: Vec<Joint>,
    /// The worn meshes by name, each by its place in `meshes`.
    worn: Vec<(String, usize)>,
    skin: Option<Swatches<Pbr>>,
    clip: Option<Clip>,
    wear: WearSettings,
}

impl Robot {
    /// The robot wearing `skin`, the `.glb` `mesh` and `clip`.
    pub fn new(
        pal: &Palette,
        wear: WearSettings,
        (skin, mesh, clip): (
            Option<&Skin>,
            Option<&Glb>,
            Option<Clip>,
        ),
    ) -> Self {
        let who = Who::citizen(SEED);
        let mut robot = Self {
            who,
            paint: matte(pal.paints[who.paint as usize]),
            meshes: Vec::new(),
            joints: Vec::new(),
            worn: Vec::new(),
            skin: skin.map(|skin| Swatches::of(skin, pal)),
            clip,
            wear,
        };
        for (name, model) in mesh.iter().flat_map(|glb| &glb.meshes) {
            let mut mb = Mb::new();
            model.lay(&mut mb, Default::default());
            robot.worn.push((name.clone(), robot.meshes.len()));
            robot.meshes.push(mb.data());
        }
        robot.build(pal);
        robot
    }

    /// The joints `Rig::make` puts a robot together of, in its order.
    fn build(&mut self, pal: &Palette) {
        let (paint, matte, brass, glow) =
            (Mat::Paint, Mat::Matte, Mat::Brass, Mat::Lit);
        let stand = Pose::stand();
        let rig =
            self.joint(0, Frame::IDENTITY, Some(Slot::Rig), None);
        let body = self.part(
            (rig, Vec3::Y * HIP, Slot::Body),
            Part::Torso,
            (Shape::Torso, paint, brass),
            pal,
        );
        let axle = stand.place(1.0).axle;
        for (x, part, slot, wheel, turns) in [
            (
                -0.11,
                Part::LegL,
                Slot::LegL,
                Part::WheelsL,
                Slot::WheelL,
            ),
            (
                0.11,
                Part::LegR,
                Slot::LegR,
                Part::WheelsR,
                Slot::WheelR,
            ),
        ] {
            let leg = self.part(
                (rig, Vec3::new(x, HIP, 0.0), slot),
                part,
                (Shape::Leg, matte, brass),
                pal,
            );
            self.piece(
                (leg, axle, Some(turns)),
                (Shape::Wheels, wheel, brass),
                true,
                pal,
            );
        }
        for (side, part, slot) in [
            (-1.0, Part::ArmL, Slot::ArmL),
            (1.0, Part::ArmR, Slot::ArmR),
        ] {
            self.part(
                (body, SHOULDER * Vec3::new(side, 1.0, 1.0), slot),
                part,
                (Shape::Arm, matte, brass),
                pal,
            );
        }
        let who = self.who;
        self.piece(
            (body, Vec3::ZERO, None),
            (Shape::Emblem(who.emblem), Part::Emblem, matte),
            false,
            pal,
        );
        self.piece(
            (body, KEY_AT, Some(Slot::Key)),
            (Shape::Key, Part::Key, brass),
            true,
            pal,
        );
        let head = self.piece(
            (body, Vec3::Y * NECK, Some(Slot::Head)),
            (Shape::Head, Part::Head, brass),
            true,
            pal,
        );
        self.piece(
            (head, Vec3::Y * FACE, None),
            (Shape::Glass, Part::Glass, glow),
            false,
            pal,
        );
        for (part, slot) in
            [(Part::EyeL, Slot::EyeL), (Part::EyeR, Slot::EyeR)]
        {
            self.piece(
                (head, Vec3::Y * FACE, Some(slot)),
                (Shape::Eye, part, matte),
                false,
                pal,
            );
        }
        self.part(
            (head, Vec3::Y * HAT_AT, Slot::Hat),
            Part::Hat,
            (Shape::Hat(who.hat), paint, brass),
            pal,
        );
        let pole = self.part(
            (rig, stand.pole_at, Slot::Pole),
            Part::Pole,
            (Shape::Pole, matte, brass),
            pal,
        );
        let lamp = self.piece(
            (pole, HOOK, Some(Slot::Lamp)),
            (Shape::Lamp, Part::Lamp, brass),
            true,
            pal,
        );
        self.piece(
            (lamp, LAMP_DROP, None),
            (Shape::LampGlass, Part::LampGlass, glow),
            false,
            pal,
        );
    }

    /// A joint under the node `parent`; returns its node.
    fn joint(
        &mut self,
        parent: usize,
        at: Frame,
        slot: Option<Slot>,
        draws: Option<(usize, Part, Mat)>,
    ) -> usize {
        self.joints.push(Joint {
            parent,
            at,
            slot,
            draws,
            shadows: true,
        });
        self.joints.len()
    }

    /// The place of `mb` in `meshes`, unless it is empty.
    fn keep(&mut self, mb: Mb) -> Option<usize> {
        (!mb.is_empty()).then(|| {
            self.meshes.push(mb.data());
            self.meshes.len() - 1
        })
    }

    /// A pivot carrying `shape`'s two meshes as `Making::part` makes
    /// one: the first on the pivot, the second on a child of it.
    fn part(
        &mut self,
        (parent, at, slot): (usize, Vec3, Slot),
        part: Part,
        (shape, first, second): (Shape, Mat, Mat),
        pal: &Palette,
    ) -> usize {
        let piece = shape.build(pal);
        let mut drawn = Vec::new();
        for (mb, mat) in [(piece.wood, first), (piece.brass, second)]
        {
            if let Some(mesh) = self.keep(mb) {
                drawn.push((mesh, part, mat));
            }
        }
        let pivot = self.joint(
            parent,
            Frame::at(at),
            Some(slot),
            drawn.first().copied(),
        );
        if let Some(&second) = drawn.get(1) {
            self.joint(pivot, Frame::IDENTITY, None, Some(second));
        }
        pivot
    }

    /// A joint drawing `shape`'s one mesh, as `Making::piece` makes
    /// one.
    fn piece(
        &mut self,
        (parent, at, slot): (usize, Vec3, Option<Slot>),
        (shape, part, mat): (Shape, Part, Mat),
        shadows: bool,
        pal: &Palette,
    ) -> usize {
        let draws = self
            .keep(shape.build(pal).wood)
            .map(|mesh| (mesh, part, mat));
        let joint = self.joint(parent, Frame::at(at), slot, draws);
        self.joints[joint - 1].shadows = shadows;
        joint
    }

    /// The robot `t` seconds into its clip, standing at ease, as
    /// `Rig::pose` and `Rig::dress` place and draw it.
    pub fn nodes(&self, t: f32) -> Vec<Node> {
        let p = Pose::stand();
        let mut placed = p.place(self.who.size);
        let mut arm_l =
            Frame::at(SHOULDER * Vec3::new(-1.0, 1.0, 1.0))
                .with_turn(p.arm_l);
        let mut arm_r = Frame::at(SHOULDER).with_turn(p.arm_r);
        if let Some(clip) = &self.clip {
            let mut gesture = Gesture::default();
            gesture.play(0.0);
            if let Some(moves) = gesture.moves(
                clip,
                t,
                true,
                self.who.seed,
                &self.wear,
            ) {
                moved(
                    [
                        &mut placed.rig,
                        &mut placed.body,
                        &mut placed.head,
                        &mut placed.hat,
                        &mut arm_l,
                        &mut arm_r,
                        &mut placed.leg_l,
                        &mut placed.leg_r,
                    ],
                    moves,
                );
            }
        }
        let wheel = |ahead: f32| {
            Frame::at(placed.axle)
                .with_turn(Quat::from_rotation_x(ahead))
        };
        let posed = |slot: Slot| match slot {
            Slot::Rig => placed.rig,
            Slot::Body => placed.body,
            Slot::Head => placed.head,
            Slot::Hat => placed.hat,
            Slot::EyeL => placed.eye_l,
            Slot::EyeR => placed.eye_r,
            Slot::LegL => placed.leg_l,
            Slot::LegR => placed.leg_r,
            Slot::WheelL => wheel(0.0),
            Slot::WheelR => wheel(0.9),
            Slot::ArmL => arm_l,
            Slot::ArmR => arm_r,
            Slot::Pole => placed.pole,
            Slot::Lamp => Frame::at(HOOK).with_turn(placed.lamp),
            Slot::Key => Frame::at(KEY_AT).with_turn(rz(p.key)),
        };
        let skin = self.skin.as_ref();
        let light = lit(
            relight(p.glow, skin.and_then(|skin| skin.lamp)),
            skin.and_then(|skin| skin.glass),
        );
        let own = |mat: Mat| match mat {
            Mat::Paint => self.paint,
            Mat::Matte => Stock::matte(),
            Mat::Brass => Stock::brass(),
            Mat::Lit => light,
        };
        let mut done = Vec::new();
        let mut nodes = vec![Node {
            parent: None,
            at: Frame::IDENTITY,
            drawn: None,
            shadows: true,
        }];
        for joint in &self.joints {
            let drawn = joint.draws.and_then(|(mesh, part, mat)| {
                match self.worn_of(part) {
                    // A part a worn mesh stands for is drawn in its
                    // first mesh's place and its others are left out.
                    Some(_) if done.contains(&part) => None,
                    Some(worn) => {
                        done.push(part);
                        Some(Drawn {
                            form: Form::Built(worn),
                            look: match mat {
                                Mat::Lit => light,
                                _ => Stock::matte(),
                            },
                        })
                    }
                    None => Some(Drawn {
                        form: Form::Built(mesh),
                        look: skin
                            .and_then(|skin| {
                                skin.material(
                                    part,
                                    mat,
                                    self.who.paint,
                                )
                            })
                            .copied()
                            .unwrap_or_else(|| own(mat)),
                    }),
                }
            });
            nodes.push(Node {
                parent: Some(joint.parent),
                at: joint.slot.map_or(joint.at, posed),
                drawn,
                shadows: joint.shadows,
            });
        }
        nodes
    }

    /// The worn mesh that stands for `part`, found by its own name or
    /// the name both sides share.
    fn worn_of(&self, part: Part) -> Option<usize> {
        let (own, shared) = part.names();
        let find = |name: &str| {
            self.worn
                .iter()
                .find(|(named, _)| named == name)
                .map(|(_, mesh)| *mesh)
        };
        find(own).or_else(|| find(shared?))
    }
}
