//! The robot's parts as meshes and materials, shared by every robot,
//! each built and handed to the engine once, a few a frame.

pub(super) mod body;
pub(super) mod hats;
mod props;

use glam::Vec3;
use hamlet_engine_bevy::prelude::Handle;
use hamlet_engine_bevy::scene::{
    Material, Mesh, MeshData, StandardMaterial,
};

use crate::build::mesh::Mb;
use crate::build::stock::rgba;
use crate::build::upload::{Draw, Unsent};
use crate::folk::keeper::kit::body::{
    arm, emblem, eye, head, key, lamp, leg, mantle, pennant, pole,
    torso, wheels,
};
use crate::folk::keeper::kit::hats::hat;
pub(super) use crate::folk::keeper::kit::props::KITE;
use crate::folk::keeper::kit::props::{glint, prop};
use crate::folk::keeper::pose::Prop;
use crate::folk::keeper::who::{Emblem, Hat, Paint};
use crate::places::lantern;
use crate::world::palette::Palette;

/// A part made of up to two meshes: paint or wood, and brass.
#[derive(Default)]
struct Piece {
    wood: Mb,
    brass: Mb,
}

/// The places in the kit of a part's two meshes: paint or wood, and
/// brass.
pub(super) type Pair = (usize, usize);

/// A part of the kit, built as it is handed over.
#[derive(Clone, Copy)]
enum Shape {
    Hat(Hat),
    Torso,
    Arm,
    Leg,
    Pole,
    Key,
    Wheels,
    Head,
    Glass,
    Eye,
    Emblem(Emblem),
    Lamp,
    LampGlass,
    Ring,
    Prop(Prop),
    Glint(Prop),
    Mantle,
    Pennant,
}

impl Shape {
    fn build(self, pal: &Palette) -> Piece {
        let one = |wood: Mb| Piece {
            wood,
            brass: Mb::new(),
        };
        match self {
            Self::Hat(which) => hat(which, pal),
            Self::Torso => torso(pal),
            Self::Arm => arm(pal),
            Self::Leg => leg(pal),
            Self::Pole => pole(pal),
            Self::Key => one(key(pal)),
            Self::Wheels => one(wheels(pal)),
            Self::Head => one(head(pal)),
            Self::Glass => one(lantern::lantern_glass(0.145, 0.22)),
            Self::Eye => one(eye(pal)),
            Self::Emblem(which) => one(emblem(which, pal)),
            Self::Lamp => one(lamp(pal)),
            Self::LampGlass => {
                one(lantern::lantern_glass(0.08, 0.15))
            }
            Self::Ring => {
                let mut ring = Mb::new();
                ring.torus(
                    Vec3::ZERO,
                    Vec3::Y,
                    0.125,
                    0.009,
                    24,
                    Vec3::ONE,
                );
                one(ring)
            }
            Self::Prop(which) => one(prop(which, pal)),
            Self::Glint(which) => one(glint(which)),
            Self::Mantle => one(mantle(pal)),
            Self::Pennant => one(pennant(pal)),
        }
    }
}

/// The meshes every robot is made from, by their places in the kit,
/// and the paints. Robots share them, so a town of robots draws in a
/// few batches. A place whose part has no mesh stays empty.
pub struct Kit {
    pub paints: [Handle<Material>; 7],
    pub torso: Pair,
    pub key: usize,
    pub arm: Pair,
    pub leg: Pair,
    pub wheels: usize,
    pub head: usize,
    pub glass: usize,
    pub eye: usize,
    pub hats: [Pair; 7],
    pub emblems: [usize; 6],
    pub pole: Pair,
    pub lamp: usize,
    pub lamp_glass: usize,
    pub ring: usize,
    pub props: [usize; 9],
    /// The part of each prop that shows the robot's light.
    pub glints: [usize; 9],
    pub mantle: usize,
    pub pennant: usize,
    /// Every mesh, once it is handed over.
    meshes: Vec<Option<Handle<Mesh>>>,
    /// The parts yet to be built, the last first, each with the
    /// places of its meshes, and the meshes built but not yet handed
    /// over.
    unbuilt: Vec<(Shape, Pair)>,
    waiting: Vec<(usize, MeshData)>,
}

