//! The librarian, who stands on the library's porch and turns to
//! whoever calls on it.

use bevy::prelude::*;
use hamlet_client::realm::Consulted;

use crate::Clock;
use crate::chief::mascot::{self, Doing, Pose, Stance};
use crate::chief::{ChiefSettings, Cloak, Hat, Holds, Light, Look};
use crate::meshes::Meshing;
use crate::palette::Palette;

/// The librarian: a sage and no chief. It is old and tall, with
/// white hair, a white beard down to its belt, spectacles and a
/// staff, and it carries a book.
#[derive(Component, Clone, Copy, Debug)]
pub struct Librarian {
    /// The time an agent last called on it.
    pub called: Option<f32>,
}

/// The librarian's dress.
pub(super) const LOOK: Look = Look {
    cloak: Cloak::Slate,
    light: Light::ALL[2],
    hat: Hat::Bare,
    holds: Holds::Book,
};

/// The meshes that set the librarian apart from a chief: the white
/// hair, brows, spectacles and beard about its head, its staff, and
/// the stone in the staff with where it sits.
pub(super) fn sage(
    pal: &Palette,
) -> (Meshing, Meshing, (Meshing, Vec3)) {
    let white = pal.page;
    let mut head = Meshing::new();
    let mut ball = |c: Vec3, r: Vec3| {
        head.blob(c, r, 5, 0.0, 0.0, |_| white);
    };
    // Hair over its own, and long at the back.
    ball(Vec3::new(0.0, 0.11, -0.03), Vec3::new(0.29, 0.21, 0.285));
    ball(Vec3::new(0.0, -0.1, -0.2), Vec3::new(0.24, 0.3, 0.12));
    // The beard, wide at the chin and down to a point at the belt.
    ball(Vec3::new(0.0, -0.2, 0.17), Vec3::new(0.22, 0.2, 0.13));
    ball(Vec3::new(0.0, -0.45, 0.2), Vec3::new(0.16, 0.26, 0.1));
    ball(Vec3::new(0.0, -0.7, 0.21), Vec3::new(0.08, 0.16, 0.06));
    for side in [-1.0f32, 1.0] {
        // A moustache and a brow at each side.
        ball(
            Vec3::new(side * 0.1, -0.07, 0.255),
            Vec3::new(0.11, 0.045, 0.05),
        );
        ball(
            Vec3::new(side * 0.11, 0.12, 0.25),
            Vec3::new(0.085, 0.03, 0.04),
        );
    }
    for side in [-1.0f32, 1.0] {
        head.torus(
            Vec3::new(side * 0.105, 0.035, 0.275),
            Vec3::Z,
            0.075,
            0.012,
            10,
            pal.brass,
        );
    }
    head.tube(
        Vec3::new(-0.03, 0.04, 0.28),
        Vec3::new(0.03, 0.04, 0.28),
        0.01,
        0.01,
        4,
        pal.brass,
        false,
    );
    // A staff taller than itself, with a crook and a stone in it.
    let mut staff = Meshing::new();
    let foot = Vec3::new(-0.42, 0.0, 0.12);
    let top = foot + Vec3::new(0.1, 1.78, 0.0);
    staff.tube(
        foot,
        foot + Vec3::Y * 1.75,
        0.035,
        0.028,
        6,
        pal.timber_dark,
        true,
    );
    staff.torus(top, Vec3::Z, 0.1, 0.028, 10, pal.timber_dark);
    // The stone is drawn apart, since it glows.
    let mut stone = Meshing::new();
    stone.blob(top, Vec3::splat(0.075), 4, 0.0, 0.0, |_| Vec3::ONE);
    (head, staff, (stone, top))
}

/// The librarian's spot, from the library's middle.
pub(super) const STANDS: Vec3 = Vec3::new(0.0, 0.6, 5.05);

/// Its size as a multiple of a chief's mascot.
const GROWN: f32 = 1.18;

/// The seconds it attends to whoever called on it.
pub(super) const ATTENDS: f32 = 6.0;

/// An agent called on the librarian: it turns to them, and the
/// library shows it.
pub(super) fn called(
    _: On<Consulted>,
    clock: Res<Clock>,
    mut librarians: Query<&mut Librarian>,
) {
    for mut librarian in &mut librarians {
        librarian.called = Some(clock.t);
    }
}

/// Has the librarian take its stance: it stands and looks about,
/// and turns to whoever has just called on it. The same system
/// that places a chief's mascot places it.
pub(super) fn tend(
    clock: Res<Clock>,
    set: Res<ChiefSettings>,
    mut librarians: Query<(&Librarian, &mut Stance)>,
) {
    for (librarian, mut stance) in &mut librarians {
        let busy =
            librarian.called.is_some_and(|at| clock.t - at < ATTENDS);
        let doing = if busy { Doing::Wave } else { Doing::Idle };
        let to = Stance {
            pose: Pose::of(doing, clock.t),
            size: set.size * GROWN,
            crest: mascot::crest(LOOK.hat),
        };
        stance.set_if_neq(to);
    }
}
