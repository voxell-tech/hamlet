//! The circle of runes and the motes at each end of a Wickstep.

use std::f32::consts::{PI, TAU};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::RequestRedraw;
use hamlet_vfx::magic::chance;
use hamlet_vfx::mark::{MarkMaterial, Marks, Shape, lie};

use super::Mascot;
use crate::Clock;
use crate::chief::ChiefSettings;
use crate::generator::Floor;
use crate::meshes::smoothstep;
use crate::sky::Quality;

/// The meshes of a Wickstep's rune circle and motes.
#[derive(Resource)]
pub(in crate::chief) struct Wand {
    mote: Handle<Mesh>,
}

/// One ring of a Wickstep's circle of runes.
#[derive(Component)]
pub(in crate::chief) struct Rune {
    began: f32,
    long: f32,
    turn: f32,
    /// True when it opens, as on arriving, or closes.
    opens: bool,
    light: Vec3,
    at: Vec3,
    fill: Handle<MarkMaterial>,
}

/// A mote of a Wickstep.
#[derive(Component)]
pub(in crate::chief) struct Mote {
    began: f32,
    from: Vec3,
    own: f32,
    /// True when it flies out from an arrival, or is drawn up after
    /// a departure.
    bursts: bool,
}

pub(in crate::chief) fn prepare(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    commands.insert_resource(Wand {
        mote: meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap()),
    });
}

/// The circle and the motes of one end of a Wickstep, at `ground`.
pub(super) fn charm(
    commands: &mut Commands,
    wand: &Wand,
    materials: &mut Assets<MarkMaterial>,
    marks: &Marks,
    floor: &mut Floor,
    set: &ChiefSettings,
    quality: Quality,
    mascot: &Mascot,
    light: Vec3,
    ground: Vec3,
    now: f32,
    arrives: bool,
) {
    let long = if arrives {
        set.in_time * 1.5
    } else {
        set.out_time
    };
    let at = ground + Vec3::Y * set.rune_lift;
    let lay = floor.under(ground, set.rune_radius * 1.1);
    for (shape, turn) in [(Shape::RING, 1.2), (Shape::DIAMOND, -2.0)]
    {
        let fill =
            materials.add(marks.shine(shape, Vec3::ZERO, &lay));
        commands.spawn((
            Rune {
                began: now,
                long,
                turn,
                opens: arrives,
                light: light * set.rune_light,
                at,
                fill: fill.clone(),
            },
            marks.mark(fill),
            lie(at, 0.0, 0.0),
        ));
    }
    let count = (set.motes as f32 * quality.density()).round() as u32;
    for i in 0..count {
        let own = i as f32 + chance(now + i as f32);
        let around = chance(own) * TAU;
        let out = chance(own + 0.5).sqrt() * set.rune_radius * 0.55;
        let from = ground
            + Vec3::new(around.cos(), 0.0, around.sin()) * out
            + Vec3::Y * (0.15 + chance(own + 0.8));
        commands.spawn((
            Mote {
                began: now,
                from,
                own,
                bursts: arrives,
            },
            Mesh3d(wand.mote.clone()),
            MeshMaterial3d(mascot.flame.clone()),
            Transform::from_translation(from).with_scale(Vec3::ZERO),
            NotShadowCaster,
        ));
    }
}

/// Plays the circles and the motes of every Wickstep, and clears
/// each away at its end.
pub(in crate::chief) fn cast(
    mut commands: Commands,
    clock: Res<Clock>,
    set: Res<ChiefSettings>,
    mut runes: Query<(Entity, &Rune, &mut Transform), Without<Mote>>,
    mut motes: Query<(Entity, &Mote, &mut Transform), Without<Rune>>,
    mut materials: ResMut<Assets<MarkMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if runes.is_empty() && motes.is_empty() {
        return;
    }
    redraw.write(RequestRedraw);
    for (entity, rune, mut at) in &mut runes {
        let u = (clock.t - rune.began) / rune.long;
        if !(0.0..1.0).contains(&u) {
            if u >= 1.0 {
                // The heights it lay on go with it.
                if let Some(fill) = materials.remove(&rune.fill) {
                    images.remove(fill.heights());
                }
                commands.entity(entity).try_despawn();
            }
            continue;
        }
        let (reach, fade) = if rune.opens {
            // It springs open under the feet and fades as they
            // settle.
            let open = 1.0 - (1.0 - (u * 2.6).min(1.0)).powi(3);
            (0.3 + 0.7 * open, (1.0 - u).powf(1.5))
        } else {
            // It closes on the place that is left.
            (
                1.0 - 0.78 * smoothstep(0.2, 1.0, u),
                1.0 - smoothstep(0.8, 1.0, u),
            )
        };
        *at = lie(rune.at, set.rune_radius * reach, u * rune.turn);
        if let Some(mut fill) = materials.get_mut(&rune.fill) {
            fill.light(rune.light * fade * (u * 8.0).min(1.0));
        }
    }
    for (entity, mote, mut at) in &mut motes {
        // Each sets out a little after the last.
        let wait = chance(mote.own + 0.25) * set.mote_time * 0.25;
        let u =
            (clock.t - mote.began - wait) / (set.mote_time - wait);
        if u >= 1.0 {
            commands.entity(entity).try_despawn();
            continue;
        }
        if u < 0.0 {
            continue;
        }
        let own = chance(mote.own + 0.75);
        let around = chance(mote.own) * TAU;
        let out = Vec3::new(around.cos(), 0.0, around.sin());
        let eased = 1.0 - (1.0 - u).powi(2);
        if mote.bursts {
            // Out from the feet and up, like sparks off a wick that
            // catches.
            at.translation = mote.from
                + out * set.mote_spread * (0.4 + 0.6 * own) * eased
                + Vec3::Y * set.mote_rise * 0.6 * own * eased;
            at.scale = Vec3::splat(
                set.mote_size * (u * 8.0).min(1.0) * (1.0 - u),
            );
        } else {
            // Drawn in and up after the flame, like its smoke.
            let sway = Vec3::new(
                (u * 5.0 + mote.own).sin(),
                0.0,
                (u * 4.0 + mote.own * 1.7).cos(),
            ) * 0.18;
            at.translation = mote.from - out * 0.3 * u
                + Vec3::Y * set.mote_rise * (0.5 + 0.5 * own) * u * u
                + sway * u;
            at.scale = Vec3::splat(
                set.mote_size * (u * PI).sin().max(0.0).sqrt() * 0.8,
            );
        }
    }
}
