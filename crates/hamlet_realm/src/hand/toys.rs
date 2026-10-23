//! The small things a chief's hand leaves in the world: a heart or a
//! spark over a robot, a letter, a gift in a yard.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::RequestRedraw;

use crate::hand::PlaySettings;
use crate::meshes::Meshing;
use crate::palette::Palette;
use crate::{Clock, Stock};

/// The meshes and lights of the toys.
#[derive(Resource)]
pub(super) struct Toys {
    heart: Handle<Mesh>,
    spark: Handle<Mesh>,
    gift: Handle<Mesh>,
    letter: Handle<Mesh>,
    warm: Handle<StandardMaterial>,
    cool: Handle<StandardMaterial>,
    matte: Handle<StandardMaterial>,
}

/// The kind of a toy, which says how it moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    /// Rises over a robot and fades.
    Heart,
    Spark,
    /// Falls and is gone as it lands.
    Letter,
    /// Falls, bounces and stays a while.
    Gift,
}

/// One toy, when it began and where it goes.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct Toy {
    pub(super) kind: Kind,
    began: f32,
    from: Vec3,
    to: Vec3,
    own: f32,
}

/// A light that glows in its own colour.
pub(super) fn lit(colour: Vec3, strength: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::LinearRgba(LinearRgba::from_vec3(colour)),
        emissive: LinearRgba::from_vec3(colour * strength),
        perceptual_roughness: 0.6,
        ..default()
    }
}

fn disc(meshing: &mut Meshing, at: Vec3, radius: f32, colour: Vec3) {
    let thick = radius * 0.3;
    meshing.lathe(
        at,
        &[(0.0, 0.0), (radius, 0.0), (radius, thick), (0.0, thick)],
        10,
        |_, _| colour,
    );
}

fn heart() -> Mesh {
    let mut meshing = Meshing::new();
    let white = |_: Vec3| Vec3::ONE;
    for side in [-1.0f32, 1.0] {
        meshing.blob(
            Vec3::new(side * 0.42, 0.5, 0.0),
            Vec3::new(0.52, 0.5, 0.36),
            5,
            0.0,
            0.0,
            white,
        );
    }
    meshing.lathe(
        Vec3::new(0.0, -0.75, 0.0),
        &[(0.0, 0.0), (0.5, 0.62), (0.86, 1.2), (0.0, 1.3)],
        10,
        |_, _| Vec3::ONE,
    );
    meshing.build()
}

fn gift(pal: &Palette) -> Mesh {
    let mut meshing = Meshing::new();
    let upright = Quat::IDENTITY;
    meshing.rbox(
        Vec3::Y * 0.5,
        Vec3::splat(0.5),
        0.04,
        upright,
        pal.log,
        0.25,
    );
    for side in [-1.0f32, 1.0] {
        meshing.rbox(
            Vec3::new(side * 0.32, 0.5, 0.0),
            Vec3::new(0.06, 0.53, 0.53),
            0.01,
            upright,
            pal.timber_dark,
            0.2,
        );
    }
    // A tag that says what is inside.
    meshing.rbox(
        Vec3::new(0.0, 0.62, 0.52),
        Vec3::new(0.2, 0.14, 0.012),
        0.005,
        Quat::from_rotation_z(0.12),
        pal.parchment,
        0.0,
    );
    meshing.rbox(
        Vec3::new(0.0, 1.02, 0.0),
        Vec3::new(0.12, 0.03, 0.12),
        0.01,
        upright,
        pal.wax,
        0.0,
    );
    meshing.build()
}

fn letter(pal: &Palette) -> Mesh {
    let mut meshing = Meshing::new();
    meshing.rbox(
        Vec3::ZERO,
        Vec3::new(0.5, 0.012, 0.34),
        0.005,
        Quat::IDENTITY,
        pal.parchment,
        0.0,
    );
    disc(&mut meshing, Vec3::Y * 0.01, 0.09, pal.wax);
    meshing.build()
}

pub(super) fn prepare(
    mut commands: Commands,
    pal: Res<Palette>,
    stock: Res<Stock>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(Toys {
        heart: meshes.add(heart()),
        spark: meshes.add(Sphere::new(0.6).mesh().uv(6, 4)),
        gift: meshes.add(gift(&pal)),
        letter: meshes.add(letter(&pal)),
        warm: materials.add(lit(pal.wax, 1.6)),
        cool: materials.add(lit(pal.rune, 3.0)),
        matte: stock.matte.clone(),
    });
}

