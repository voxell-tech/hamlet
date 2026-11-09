//! The built-in assets drawn by Hamlet's own code: the parts of a
//! house that are not boxes.

use bevy::math::Affine3A;
use bevy::prelude::*;
use hamlet_blueprint::Frame;

use crate::lantern::lantern_cage;
use crate::meshes::{Meshing, Rng};

/// The height a tower is drawn at a scale of one along y.
pub(super) const TOWER: f32 = 4.0;

/// Draws the built-in asset `name` on `frame` into `meshing`, each of
/// its slots in the colour `colour` gives it. A slot `colour` gives
/// none is drawn into `apart` in white, as panes are. False for a
/// name with no code of its own.
pub(super) fn draw(
    name: &str,
    meshing: &mut Meshing,
    mut apart: Option<&mut Meshing>,
    frame: &Frame,
    colour: &dyn Fn(&str) -> Option<Vec3>,
) -> bool {
    let solid = |slot: &str| colour(slot).unwrap_or(Vec3::splat(0.7));
    let place = Affine3A::from_scale_rotation_translation(
        frame.scale,
        frame.rotation,
        frame.translation,
    );
    // A tower takes its height from its scale and keeps its girth.
    let upright = Affine3A::from_rotation_translation(
        frame.rotation,
        frame.translation,
    );
    let was = meshing.transform;
    let apart_was = apart.as_deref().map(|apart| apart.transform);
    meshing.at(if name == "tower" { upright } else { place });
    if let Some(apart) = apart.as_deref_mut() {
        apart.at(place);
    }
    let known = match name {
        "knob" => {
            let brass = solid(hamlet_graph::BODY);
            meshing.blob(
                Vec3::ZERO,
                Vec3::splat(0.035),
                3,
                0.0,
                0.0,
                |_| brass,
            );
            true
        }
        "shuttered_window" => {
            window(meshing, apart.as_deref_mut(), colour);
            true
        }
        "lantern" => {
            lantern(meshing, solid("bracket"), solid("cage"));
            true
        }
        "mast" => {
            let finial = solid("finial");
            let top = Vec3::Y * 1.9;
            meshing.tube(
                Vec3::ZERO,
                top,
                0.035,
                0.025,
                6,
                solid("pole"),
                true,
            );
            meshing.blob(top, Vec3::splat(0.07), 3, 0.0, 0.0, |_| {
                finial
            });
            true
        }
        "barrel" => {
            let timber = solid(hamlet_graph::BODY);
            let staves = [
                (0.0, 0.0),
                (0.18, 0.0),
                (0.23, 0.25),
                (0.18, 0.56),
                (0.0, 0.56),
            ];
            meshing.lathe(Vec3::ZERO, &staves, 8, |_, s| {
                timber * (1.1 + 0.08 * (s % 3) as f32)
            });
            true
        }
        "bush" => {
            bush(
                meshing,
                &mut Rng::seeded("bush"),
                solid("light"),
                solid("shade"),
                Vec3::ZERO,
                1.0,
            );
            true
        }
        "woodpile" => {
            let log = solid(hamlet_graph::BODY);
            let mut rng = Rng::seeded("woodpile");
            for i in 0..9 {
                let (row, k) = (i / 4, i % 4);
                let p = Vec3::new(
                    0.0,
                    0.1 + row as f32 * 0.17,
                    -0.5 + k as f32 * 0.19 + row as f32 * 0.09,
                );
                meshing.tube(
                    p - Vec3::X * 0.3,
                    p + Vec3::X * 0.3,
                    0.085,
                    0.085,
                    6,
                    log * rng.value(0.12),
                    true,
                );
            }
            true
        }
        "tower" => {
            let (stone, shade) = (solid("stone"), solid("shade"));
            let h = TOWER * frame.scale.y;
            meshing.lathe(
                -Vec3::Y * 0.3,
                &[
                    (0.92, 0.0),
                    (0.86, 0.5),
                    (0.8, h + 0.3),
                    (0.9, h + 0.42),
                ],
                12,
                |r, s| {
                    stone.lerp(
                        shade,
                        ((r * 7 + s * 3) % 5) as f32 * 0.07,
                    )
                },
            );
            true
        }
        "tower_roof" => {
            let (roof, drift) =
                (solid(hamlet_graph::BODY), solid("drift"));
            meshing.lathe(
                Vec3::ZERO,
                &[(1.12, 0.0), (0.7, 0.75), (0.3, 1.5), (0.0, 1.95)],
                12,
                |r, s| {
                    roof.lerp(
                        drift,
                        ((r * 5 + s * 7) % 4) as f32 * 0.25,
                    )
                },
            );
            true
        }
        _ => false,
    };
    meshing.at(was);
    if let (Some(apart), Some(was)) = (apart, apart_was) {
        apart.at(was);
    }
    known
}

