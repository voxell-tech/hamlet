//! The budget where it can be seen: a light meter on each common.

use bevy::prelude::*;
use bevy::window::RequestRedraw;
use hamlet_client::realm::Treasury;

pub(super) mod gauge;

use self::gauge::{COLUMN, Gauge};
use crate::generator::Common;
use crate::generator::land::Land;
use crate::hamlet::Sited;
use crate::hand::PlaySettings;
use crate::label::{self, Lens};
use crate::{Clock, Sky};

/// On a common that has its meter.
#[derive(Component)]
pub(super) struct Metered;

/// The light in a hamlet's meter.
#[derive(Component)]
pub(super) struct Level {
    hamlet: Entity,
}

/// The pointer of a hamlet's meter.
#[derive(Component)]
pub(super) struct Pace {
    hamlet: Entity,
}

/// The stand of a hamlet's meter.
#[derive(Component)]
pub(super) struct Meter {
    hamlet: Entity,
}

/// The rest of a hamlet's budget, written over its meter.
#[derive(Component)]
pub struct Readout {
    of: Entity,
}

/// The words of a readout.
#[derive(Component)]
pub(super) struct ReadoutText;

/// Keeps what is left of each hamlet's budget written over its
/// meter, for as long as the camera is near enough to read it.
pub(super) fn readouts(
    mut commands: Commands,
    set: Res<PlaySettings>,
    lens: Lens,
    meters: Query<(Entity, &Meter, &GlobalTransform)>,
    hamlets: Query<&Treasury>,
    mut shown: Query<(
        Entity,
        &Readout,
        &mut Node,
        &mut Visibility,
        &ComputedNode,
        &Children,
    )>,
    mut texts: Query<&mut Text, With<ReadoutText>>,
) {
    let (camera, view) = lens.camera();
    let (foot, tall) = COLUMN;
    for (meter, ..) in &meters {
        if !shown.iter().any(|(_, readout, ..)| readout.of == meter) {
            commands.spawn((
                Readout { of: meter },
                Node {
                    position_type: PositionType::Absolute,
                    padding: UiRect::axes(px(6), px(1)),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(Color::srgba_u8(
                    0x01, 0x0a, 0x13, 0xd8,
                )),
                BorderColor::all(Color::srgb_u8(0x7c, 0x5d, 0x2f)),
                Pickable::IGNORE,
                Visibility::Hidden,
                children![(
                    ReadoutText,
                    Text::new(""),
                    TextFont::from_font_size(11.0),
                    TextColor(Color::srgb_u8(0xf2, 0xe7, 0xd0)),
                    Pickable::IGNORE,
                )],
            ));
        }
    }
    for (entity, readout, mut node, mut seen, computed, parts) in
        &mut shown
    {
        let Ok((_, meter, was)) = meters.get(readout.of) else {
            commands.entity(entity).try_despawn();
            continue;
        };
        let at = lens.stands(readout.of, was);
        // Over the cap of the meter.
        let top = at.transform_point(Vec3::Y * (foot + tall + 0.8));
        let words =
            hamlets.get(meter.hamlet).ok().and_then(|treasury| {
                let report = &treasury.0;
                let left = report.remains.as_deref()?;
                Some(format!(
                    "{left} LEFT {}",
                    report.period_words.to_uppercase()
                ))
            });
        let point = words
            .as_ref()
            .filter(|_| {
                view.translation().distance(top) < set.meter_read_far
            })
            .and_then(|_| camera.world_to_viewport(&view, top).ok());
        let (Some(words), Some(point)) = (words, point) else {
            seen.set_if_neq(Visibility::Hidden);
            continue;
        };
        for part in parts.iter() {
            if let Ok(mut text) = texts.get_mut(part)
                && text.0 != words
            {
                text.0.clone_from(&words);
            }
        }
        label::put(&mut node, label::above(computed, point, 0.0));
        seen.set_if_neq(Visibility::Visible);
    }
}

/// The light a meter throws on what is about it, dark until it is
/// told how bright.
fn lamp(reach: f32) -> PointLight {
    PointLight {
        intensity: 0.0,
        range: reach,
        shadow_maps_enabled: false,
        ..default()
    }
}

/// Sets what a meter's lamp throws.
fn throw(lamp: &mut PointLight, colour: Color, lumens: f32) {
    if lamp.color != colour {
        lamp.color = colour;
    }
    if lamp.intensity != lumens {
        lamp.intensity = lumens;
    }
}

/// True when a light that blinks is lit just now. It is while the
/// clock is stopped, so a still shows it.
fn blinks(clock: &Clock, set: &PlaySettings) -> bool {
    clock.frozen || (clock.t * set.meter_blink).fract() < 0.5
}

/// Stands a meter on every new common, and keeps each
/// showing its hamlet's budget.
pub(super) fn meters(
    mut commands: Commands,
    gauge: Option<Res<Gauge>>,
    set: Res<PlaySettings>,
    land: Res<Land>,
    commons: Query<(Entity, &Common), Without<Metered>>,
    hamlets: Query<(Entity, &Sited, Option<&Treasury>)>,
    mut levels: Query<
        (
            &Level,
            &mut Transform,
            &mut Visibility,
            &mut MeshMaterial3d<StandardMaterial>,
            &mut PointLight,
        ),
        Without<Pace>,
    >,
    mut paces: Query<(&Pace, &mut Transform), Without<Level>>,
    clock: Res<Clock>,
    sky: Res<Sky>,
    mut redraw: Option<ResMut<Messages<RequestRedraw>>>,
) {
    let Some(gauge) = gauge else {
        return;
    };
    let (foot, tall) = COLUMN;
    for (common, of) in &commons {
        let Some((hamlet, ..)) =
            hamlets.iter().find(|(_, sited, _)| sited.0 == of.town)
        else {
            continue;
        };
        let town = &land.set.town;
        let at = set.meter_at;
        commands.entity(common).try_insert(Metered).with_child((
            Meter { hamlet },
            Mesh3d(gauge.meter.clone()),
            MeshMaterial3d(gauge.matte.clone()),
            Transform::from_xyz(at.x, town.plaza_lift, at.y)
                .with_rotation(Quat::from_rotation_y(0.5))
                .with_scale(Vec3::splat(set.meter_size)),
            children![
                (
                    Level { hamlet },
                    Mesh3d(gauge.level.clone()),
                    MeshMaterial3d(gauge.light(1.0)),
                    Transform::from_xyz(0.0, foot, 0.0),
                    Visibility::Inherited,
                    bevy::light::NotShadowCaster,
                    lamp(set.meter_reach),
                ),
                (
                    Pace { hamlet },
                    Mesh3d(gauge.pace.clone()),
                    MeshMaterial3d(gauge.brass.clone()),
                    Transform::from_xyz(0.0, foot + tall, 0.0),
                ),
            ],
        ));
    }
    let budget = |hamlet: Entity| {
        let report = &hamlets.get(hamlet).ok()?.2?.0;
        Some((report.left?, report.gone as f32))
    };
    let lit = blinks(&clock, &set);
    let dark = sky.look.candle;
    for (level, mut at, mut seen, mut light, mut thrown) in
        &mut levels
    {
        // A budget with no cap is whole.
        let left =
            budget(level.hamlet).map_or(1.0, |(left, _)| left as f32);
        // Spent, the whole of it blinks red: an alarm, where a
        // dark meter would say nothing.
        let empty = left <= 0.0;
        let want = if !empty || lit {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        seen.set_if_neq(want);
        if empty && let Some(redraw) = &mut redraw {
            redraw.write(RequestRedraw);
        }
        // The light sinks as the budget is spent.
        let tall = if empty { 1.0 } else { left.max(0.02) };
        let scale = Vec3::new(1.0, tall, 1.0);
        if at.scale != scale {
            at.scale = scale;
        }
        let glow = gauge.light(left);
        if light.0 != glow {
            light.0 = glow;
        }
        // It throws its colour on the common after dark, and
        // nothing while it is blinked out.
        let lumens = if want == Visibility::Hidden {
            0.0
        } else {
            set.meter_lumens * dark
        };
        throw(&mut thrown, gauge.tint(left), lumens);
    }
    for (pace, mut at) in &mut paces {
        let gone = budget(pace.hamlet).map_or(0.0, |(_, gone)| gone);
        let y = foot + tall * (1.0 - gone);
        if at.translation.y != y {
            at.translation.y = y;
        }
    }
}