/// Sets a toy of `kind` going at clock time `began`, from `from` to
/// `to`. `own` tells toys of one moment apart.
pub(super) fn toss(
    commands: &mut Commands,
    toys: &Toys,
    kind: Kind,
    began: f32,
    (from, to): (Vec3, Vec3),
    own: f32,
) -> Entity {
    let (mesh, fill) = match kind {
        Kind::Heart => (&toys.heart, &toys.warm),
        Kind::Spark => (&toys.spark, &toys.cool),
        Kind::Letter => (&toys.letter, &toys.matte),
        Kind::Gift => (&toys.gift, &toys.matte),
    };
    commands
        .spawn((
            Toy {
                kind,
                began,
                from,
                to,
                own,
            },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(fill.clone()),
            Transform::from_translation(from).with_scale(Vec3::ZERO),
            NotShadowCaster,
        ))
        .id()
}

/// The place and size of a toy `t` seconds after it began. `None`
/// once it is over.
fn placed(
    toy: &Toy,
    t: f32,
    set: &PlaySettings,
) -> Option<Transform> {
    let spin = Quat::from_rotation_y(t * 2.2 + toy.own);
    match toy.kind {
        Kind::Heart | Kind::Spark => {
            let u = t / set.heart_time;
            if u >= 1.0 {
                return None;
            }
            // It pops out, drifts up with a sway and thins away.
            let pop = (u * 5.0).min(1.0);
            let size = (1.0 - (1.0 - pop).powi(3))
                * (1.0 - ((u - 0.6) / 0.4).clamp(0.0, 1.0).powi(2));
            let sway =
                Vec3::new((t * 5.0 + toy.own).sin(), 0.0, 0.0) * 0.06;
            let at = toy.from.lerp(toy.to, 1.0 - (1.0 - u).powi(2));
            Some(
                Transform::from_translation(at + sway)
                    .with_rotation(Quat::from_rotation_y(
                        (t * 2.6 + toy.own).sin() * 0.6,
                    ))
                    .with_scale(Vec3::splat(set.heart_size * size)),
            )
        }
        Kind::Letter => {
            // Each sets out a little after the last.
            let u = (t - toy.own * 0.07) / set.drop_time;
            if u >= 1.0 {
                return None;
            }
            let u = u.max(0.0);
            let at = toy.from.lerp(toy.to, u * u);
            // A letter rocks as it comes down.
            let rock = (t * 7.0).sin() * 0.5 * (1.0 - u);
            let (size, turn) = (0.5, Quat::from_rotation_z(rock));
            Some(
                Transform::from_translation(at)
                    .with_rotation(spin * turn)
                    .with_scale(Vec3::splat(
                        size * (u * 8.0).min(1.0),
                    )),
            )
        }
        Kind::Gift => {
            if t >= set.drop_time + set.gift_stays {
                return None;
            }
            let u = (t / set.drop_time).min(1.0);
            let after = t - set.drop_time;
            // It lands with a bounce and a squash, and is taken in
            // at the end of its stay.
            let bounce = if after > 0.0 {
                (-after * 5.0).exp()
                    * (after * 11.0).sin().abs()
                    * 0.3
            } else {
                0.0
            };
            let squash = if after > 0.0 {
                1.0 - 0.25
                    * (-after * 9.0).exp()
                    * (after * 18.0).cos()
            } else {
                1.0
            };
            let leave =
                ((set.gift_stays - after) / 0.5).clamp(0.0, 1.0);
            let at = toy.from.lerp(toy.to, u * u) + Vec3::Y * bounce;
            let wide = squash.max(0.2).sqrt().recip();
            Some(
                Transform::from_translation(at)
                    .with_rotation(Quat::from_rotation_y(toy.own))
                    .with_scale(
                        Vec3::new(wide, squash, wide)
                            * set.meter_size
                            * leave,
                    ),
            )
        }
    }
}

/// Plays every toy and clears each away at its end.
pub(super) fn play(
    mut commands: Commands,
    clock: Res<Clock>,
    set: Res<PlaySettings>,
    mut toys: Query<(Entity, &Toy, &mut Transform)>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if toys.is_empty() {
        return;
    }
    redraw.write(RequestRedraw);
    for (entity, toy, mut at) in &mut toys {
        match placed(toy, clock.t - toy.began, &set) {
            Some(to) => *at = to,
            None => commands.entity(entity).try_despawn(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toy(kind: Kind) -> Toy {
        Toy {
            kind,
            began: 0.0,
            from: Vec3::Y * 3.0,
            to: Vec3::ZERO,
            own: 0.0,
        }
    }

    #[test]
    fn every_toy_is_finite_and_those_that_end_do() {
        let set = PlaySettings::default();
        for kind in
            [Kind::Heart, Kind::Spark, Kind::Letter, Kind::Gift]
        {
            let toy = toy(kind);
            for i in 0..400 {
                if let Some(at) = placed(&toy, i as f32 * 0.02, &set)
                {
                    assert!(at.is_finite(), "{kind:?} at step {i}");
                }
            }
            assert!(placed(&toy, 600.0, &set).is_none(), "{kind:?}");
        }
        // A gift is in the yard once it has landed.
        let landed = placed(&toy(Kind::Gift), 5.0, &set).unwrap();
        assert!(landed.translation.length() < 0.01);
    }
}
