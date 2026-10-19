//! Lanterns: the brass frame and the glass, as every lamp in the
//! world is made.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use super::meshes::Mb;

/// Lanterns burn as the day goes.
pub fn plugin(app: &mut App) {
    app.add_systems(Update, burn);
}

/// A light that burns after dark and is out by day, with the light
/// it throws at its brightest.
#[derive(Component, Clone, Copy, Debug)]
pub struct NightLight(pub f32);

/// Lights every [`NightLight`] as the day goes.
pub fn burn(
    sky: Res<crate::Sky>,
    mut lamps: Query<(&NightLight, &mut PointLight)>,
) {
    for (lamp, mut light) in &mut lamps {
        let lumens = lamp.0 * sky.look.candle;
        if light.intensity != lumens {
            light.intensity = lumens;
        }
    }
}

/// Brass cap, base, bars and ring for a lantern whose glass is
/// centred at `c`.
pub fn lantern_frame(
    mb: &mut Mb,
    c: Vec3,
    r: f32,
    h: f32,
    col: Vec3,
) {
    lantern_cage(mb, c, r, h, col);
    let cap = [
        (r * 1.25, 0.0),
        (r * 1.3, 0.015),
        (r * 0.75, h * 0.3),
        (r * 0.3, h * 0.48),
        (r * 0.16, h * 0.56),
        (0.0, h * 0.58),
    ];
    mb.lathe(c + Vec3::Y * h * 0.5, &cap, 12, |_, _| col);
    mb.torus(
        c + Vec3::Y * (h * 0.5 + h * 0.58 + r * 0.3),
        Vec3::Z,
        r * 0.38,
        r * 0.09,
        12,
        col,
    );
}

/// A lantern's base and corner posts, with nothing on top.
pub(crate) fn lantern_cage(
    mb: &mut Mb,
    c: Vec3,
    r: f32,
    h: f32,
    col: Vec3,
) {
    let base = [
        (0.0, -0.035),
        (r * 0.9, -0.035),
        (r * 1.15, -0.012),
        (r * 1.15, 0.0),
        (0.0, 0.0),
    ];
    mb.lathe(c - Vec3::Y * h * 0.5, &base, 12, |_, _| col * 0.85);
    for i in 0..4 {
        let a = i as f32 * FRAC_PI_2 + 0.785;
        let d = Vec3::new(a.cos(), 0.0, a.sin()) * r * 1.02;
        mb.tube(
            c + d - Vec3::Y * h * 0.5,
            c + d + Vec3::Y * h * 0.5,
            r * 0.09,
            r * 0.09,
            5,
            col * 0.8,
            false,
        );
    }
}

/// The glass of a lantern, centred on the origin.
pub fn lantern_glass(r: f32, h: f32) -> Mesh {
    let mut mb = Mb::new();
    mb.tube(
        Vec3::Y * -h * 0.5,
        Vec3::Y * h * 0.5,
        r * 0.93,
        r * 0.93,
        14,
        Vec3::ONE,
        true,
    );
    mb.build()
}
