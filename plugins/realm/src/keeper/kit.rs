//! The robot's parts as meshes and materials, shared by every robot
//! and handed to the engine once.

pub(super) mod body;
pub(super) mod hats;
mod props;

use glam::Vec3;
use hamlet_engine_sdk::scene::{Material, Mesh, MeshData, Pbr};

use self::body::{
    arm, emblem, eye, head, key, lamp, leg, mantle, pennant, pole,
    torso, wheels,
};
use self::hats::hat;
pub(super) use self::props::KITE;
use self::props::{glint, prop};
use super::pose::Prop;
use super::who::{Emblem, Hat, Paint};
use crate::lantern;
use crate::mesh::Mb;
use crate::palette::Palette;
use crate::stock::rgba;
use crate::upload::{Unsent, Upload};

/// A part made of up to two meshes: paint or wood, and brass.
#[derive(Default)]
struct Piece {
    wood: Mb,
    brass: Mb,
}

/// The places in the kit of a part's two meshes, either of which
/// may be missing.
pub(super) type Pair = (Option<usize>, Option<usize>);

/// The meshes every robot is made from, by their places in the kit,
/// and the paints. Robots share them, so a town of robots draws in a
/// few batches.
pub(super) struct Kit {
    pub paints: [Material; 7],
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
    pub glints: [Option<usize>; 9],
    pub mantle: usize,
    pub pennant: usize,
    /// Every mesh, once it is handed over, and those still waiting.
    meshes: Vec<Option<Mesh>>,
    waiting: Vec<(usize, MeshData)>,
}

/// The meshes of a kit as they are made, before they are handed over.
#[derive(Default)]
struct Making(Vec<MeshData>);

impl Making {
    fn add(&mut self, mb: Mb) -> usize {
        self.0.push(mb.data());
        self.0.len() - 1
    }

    fn maybe(&mut self, mb: Mb) -> Option<usize> {
        (!mb.is_empty()).then(|| self.add(mb))
    }

    fn pair(&mut self, piece: Piece) -> Pair {
        (self.maybe(piece.wood), self.maybe(piece.brass))
    }
}

impl Kit {
    pub fn make(pal: &Palette) -> Result<Self, String> {
        let mut made = Making::default();
        let mut ring = Mb::new();
        ring.torus(Vec3::ZERO, Vec3::Y, 0.125, 0.009, 24, Vec3::ONE);
        let paints = Paint::ALL
            .map(|p| Material::make(matte(pal.paints[p as usize])));
        let [a, b, c, d, e, f, g] = paints;
        let paints = [a?, b?, c?, d?, e?, f?, g?];
        let mut hat = |h: Hat| made.pair(hat(h, pal));
        let hats = [
            hat(Hat::Dome),
            hat(Hat::Cap),
            hat(Hat::Cone),
            hat(Hat::Bowler),
            hat(Hat::Pipe),
            hat(Hat::Knit),
            hat(Hat::Straw),
        ];
        let torso = made.pair(torso(pal));
        let key = made.add(key(pal));
        let arm = made.pair(arm(pal));
        let leg = made.pair(leg(pal));
        let wheels = made.add(wheels(pal));
        let head = made.add(head(pal));
        let glass = made.add(lantern::lantern_glass(0.145, 0.22));
        let eye = made.add(eye(pal));
        let emblems = [
            Emblem::Keyhole,
            Emblem::Dot,
            Emblem::Bars,
            Emblem::Diamond,
            Emblem::Plus,
            Emblem::Chevron,
        ]
        .map(|e| made.add(emblem(e, pal)));
        let pole = made.pair(pole(pal));
        let lamp = made.add(lamp(pal));
        let lamp_glass = made.add(lantern::lantern_glass(0.08, 0.15));
        let ring = made.add(ring);
        let props = Prop::HELD.map(|p| made.add(prop(p, pal)));
        let glints = Prop::HELD.map(|p| made.maybe(glint(p)));
        let mantle = made.add(mantle(pal));
        let pennant = made.add(pennant(pal));
        let waiting =
            made.0.into_iter().enumerate().collect::<Vec<_>>();
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
            meshes: (0..waiting.len()).map(|_| None).collect(),
            waiting,
        })
    }

    /// Hands over the meshes still waiting, as many as `upload`
    /// allows. True once every one is handed over.
    pub fn hand_over(&mut self, upload: &mut Upload) -> bool {
        while let Some((at, data)) = self.waiting.pop() {
            match upload.mesh(data) {
                Ok(mesh) => self.meshes[at] = Some(mesh),
                Err(Unsent::Full(data)) => {
                    self.waiting.push((at, data));
                    return false;
                }
                Err(Unsent::Refused(why)) => {
                    hamlet_engine_sdk::warn(&format!(
                        "a robot's part: {why}"
                    ));
                }
            }
        }
        true
    }

    /// The mesh at `at`, once it is handed over.
    pub fn mesh(&self, at: usize) -> Option<&Mesh> {
        self.meshes.get(at).and_then(Option::as_ref)
    }
}

fn matte(colour: Vec3) -> Pbr {
    Pbr {
        roughness: 0.92,
        ..Pbr::colored(rgba(colour))
    }
}