/// A window with its frame, cross bars and shutters about its middle,
/// facing +z. Its pane is drawn into `apart` when `colour` gives the
/// pane none.
fn window(
    meshing: &mut Meshing,
    apart: Option<&mut Meshing>,
    colour: &dyn Fn(&str) -> Option<Vec3>,
) {
    let solid = |slot: &str| colour(slot).unwrap_or(Vec3::splat(0.7));
    let (w, h) = (0.2, 0.27);
    let frame = solid("frame");
    meshing.rbox(
        Vec3::ZERO,
        Vec3::new(w + 0.045, h + 0.045, 0.03),
        0.015,
        Quat::IDENTITY,
        frame,
        0.0,
    );
    let pane = [
        Vec3::new(-w, -h, 0.034),
        Vec3::new(w, -h, 0.034),
        Vec3::new(w, h, 0.034),
        Vec3::new(-w, h, 0.034),
    ];
    match (colour("pane"), apart) {
        (Some(lit), _) => meshing.face(pane, Vec3::Z, lit),
        (None, Some(apart)) => apart.face(pane, Vec3::Z, Vec3::ONE),
        (None, None) => {}
    }
    for (hw, hh) in [(0.014, h), (w, 0.014)] {
        meshing.face(
            [
                Vec3::new(-hw, -hh, 0.04),
                Vec3::new(hw, -hh, 0.04),
                Vec3::new(hw, hh, 0.04),
                Vec3::new(-hw, hh, 0.04),
            ],
            Vec3::Z,
            frame,
        );
    }
    let shutter = solid("shutter");
    for side in [-1.0f32, 1.0] {
        meshing.rbox(
            Vec3::new(side * (w + 0.15), 0.0, 0.035),
            Vec3::new(0.1, h + 0.02, 0.018),
            0.012,
            Quat::from_rotation_y(side * -0.22),
            shutter,
            0.1,
        );
    }
}

/// A lantern hung from a bracket on the wall behind it, at -z: the
/// cage, the cap and the bar, about where the light hangs.
fn lantern(meshing: &mut Meshing, bracket: Vec3, cage: Vec3) {
    meshing.tube(
        Vec3::new(0.0, 0.27, -0.3),
        Vec3::new(0.0, 0.3, 0.0),
        0.016,
        0.016,
        5,
        bracket,
        true,
    );
    meshing.tube(
        Vec3::Y * 0.3,
        Vec3::Y * 0.12,
        0.008,
        0.008,
        4,
        bracket,
        false,
    );
    lantern_cage(meshing, Vec3::ZERO, 0.075, 0.17, cage);
    meshing.lathe(
        Vec3::Y * 0.085,
        &[(0.095, 0.0), (0.05, 0.06), (0.0, 0.085)],
        8,
        |_, _| cage,
    );
}

/// A bush of two lumps, `r` across, standing at `c`.
pub(crate) fn bush(
    meshing: &mut Meshing,
    rng: &mut Rng,
    light: Vec3,
    shade: Vec3,
    c: Vec3,
    r: f32,
) {
    for _ in 0..2 {
        let o =
            Vec3::new(rng.sym(r * 0.5), r * 0.4, rng.sym(r * 0.5));
        let rr = r * rng.range(0.65, 0.95);
        let tint = rng.value(0.08);
        meshing.blob(
            c + o,
            Vec3::new(rr, rr * 0.8, rr),
            4,
            0.25,
            rng.f() * 30.0,
            move |d| {
                shade.lerp(light, (d.y * 0.6 + 0.5).clamp(0.0, 1.0))
                    * tint
            },
        );
    }
}
