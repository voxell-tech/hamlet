//! The robot's parts as meshes and materials, shared by every robot.

pub(super) mod body;
pub(super) mod hats;
mod props;

use bevy::prelude::*;

use self::body::{
    arm, emblem, eye, head, key, lamp, leg, mantle, pennant, pole,
    torso, wheels,
};
use self::hats::hat;
pub(super) use self::props::KITE;
use self::props::{glint, prop};
use super::pose::Prop;
use super::{Emblem, Hat, Paint};
use crate::meshes::Mb;
use crate::palette::Palette;
use crate::{Stock, lantern};

/// A part made of up to two meshes: paint or wood, and brass.
#[derive(Default)]
struct Piece {
    wood: Mb,
    brass: Mb,
}

type Pair = (Option<Handle<Mesh>>, Option<Handle<Mesh>>);

fn pair(meshes: &mut Assets<Mesh>, piece: Piece) -> Pair {
    let mut add =
        |mb: Mb| (!mb.is_empty()).then(|| meshes.add(mb.build()));
    (add(piece.wood), add(piece.brass))
}

/// The meshes and materials every robot is made from. Robots share
/// them, so a town of robots draws in a few batches.
#[derive(Resource)]
pub struct Kit {
    pub(super) matte: Handle<StandardMaterial>,
    pub(super) brass: Handle<StandardMaterial>,
    pub(super) paints: [Handle<StandardMaterial>; 7],
    pub(super) torso: Pair,
    pub(super) key: Handle<Mesh>,
    pub(super) arm: Pair,
    pub(super) leg: Pair,
    pub(super) wheels: Handle<Mesh>,
    pub(super) head: Handle<Mesh>,
    pub(super) glass: Handle<Mesh>,
    pub(super) eye: Handle<Mesh>,
    pub(super) hats: [Pair; 7],
    pub(super) emblems: [Handle<Mesh>; 6],
    pub(super) pole: Pair,
    pub(super) lamp: Handle<Mesh>,
    pub(super) lamp_glass: Handle<Mesh>,
    pub(super) ring: Handle<Mesh>,
    pub(super) props: [Handle<Mesh>; 9],
    /// The part of each prop that shows the robot's light.
    pub(super) glints: [Option<Handle<Mesh>>; 9],
    pub(super) mantle: Handle<Mesh>,
    pub(super) pennant: Handle<Mesh>,
    /// True when each robot's carried lantern lights the ground
    /// around it. One point light per robot.
    pub lamps_cast_light: bool,
}

impl Kit {
    pub fn new(
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        stock: &Stock,
        pal: &Palette,
    ) -> Self {
        let hat = |meshes: &mut Assets<Mesh>, h: Hat| {
            pair(meshes, hat(h, pal))
        };
        let mut ring = Mb::new();
        ring.torus(Vec3::ZERO, Vec3::Y, 0.125, 0.009, 24, Vec3::ONE);
        Self {
            matte: stock.matte.clone(),
            brass: stock.brass.clone(),
            paints: Paint::ALL.map(|p| {
                materials.add(matte(pal.paints[p as usize]))
            }),
            torso: pair(meshes, torso(pal)),
            key: meshes.add(key(pal).build()),
            arm: pair(meshes, arm(pal)),
            leg: pair(meshes, leg(pal)),
            wheels: meshes.add(wheels(pal).build()),
            head: meshes.add(head(pal).build()),
            glass: meshes.add(lantern::lantern_glass(0.145, 0.22)),
            eye: meshes.add(eye(pal).build()),
            hats: [
                hat(meshes, Hat::Dome),
                hat(meshes, Hat::Cap),
                hat(meshes, Hat::Cone),
                hat(meshes, Hat::Bowler),
                hat(meshes, Hat::Pipe),
                hat(meshes, Hat::Knit),
                hat(meshes, Hat::Straw),
            ],
            emblems: [
                Emblem::Keyhole,
                Emblem::Dot,
                Emblem::Bars,
                Emblem::Diamond,
                Emblem::Plus,
                Emblem::Chevron,
            ]
            .map(|e| meshes.add(emblem(e, pal).build())),
            pole: pair(meshes, pole(pal)),
            lamp: meshes.add(lamp(pal).build()),
            lamp_glass: meshes
                .add(lantern::lantern_glass(0.08, 0.15)),
            ring: meshes.add(ring.build()),
            props: Prop::HELD
                .map(|p| meshes.add(prop(p, pal).build())),
            glints: Prop::HELD.map(|p| {
                let mb = glint(p);
                (!mb.is_empty()).then(|| meshes.add(mb.build()))
            }),
            mantle: meshes.add(mantle(pal).build()),
            pennant: meshes.add(pennant(pal).build()),
            lamps_cast_light: true,
        }
    }
}

fn matte(colour: Vec3) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::linear_rgb(colour.x, colour.y, colour.z),
        perceptual_roughness: 0.92,
        reflectance: 0.18,
        ..default()
    }
}
