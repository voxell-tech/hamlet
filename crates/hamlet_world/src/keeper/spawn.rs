//! A robot put into the world, part by part.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::acts::{Act, Quirk};
use super::kit::Kit;
use super::kit::body::{KEY_AT, LAMP_DROP};
use super::kit::hats::SPIRE;
use super::kit::parts::Part;
use super::pose::{
    FACE, HAT_AT, HIP, HOOK, NECK, Pose, Prop, SHOULDER,
};
use super::wear::{Shape, Slot, Wears};
use super::{KState, Keeper, LONG_AGO, Parts, Who};

/// A mesh with its material, and how a skin colours it.
type Coat<'a> =
    (&'a Option<Handle<Mesh>>, &'a Handle<StandardMaterial>, Slot);

/// A pivot carrying a part's meshes. The first mesh rides on the
/// pivot itself.
fn part(
    commands: &mut Commands,
    shapes: &mut Vec<Shape>,
    parent: Entity,
    at: Vec3,
    of: Part,
    meshes: [Coat; 2],
) -> Entity {
    let pivot = commands
        .spawn((
            Transform::from_translation(at),
            Visibility::Inherited,
            ChildOf(parent),
        ))
        .id();
    let mut on_pivot = false;
    for (mesh, mat, slot) in meshes {
        let Some(mesh) = mesh else { continue };
        let bundle =
            (Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()));
        let entity = if on_pivot {
            commands
                .spawn((
                    bundle,
                    Visibility::Inherited,
                    ChildOf(pivot),
                ))
                .id()
        } else {
            commands.entity(pivot).try_insert(bundle);
            on_pivot = true;
            pivot
        };
        shapes.push(Shape::new(entity, of, slot, mesh, mat));
    }
    pivot
}

/// Spawns a mesh of a part and keeps it in `shapes`.
fn shape(
    commands: &mut Commands,
    shapes: &mut Vec<Shape>,
    of: Part,
    (mesh, mat, slot): (
        &Handle<Mesh>,
        &Handle<StandardMaterial>,
        Slot,
    ),
    bundle: impl Bundle,
) -> Entity {
    let entity = commands
        .spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            // A replaced part hides the rest of its meshes.
            Visibility::Inherited,
            bundle,
        ))
        .id();
    shapes.push(Shape::new(entity, of, slot, mesh, mat));
    entity
}

/// Spawns a robot standing at `at`. It stays where it is put unless
/// it is given a `Trip`. With `born`, the clock time of its
/// creation, it plays its first appearance from then; without, it is
/// simply there, idle.
pub fn spawn_agent(
    commands: &mut Commands,
    kit: &Kit,
    materials: &mut Assets<StandardMaterial>,
    who: Who,
    at: Transform,
    born: Option<f32>,
) -> Entity {
    raise(commands, kit, materials, who, at, KState::Rest, born)
}

/// Spawns a robot standing at `at`, settled in the plain motion of
/// `state`.
pub fn spawn_as(
    commands: &mut Commands,
    kit: &Kit,
    materials: &mut Assets<StandardMaterial>,
    who: Who,
    at: Transform,
    state: KState,
) -> Entity {
    raise(commands, kit, materials, who, at, state, None)
}

/// Spawns a robot standing at `at`, at rest, wearing `wears`.
pub fn spawn_dressed(
    commands: &mut Commands,
    kit: &Kit,
    materials: &mut Assets<StandardMaterial>,
    who: Who,
    at: Transform,
    wears: Wears,
) -> Entity {
    let robot =
        raise(commands, kit, materials, who, at, KState::Rest, None);
    commands.entity(robot).try_insert(wears);
    robot
}

