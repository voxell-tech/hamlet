//! A robot put into the world, part by part.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::acts::{Act, Quirk};
use super::kit::Kit;
use super::kit::body::{KEY_AT, LAMP_DROP};
use super::kit::hats::SPIRE;
use super::pose::{
    FACE, HAT_AT, HIP, HOOK, NECK, Pose, Prop, SHOULDER,
};
use super::{KState, Keeper, LONG_AGO, Parts, Who};

/// A pivot carrying a part's meshes. The first mesh rides on the
/// pivot itself.
fn part(
    commands: &mut Commands,
    parent: Entity,
    at: Vec3,
    meshes: [(&Option<Handle<Mesh>>, &Handle<StandardMaterial>); 2],
) -> Entity {
    let pivot = commands
        .spawn((
            Transform::from_translation(at),
            Visibility::Inherited,
            ChildOf(parent),
        ))
        .id();
    let mut on_pivot = false;
    for (mesh, mat) in meshes {
        let Some(mesh) = mesh else { continue };
        let bundle =
            (Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()));
        if on_pivot {
            commands.spawn((bundle, ChildOf(pivot)));
        } else {
            commands.entity(pivot).try_insert(bundle);
            on_pivot = true;
        }
    }
    pivot
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
    let paint = &kit.paints[who.paint as usize];
    let body = part(
        commands,
        rig,
        Vec3::Y * HIP,
        [(&kit.torso.0, paint), (&kit.torso.1, &kit.brass)],
    );
    let legs = [-0.11f32, 0.11].map(|x| {
        part(
            commands,
            rig,
            Vec3::new(x, HIP, 0.0),
            [(&kit.leg.0, &kit.matte), (&kit.leg.1, &kit.brass)],
        )
    });
    let wheels = legs.map(|leg| {
        commands
            .spawn((
                Mesh3d(kit.wheels.clone()),
                MeshMaterial3d(kit.brass.clone()),
                Transform::from_translation(
                    Pose::stand().place(1.0).axle,
                ),
                ChildOf(leg),
            ))
            .id()
    });
    let arms = [-1.0f32, 1.0].map(|side| {
        part(
            commands,
            body,
            SHOULDER * Vec3::new(side, 1.0, 1.0),
            [(&kit.arm.0, &kit.matte), (&kit.arm.1, &kit.brass)],
        )
    });
    commands.spawn((
        Mesh3d(kit.emblems[who.emblem as usize].clone()),
        MeshMaterial3d(kit.matte.clone()),
        NotShadowCaster,
        ChildOf(body),
    ));
    let key = commands
        .spawn((
            Mesh3d(kit.key.clone()),
            MeshMaterial3d(kit.brass.clone()),
            Transform::from_translation(KEY_AT),
            ChildOf(body),
        ))
        .id();
    if who.leader {
        commands.spawn((
            Mesh3d(kit.mantle.clone()),
            MeshMaterial3d(kit.matte.clone()),
            ChildOf(body),
        ));
    }

    let head = commands
        .spawn((
            Mesh3d(kit.head.clone()),
            MeshMaterial3d(kit.brass.clone()),
            Transform::from_translation(Vec3::Y * NECK),
            ChildOf(body),
        ))
        .id();
    // One material for both lanterns and the ring, so they show the
    // same state.
    let lit = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.19, 0.17),
        perceptual_roughness: 0.4,
        ..default()
    });
    commands.spawn((
        Mesh3d(kit.glass.clone()),
        MeshMaterial3d(lit.clone()),
        Transform::from_translation(Vec3::Y * FACE),
        NotShadowCaster,
        ChildOf(head),
    ));
    let eyes = [(); 2].map(|_| {
        commands
            .spawn((
                Mesh3d(kit.eye.clone()),
                MeshMaterial3d(kit.matte.clone()),
                Transform::from_translation(Vec3::Y * FACE),
                NotShadowCaster,
                ChildOf(head),
            ))
            .id()
    });
    let worn = &kit.hats[who.hat as usize];
    let hat = part(
        commands,
        head,
        Vec3::Y * HAT_AT,
        [(&worn.0, paint), (&worn.1, &kit.brass)],
    );
    let flag = who.leader.then(|| {
        commands
            .spawn((
                Mesh3d(kit.pennant.clone()),
                MeshMaterial3d(kit.matte.clone()),
                Transform::from_translation(Vec3::Y * (SPIRE - 0.02)),
                ChildOf(hat),
            ))
            .id()
    });

    let pole = part(
        commands,
        rig,
        Pose::stand().pole_at,
        [(&kit.pole.0, &kit.matte), (&kit.pole.1, &kit.brass)],
    );
    let lamp = commands
        .spawn((
            Mesh3d(kit.lamp.clone()),
            MeshMaterial3d(kit.brass.clone()),
            Transform::from_translation(HOOK),
            ChildOf(pole),
        ))
        .id();
    commands.spawn((
        Mesh3d(kit.lamp_glass.clone()),
        MeshMaterial3d(lit.clone()),
        Transform::from_translation(LAMP_DROP),
        NotShadowCaster,
        ChildOf(lamp),
    ));
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
        },
    });
    root
}
