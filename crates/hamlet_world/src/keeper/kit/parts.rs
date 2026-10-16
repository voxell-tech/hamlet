//! The parts of a robot a worn mesh replaces, by name, and the meshes
//! they have by default (docs/looks.md, "Agent meshes").

use bevy::prelude::*;

use super::body::{
    arm, emblem, eye, head, key, lamp, leg, mantle, pennant, pole,
    torso, wheels,
};
use super::hats::hat;
use super::{GLASS, LAMP_GLASS, Piece};
use crate::keeper::Who;
use crate::lantern::lantern_glass;
use crate::palette::Palette;

/// Every name a worn mesh's parts are found by. A part of one side,
/// such as `arm_l`, is found by its own name first and then by the
/// name both sides share, such as `arm`.
pub const MESH_PARTS: [&str; 23] = [
    "torso",
    "key",
    "emblem",
    "mantle",
    "arm",
    "arm_l",
    "arm_r",
    "leg",
    "leg_l",
    "leg_r",
    "wheels",
    "wheels_l",
    "wheels_r",
    "head",
    "glass",
    "eye",
    "eye_l",
    "eye_r",
    "hat",
    "pennant",
    "pole",
    "lamp",
    "lamp_glass",
];

/// A part of a robot a worn mesh can replace.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::keeper) enum Part {
    Torso,
    Key,
    Emblem,
    Mantle,
    ArmL,
    ArmR,
    LegL,
    LegR,
    WheelsL,
    WheelsR,
    Head,
    Glass,
    EyeL,
    EyeR,
    Hat,
    Pennant,
    Pole,
    Lamp,
    LampGlass,
}

impl Part {
    /// Its name in a worn mesh, and the name it shares with the part
    /// of the other side.
    pub fn names(self) -> (&'static str, Option<&'static str>) {
        match self {
            Part::Torso => ("torso", None),
            Part::Key => ("key", None),
            Part::Emblem => ("emblem", None),
            Part::Mantle => ("mantle", None),
            Part::ArmL => ("arm_l", Some("arm")),
            Part::ArmR => ("arm_r", Some("arm")),
            Part::LegL => ("leg_l", Some("leg")),
            Part::LegR => ("leg_r", Some("leg")),
            Part::WheelsL => ("wheels_l", Some("wheels")),
            Part::WheelsR => ("wheels_r", Some("wheels")),
            Part::Head => ("head", None),
            Part::Glass => ("glass", None),
            Part::EyeL => ("eye_l", Some("eye")),
            Part::EyeR => ("eye_r", Some("eye")),
            Part::Hat => ("hat", None),
            Part::Pennant => ("pennant", None),
            Part::Pole => ("pole", None),
            Part::Lamp => ("lamp", None),
            Part::LampGlass => ("lamp_glass", None),
        }
    }
}

/// The meshes the parts of the robot `who` have by default, by the
/// name both sides share, each in its part's own frame. Their vertex
/// colours are the colours drawn, paint included, so that drawn with
/// the shared matte material they look as the robot does.
pub fn default_parts(
    pal: &Palette,
    who: Who,
) -> Vec<(&'static str, Mesh)> {
    let paint = pal.paints[who.paint as usize];
    let whole = |piece: Piece, tint: Vec3| {
        let Piece { mut wood, brass } = piece;
        wood.tint(tint);
        wood.append(brass);
        wood.build()
    };
    vec![
        ("torso", whole(torso(pal), paint)),
        ("key", key(pal).build()),
        ("emblem", emblem(who.emblem, pal).build()),
        ("mantle", mantle(pal).build()),
        ("arm", whole(arm(pal), Vec3::ONE)),
        ("leg", whole(leg(pal), Vec3::ONE)),
        ("wheels", wheels(pal).build()),
        ("head", head(pal).build()),
        ("glass", lantern_glass(GLASS.x, GLASS.y)),
        ("eye", eye(pal).build()),
        ("hat", whole(hat(who.hat, pal), paint)),
        ("pennant", pennant(pal).build()),
        ("pole", whole(pole(pal), Vec3::ONE)),
        ("lamp", lamp(pal).build()),
        ("lamp_glass", lantern_glass(LAMP_GLASS.x, LAMP_GLASS.y)),
    ]
}
