//! The small things a chief's hand leaves in the world: a heart or a
//! spark over a robot.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::RequestRedraw;

use crate::Clock;
use crate::hand::PlaySettings;
use crate::meshes::Meshing;
use crate::palette::Palette;

/// The meshes and lights of the toys.
#[derive(Resource)]
pub(super) struct Toys {
    heart: Handle<Mesh>,
    spark: Handle<Mesh>,
    warm: Handle<StandardMaterial>,
    cool: Handle<StandardMaterial>,
}

/// The kind of a toy, which says how it moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    /// Rises over a robot and fades.
    Heart,
    Spark,
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

pub(super) fn prepare(
    mut commands: Commands,
    pal: Res<Palette>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(Toys {
        heart: meshes.add(heart()),
        spark: meshes.add(Sphere::new(0.6).mesh().uv(6, 4)),
        warm: materials.add(lit(pal.wax, 1.6)),
        cool: materials.add(lit(pal.rune, 3.0)),
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