fn raise(
    commands: &mut Commands,
    kit: &Kit,
    materials: &mut Assets<StandardMaterial>,
    who: Who,
    at: Transform,
    state: KState,
    born: Option<f32>,
) -> Entity {
    let root = commands.spawn((at, Visibility::Inherited)).id();
    let rig = commands
        .spawn((
            Transform::from_scale(Vec3::splat(who.size)),
            Visibility::Inherited,
            ChildOf(root),
        ))
        .id();
    let mut shapes = Vec::with_capacity(24);
    let shapes = &mut shapes;
    let paint = &kit.paints[who.paint as usize];
    let (brass, matte) = (&kit.brass, &kit.matte);
    let body = part(
        commands,
        shapes,
        rig,
        Vec3::Y * HIP,
        Part::Torso,
        [
            (&kit.torso.0, paint, Slot::Paint),
            (&kit.torso.1, brass, Slot::Brass),
        ],
    );
    let legs = [(-0.11f32, Part::LegL), (0.11, Part::LegR)].map(
        |(x, of)| {
            part(
                commands,
                shapes,
                rig,
                Vec3::new(x, HIP, 0.0),
                of,
                [
                    (&kit.leg.0, matte, Slot::Steel),
                    (&kit.leg.1, brass, Slot::Brass),
                ],
            )
        },
    );
    let wheels = [(legs[0], Part::WheelsL), (legs[1], Part::WheelsR)]
        .map(|(leg, of)| {
            shape(
                commands,
                shapes,
                of,
                (&kit.wheels, brass, Slot::Brass),
                (
                    Transform::from_translation(
                        Pose::stand().place(1.0).axle,
                    ),
                    ChildOf(leg),
                ),
            )
        });
    let arms = [(-1.0f32, Part::ArmL), (1.0, Part::ArmR)].map(
        |(side, of)| {
            part(
                commands,
                shapes,
                body,
                SHOULDER * Vec3::new(side, 1.0, 1.0),
                of,
                [
                    (&kit.arm.0, matte, Slot::Steel),
                    (&kit.arm.1, brass, Slot::Brass),
                ],
            )
        },
    );
    shape(
        commands,
        shapes,
        Part::Emblem,
        (&kit.emblems[who.emblem as usize], matte, Slot::Plain),
        (NotShadowCaster, ChildOf(body)),
    );
    let key = shape(
        commands,
        shapes,
        Part::Key,
        (&kit.key, brass, Slot::Brass),
        (Transform::from_translation(KEY_AT), ChildOf(body)),
    );
    if who.leader {
        shape(
            commands,
            shapes,
            Part::Mantle,
            (&kit.mantle, matte, Slot::Plain),
            ChildOf(body),
        );
    }

    let head = shape(
        commands,
        shapes,
        Part::Head,
        (&kit.head, brass, Slot::Brass),
        (Transform::from_translation(Vec3::Y * NECK), ChildOf(body)),
    );
    // One material for both lanterns and the ring, so they show the
    // same state.
    let lit = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.19, 0.17),
        perceptual_roughness: 0.4,
        ..default()
    });
    shape(
        commands,
        shapes,
        Part::Glass,
        (&kit.glass, &lit, Slot::Lit),
        (
            Transform::from_translation(Vec3::Y * FACE),
            NotShadowCaster,
            ChildOf(head),
        ),
    );
    let eyes = [Part::EyeL, Part::EyeR].map(|of| {
        shape(
            commands,
            shapes,
            of,
            (&kit.eye, matte, Slot::Plain),
            (
                Transform::from_translation(Vec3::Y * FACE),
                NotShadowCaster,
                ChildOf(head),
            ),
        )
    });
    let worn = &kit.hats[who.hat as usize];
    let hat = part(
        commands,
        shapes,
        head,
        Vec3::Y * HAT_AT,
        Part::Hat,
        [
            (&worn.0, paint, Slot::Paint),
            (&worn.1, brass, Slot::Brass),
        ],
    );
    let flag = who.leader.then(|| {
        shape(
            commands,
            shapes,
            Part::Pennant,
            (&kit.pennant, matte, Slot::Plain),
            (
                Transform::from_translation(Vec3::Y * (SPIRE - 0.02)),
                ChildOf(hat),
            ),
        )
    });

    let pole = part(
        commands,
        shapes,
        rig,
        Pose::stand().pole_at,
        Part::Pole,
        [
            (&kit.pole.0, matte, Slot::Steel),
            (&kit.pole.1, brass, Slot::Brass),
        ],
    );
    let lamp = shape(
        commands,
        shapes,
        Part::Lamp,
        (&kit.lamp, brass, Slot::Brass),
        (Transform::from_translation(HOOK), ChildOf(pole)),
    );
    shape(
        commands,
        shapes,
        Part::LampGlass,
        (&kit.lamp_glass, &lit, Slot::Lit),
        (
            Transform::from_translation(LAMP_DROP),
            NotShadowCaster,
            ChildOf(lamp),
        ),
    );
    // The ring and the light go with the carried lantern, which
    // leads.
    let ring = commands
        .spawn((
            Mesh3d(kit.ring.clone()),
            MeshMaterial3d(lit.clone()),
            Transform::from_translation(LAMP_DROP),
            NotShadowCaster,
            Visibility::Hidden,
            ChildOf(lamp),
        ))
        .id();
    let light = kit.lamps_cast_light.then(|| {
        commands
            .spawn((
                PointLight {
                    intensity: 0.0,
                    range: 5.0,
                    radius: 0.05,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_translation(
                    LAMP_DROP + Vec3::new(0.0, 0.0, 0.12),
                ),
                ChildOf(lamp),
            ))
            .id()
    });
    let prop = commands
        .spawn((
            Mesh3d(kit.props[0].clone()),
            MeshMaterial3d(kit.matte.clone()),
            Visibility::Hidden,
            ChildOf(arms[0]),
        ))
        .id();
    let glint = commands
        .spawn((
            Mesh3d::default(),
            MeshMaterial3d(lit.clone()),
            NotShadowCaster,
            ChildOf(prop),
        ))
        .id();

    commands.entity(root).try_insert(Keeper {
        state,
        who,
        quirk: Quirk::of(who.seed),
        act: if born.is_some() {
            Act::Appear
        } else {
            Act::of(state)
        },
        since: born.unwrap_or(LONG_AGO),
        from: None,
        synced: state,
        gait: 0.0,
        pace: 0.0,
        roll: 0.0,
        seen: f32::NAN,
        shown: Prop::None,
        lit_as: Vec3::splat(-1.0),
        props: kit.props.clone(),
        glints: kit.glints.clone(),
        strode: None,
        look: Vec2::ZERO,
        parts: Parts {
            rig,
            body,
            head,
            hat,
            eye_l: eyes[0],
            eye_r: eyes[1],
            arm_l: arms[0],
            arm_r: arms[1],
            leg_l: legs[0],
            leg_r: legs[1],
            wheel_l: wheels[0],
            wheel_r: wheels[1],
            pole,
            lamp,
            ring,
            light,
            prop,
            glint,
            key,
            flag,
            lit,
            shapes: std::mem::take(shapes),
        },
    });
    root
}