/// The parts of a kit as their places are given out.
#[derive(Default)]
struct Plan(Vec<(Shape, Pair)>);

impl Plan {
    fn pair(&mut self, shape: Shape) -> Pair {
        let at = self.0.len() * 2;
        self.0.push((shape, (at, at + 1)));
        (at, at + 1)
    }

    fn one(&mut self, shape: Shape) -> usize {
        self.pair(shape).0
    }
}

impl Kit {
    /// The paints, with every mesh still to build.
    pub fn make(
        pal: &Palette,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        let mut plan = Plan::default();
        let paints = Paint::ALL
            .map(|p| draw.material(matte(pal.paints[p as usize])));
        let [a, b, c, d, e, f, g] = paints;
        let paints = [a?, b?, c?, d?, e?, f?, g?];
        let hats = [
            Hat::Dome,
            Hat::Cap,
            Hat::Cone,
            Hat::Bowler,
            Hat::Pipe,
            Hat::Knit,
            Hat::Straw,
        ]
        .map(|which| plan.pair(Shape::Hat(which)));
        let torso = plan.pair(Shape::Torso);
        let key = plan.one(Shape::Key);
        let arm = plan.pair(Shape::Arm);
        let leg = plan.pair(Shape::Leg);
        let wheels = plan.one(Shape::Wheels);
        let head = plan.one(Shape::Head);
        let glass = plan.one(Shape::Glass);
        let eye = plan.one(Shape::Eye);
        let emblems = [
            Emblem::Keyhole,
            Emblem::Dot,
            Emblem::Bars,
            Emblem::Diamond,
            Emblem::Plus,
            Emblem::Chevron,
        ]
        .map(|which| plan.one(Shape::Emblem(which)));
        let pole = plan.pair(Shape::Pole);
        let lamp = plan.one(Shape::Lamp);
        let lamp_glass = plan.one(Shape::LampGlass);
        let ring = plan.one(Shape::Ring);
        let props = Prop::HELD.map(|p| plan.one(Shape::Prop(p)));
        let glints = Prop::HELD.map(|p| plan.one(Shape::Glint(p)));
        let mantle = plan.one(Shape::Mantle);
        let pennant = plan.one(Shape::Pennant);
        let Plan(unbuilt) = plan;
        Ok(Self {
            paints,
            torso,
            key,
            arm,
            leg,
            wheels,
            head,
            glass,
            eye,
            hats,
            emblems,
            pole,
            lamp,
            lamp_glass,
            ring,
            props,
            glints,
            mantle,
            pennant,
            meshes: (0..unbuilt.len() * 2).map(|_| None).collect(),
            unbuilt,
            waiting: Vec::new(),
        })
    }

    /// Builds the parts and hands over their meshes, as many as
    /// the frame has time for. True once every one is handed over.
    pub fn hand_over(
        &mut self,
        pal: &Palette,
        draw: &mut Draw,
    ) -> bool {
        while !self.unbuilt.is_empty() || !self.waiting.is_empty() {
            if !draw.left() {
                return false;
            }
            let Some((at, data)) = self.waiting.pop() else {
                let Some((shape, (wood, brass))) = self.unbuilt.pop()
                else {
                    break;
                };
                let piece = shape.build(pal);
                for (at, mb) in
                    [(wood, piece.wood), (brass, piece.brass)]
                {
                    if !mb.is_empty() {
                        self.waiting.push((at, mb.data()));
                    }
                }
                continue;
            };
            match draw.mesh(data) {
                Ok(mesh) => self.meshes[at] = Some(mesh),
                Err(Unsent::Full(data)) => {
                    self.waiting.push((at, data));
                    return false;
                }
                Err(Unsent::Refused(why)) => {
                    hamlet_engine_bevy::warn(&format!(
                        "a robot's part: {why}"
                    ));
                }
            }
        }
        true
    }

    /// The mesh at `at`, once it is handed over.
    pub fn mesh(&self, at: usize) -> Option<&Handle<Mesh>> {
        self.meshes.get(at).and_then(Option::as_ref)
    }
}

fn matte(colour: Vec3) -> StandardMaterial {
    StandardMaterial {
        perceptual_roughness: 0.92,
        reflectance: 0.18,
        ..StandardMaterial::colored(rgba(colour))
    }
}
