//! The merchant, who stands before the pavilion's counter and waves
//! when the chief buys a model.

use bevy::prelude::*;

use super::build::FLOOR;
use crate::Clock;
use crate::chief::mascot::{self, Doing, Pose, Stance};
use crate::chief::{ChiefSettings, Cloak, Hat, Holds, Light, Look};
use crate::meshes::Mb;
use crate::palette::Palette;

/// The merchant: no chief, in a saffron robe and a striped turban,
/// with a curled moustache and a cup of tea.
#[derive(Component, Clone, Copy, Debug)]
pub struct Merchant {
    /// The time the chief last bought a model.
    pub sold: Option<f32>,
}

/// The chief added a model to the home in the bazaar.
#[derive(Event, Clone, Copy, Debug)]
pub struct Traded;

/// The merchant's dress.
pub(super) const LOOK: Look = Look {
    cloak: Cloak::Mustard,
    light: Light::ALL[0],
    hat: Hat::Bare,
    holds: Holds::Teacup,
};

/// The merchant's spot, from the bazaar's middle: before the
/// counter, facing the gate.
pub(super) const STANDS: Vec3 = Vec3::new(0.0, FLOOR, 1.85);

/// The seconds it waves after a sale.
pub(super) const CHEERS: f32 = 5.0;

/// The meshes about the merchant's head: its turban with a jewel,
/// and its moustache.
pub(super) fn turban(pal: &Palette) -> Mb {
    let mut head = Mb::new();
    let cloth = pal.awnings[0];
    head.blob(
        Vec3::new(0.0, 0.17, -0.01),
        Vec3::new(0.31, 0.2, 0.31),
        5,
        0.08,
        1.3,
        |_| cloth,
    );
    head.torus(
        Vec3::new(0.0, 0.07, 0.0),
        Vec3::Y,
        0.28,
        0.06,
        14,
        pal.whitewash,
    );
    head.blob(
        Vec3::new(0.0, 0.15, 0.31),
        Vec3::splat(0.05),
        3,
        0.0,
        0.0,
        |_| pal.brass,
    );
    for side in [-1.0f32, 1.0] {
        head.blob(
            Vec3::new(side * 0.1, -0.07, 0.255),
            Vec3::new(0.11, 0.04, 0.05),
            4,
            0.0,
            0.0,
            |_| pal.hair,
        );
        head.blob(
            Vec3::new(side * 0.205, -0.03, 0.235),
            Vec3::new(0.035, 0.045, 0.035),
            3,
            0.0,
            0.0,
            |_| pal.hair,
        );
    }
    head
}

/// The chief bought a model: the merchant waves, and the bazaar
/// shows it.
pub(super) fn sold(
    _: On<Traded>,
    clock: Res<Clock>,
    mut merchants: Query<&mut Merchant>,
) {
    for mut merchant in &mut merchants {
        merchant.sold = Some(clock.t);
    }
}

/// Has the merchant take its stance: it stands and looks about, and
/// waves a while after a sale. The same system that places a chief's
/// mascot places it.
pub(super) fn tend(
    clock: Res<Clock>,
    set: Res<ChiefSettings>,
    mut merchants: Query<(&Merchant, &mut Stance)>,
) {
    for (merchant, mut stance) in &mut merchants {
        let cheers =
            merchant.sold.is_some_and(|at| clock.t - at < CHEERS);
        let doing = if cheers { Doing::Wave } else { Doing::Idle };
        let to = Stance {
            pose: Pose::of(doing, clock.t),
            size: set.size,
            crest: mascot::crest(LOOK.hat),
        };
        stance.set_if_neq(to);
    }
}
