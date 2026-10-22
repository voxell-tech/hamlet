//! The merchant, who stands before the pavilion's counter in a
//! saffron robe and a striped turban, with a curled moustache and a
//! cup of tea, and waves when the chief buys something.

use glam::Vec3;

use crate::build::mesh::Mb;
use crate::folk::chief::look::{Cloak, Hat, Holds, Light, Look};
use crate::folk::chief::mascot::{Doing, Pose};
use crate::places::bazaar::build::{FLOOR, Shapes};
use crate::places::bazaar::figure::{Figure, standing};
use crate::world::palette::Palette;

/// The merchant's dress.
pub const LOOK: Look = Look {
    cloak: Cloak::Mustard,
    light: Light::Candle,
    hat: Hat::Bare,
    holds: Holds::Teacup,
};

/// The merchant's spot, from the bazaar's middle: before the
/// counter, facing the gate.
pub const STANDS: Vec3 = Vec3::new(0.0, FLOOR, 3.05);

/// The meshes about the merchant's head: its turban with a jewel,
/// and its moustache.
pub fn turban(pal: &Palette) -> Mb {
    let mut head = Mb::new();
    head.blob(
        Vec3::new(0.0, 0.17, -0.01),
        Vec3::new(0.31, 0.2, 0.31),
        5,
        0.08,
        1.3,
        |_| pal.bazaar.awnings[0],
    );
    head.torus(
        Vec3::new(0.0, 0.07, 0.0),
        Vec3::Y,
        0.28,
        0.06,
        14,
        pal.whitewash,
    );
    head.ball(
        Vec3::new(0.0, 0.15, 0.31),
        Vec3::splat(0.05),
        3,
        pal.brass,
    );
    for side in [-1.0f32, 1.0] {
        head.ball(
            Vec3::new(side * 0.1, -0.07, 0.255),
            Vec3::new(0.11, 0.04, 0.05),
            4,
            pal.hair,
        );
        head.ball(
            Vec3::new(side * 0.205, -0.03, 0.235),
            Vec3::new(0.035, 0.045, 0.035),
            3,
            pal.hair,
        );
    }
    head
}

/// Has the merchant stand and look about at clock time `now`, or
/// wave while a sale is `cheering`, `size` times the size it is
/// built at.
pub fn tend(
    figure: &mut Figure,
    now: f32,
    cheering: bool,
    size: f32,
) {
    let doing = match cheering {
        true => Doing::Wave,
        false => Doing::Idle,
    };
    figure.place(standing(STANDS, 0.0), &Pose::of(doing, now), size);
}
