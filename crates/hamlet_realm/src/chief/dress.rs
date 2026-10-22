//! The meshes and lights that follow a chief's look and the hour, and
//! the pose of its mascot.

use bevy::prelude::*;

use super::mascot::{self, Pose, Rig, Stance};
use super::{
    ChiefSettings, Hat, Look, Mascot, Phase, Place, Settled, place,
};
use crate::generator::glow;
use crate::generator::land::Land;
use crate::meshes::smoothstep;
use crate::palette::Palette;
use crate::{Clock, Sky};

/// A light of its own colour that no shading dims.
pub(super) fn lit(colour: Vec3, strength: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::LinearRgba(LinearRgba::from_vec3(
            colour * strength,
        )),
        unlit: true,
        fog_enabled: false,
        ..default()
    }
}

/// Builds the meshes that follow a chief's look, when it is new and
/// when it changes: the mascot, and the roof and the letterbox at
/// home.
pub(super) fn dress(
    mut commands: Commands,
    land: Res<Land>,
    set: Res<ChiefSettings>,
    pal: Res<Palette>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    chiefs: Query<
        (&Look, &Settled),
        Or<(Changed<Look>, Added<Settled>)>,
    >,
    places: Query<(&Place, &Transform)>,
    mascots: Query<(&Mascot, &Rig)>,
    mut lamps: Query<&mut PointLight>,
) {
    for (look, settled) in &chiefs {
        let light = look.light.colour(&pal);
        if let Ok((place, frame)) = places.get(settled.place) {
            let floor = frame.translation.y;
            let built = place::build(&pal, &set, *look, |local| {
                let at = frame.transform_point(Vec3::new(
                    local.x, 0.0, local.y,
                ));
                land.surface(at.xz()) - floor
            });
            commands
                .entity(settled.place)
                .insert(Mesh3d(meshes.add(built.body.build())));
            commands
                .entity(place.panes)
                .insert(Mesh3d(meshes.add(built.panes.build())));
            if let Some(mut orb) = materials.get_mut(&place.orb) {
                *orb = lit(light, set.orb_light);
            }
            if let Some(mut ring) = materials.get_mut(&place.ring) {
                *ring = glow(light, set.waystone_light);
            }
        }
        let Ok((mascot, rig)) = mascots.get(settled.mascot) else {
            continue;
        };
        let handles = mascot::parts(&pal, *look)
            .each()
            .map(|mb| meshes.add(mb.build()));
        for (entity, which) in rig.dressed() {
            commands
                .entity(entity)
                .insert(Mesh3d(handles[which].clone()));
        }
        if let Some(mut flame) = materials.get_mut(&mascot.flame) {
            *flame = lit(light, set.wick_light);
        }
        if let Ok(mut lamp) = lamps.get_mut(rig.lamp) {
            lamp.color = Color::linear_rgb(light.x, light.y, light.z);
            lamp.range = set.lamp_range;
        }
    }
}

/// Has every mascot take the pose of what it is doing.
pub(super) fn animate(
    clock: Res<Clock>,
    set: Res<ChiefSettings>,
    looks: Query<&Look>,
    mut mascots: Query<(&mut Mascot, &mut Stance)>,
) {
    let now = clock.t;
    for (mut mascot, mut stance) in &mut mascots {
        let hat = looks
            .get(mascot.chief)
            .map_or(Hat::default(), |look| look.hat);
        let to = Pose::of(mascot.doing, now);
        let k = smoothstep(0.0, set.blend, now - mascot.changed);
        let pose = mascot.from.toward(&to, k);
        if mascot.shown != pose {
            mascot.shown = pose;
        }
        let to = Stance {
            pose: pose.stepping(mascot.phase, now),
            size: set.size,
            crest: mascot::crest(hat),
        };
        stance.set_if_neq(to);
    }
}

/// Lights the wick's lamp and the gate's lantern by the hour, and
/// flares the lamp as a mascot arrives.
pub(super) fn light(
    clock: Res<Clock>,
    sky: Res<Sky>,
    set: Res<ChiefSettings>,
    pal: Res<Palette>,
    places: Query<&Place>,
    mascots: Query<(&Mascot, &Rig)>,
    mut lamps: Query<&mut PointLight>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let candle = sky.look.candle;
    if sky.is_changed() {
        for place in &places {
            let Some(mut glass) = materials.get_mut(&place.glass)
            else {
                continue;
            };
            let warm = pal.wicks[0] * set.lantern_light * candle;
            glass.emissive = LinearRgba::rgb(warm.x, warm.y, warm.z);
        }
    }
    for (mascot, rig) in &mascots {
        let Ok(mut lamp) = lamps.get_mut(rig.lamp) else {
            continue;
        };
        let flare = match mascot.phase {
            Phase::In { began, long } => {
                let u = ((clock.t - began) / long).clamp(0.0, 1.0);
                1.0 + set.flare * (1.0 - smoothstep(0.1, 0.7, u))
            }
            Phase::Dark { .. } => 0.0,
            _ => 1.0,
        };
        let lumens = set.lamp * (0.25 + 0.75 * candle) * flare;
        if lamp.intensity != lumens {
            lamp.intensity = lumens;
        }
    }
}
