//! The light of the grooves and of the door lanterns.

use std::collections::HashMap;

use bevy::pbr::ExtendedMaterial;
use bevy::prelude::*;
use hamlet_vfx::inlay::{
    self, InlayExt, InlayMaterial, InlayUniform, LIT,
};

use super::land::Land;
use super::society::{Pair, Society};
use super::sync::Drawn;
use super::traffic::Trips;
use super::{Kit, state_index};
use crate::keeper::KeeperState;
use crate::palette::Palette;
use crate::sky::{Looks, SkySettings};
use crate::{Clock, Sky};

/// Lights the lamps and spills that are added.
pub(super) fn plugin(app: &mut App) {
    app.add_observer(show_lamp).add_observer(show_spill);
}

/// Lights the grooves that have something to show: a mote under a
/// walking robot, orange at the door of an agent that waits, red
/// where one has faulted. Every other groove shares one unlit
/// material.
pub(super) fn light_roads(
    mut commands: Commands,
    clock: Res<Clock>,
    sky: Res<Sky>,
    looks: Res<Looks>,
    land: Res<Land>,
    society: Res<Society>,
    traffic: Res<Trips>,
    kit: Res<Kit>,
    pal: Res<Palette>,
    drawn: Res<Drawn>,
    mut inlays: ResMut<Assets<InlayMaterial>>,
    mut wanted: Local<HashMap<Pair, InlayUniform>>,
    // The material of each groove that is lit, and the groove that
    // wears it.
    mut lit: Local<HashMap<Pair, (Handle<InlayMaterial>, Entity)>>,
) {
    let gain = sky.look.inlay_gain;
    let (strip, half_life) =
        (looks.strip_share, looks.trace_half_life);
    let speed = land.set.road.walk_speed;
    wanted.clear();
    for agent in society.agents.iter().filter(|a| !a.archived) {
        let (hue, mode, reach) = match agent.state {
            KeeperState::Waiting => (inlay::signal(), 3.0, 4.5),
            KeeperState::Fault => (inlay::fault(), 4.0, 7.0),
            _ => continue,
        };
        let Some(town) = society.town(agent.town) else {
            continue;
        };
        let pair = Pair::new(agent.id, town.mayor);
        let Some(road) = drawn.roads.get(&pair) else {
            continue;
        };
        let total = road.route.total();
        let range = if road.first == agent.id {
            (-1.0, reach)
        } else {
            (total - reach, total + 1.0)
        };
        wanted.insert(
            pair,
            InlayUniform {
                hue: hue.extend(LIT * gain),
                range: Vec4::new(range.0, range.1, 0.0, total),
                anim: Vec4::new(mode, clock.t, 0.0, half_life),
                shape: Vec4::new(strip, 0.0, speed, 0.0),
            },
        );
    }
    for trip in &traffic.trips {
        let Some(road) = drawn.roads.get(&trip.pair) else {
            continue;
        };
        let total = road.route.total();
        let forward = (road.first == trip.walker) != trip.back;
        let mote = if road.first == trip.walker {
            trip.gone
        } else {
            total - trip.gone
        };
        // The shader leaves a trace behind a mote that runs up the
        // line. One running down it shows only its head.
        let from = if forward { 0.0 } else { total + 1.0 };
        wanted.insert(
            trip.pair,
            InlayUniform {
                hue: inlay::current().extend(LIT * gain),
                range: Vec4::new(0.0, total, 0.0, total + 2.0),
                anim: Vec4::new(1.0, clock.t, mote, half_life),
                shape: Vec4::new(strip, 0.0, speed, from),
            },
        );
    }
    lit.retain(|pair, (_, groove)| {
        if wanted.contains_key(pair) {
            return true;
        }
        // A road laid anew has an unlit groove already.
        if drawn.roads.get(pair).is_some_and(|r| r.groove == *groove)
        {
            commands
                .entity(*groove)
                .try_insert(MeshMaterial3d(kit.groove.clone()));
        }
        false
    });
    for (pair, u) in wanted.drain() {
        let Some(road) = drawn.roads.get(&pair) else {
            continue;
        };
        let Some((handle, groove)) = lit.get_mut(&pair) else {
            let handle = inlays.add(ExtendedMaterial {
                base: StandardMaterial {
                    perceptual_roughness: 0.85,
                    ..default()
                },
                extension: InlayExt::new(u, pal.bed, pal.pewter),
            });
            commands
                .entity(road.groove)
                .try_insert(MeshMaterial3d(handle.clone()));
            lit.insert(pair, (handle, road.groove));
            continue;
        };
        if let Some(mut m) = inlays.get_mut(&*handle)
            && m.extension.u != u
        {
            m.extension.u = u;
        }
        if *groove != road.groove {
            *groove = road.groove;
            commands
                .entity(road.groove)
                .try_insert(MeshMaterial3d(handle.clone()));
        }
    }
}

/// A door lantern's hue and strength for the state of the agent
/// inside.
fn lantern(state: KeeperState, t: f32) -> (Vec3, f32) {
    match state {
        KeeperState::Rest => (inlay::candle(), inlay::EMBER * 1.4),
        KeeperState::Working => (inlay::current(), LIT),
        KeeperState::Waiting => (
            inlay::signal(),
            LIT * (0.8 + 0.2 * (t * std::f32::consts::PI).sin()),
        ),
        KeeperState::Fault => {
            let tick = (t * 2.6).floor();
            let drop =
                ((tick * 12.9898).sin() * 43758.547).fract().abs()
                    < 0.4;
            (inlay::fault(), if drop { LIT * 0.08 } else { LIT })
        }
        KeeperState::Failed => (Vec3::ZERO, 0.0),
    }
}

/// A lantern that shows an agent's state.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Lamp(pub(super) KeeperState);

/// The light a lantern throws on the ground after dark. A child of
/// its lantern.
#[derive(Component)]
pub(super) struct Spill;

impl Spill {
    pub(super) fn bundle(set: &SkySettings) -> impl Bundle {
        (
            Spill,
            PointLight {
                intensity: 0.0,
                range: set.door_reach,
                shadow_maps_enabled: false,
                ..default()
            },
            Visibility::Hidden,
        )
    }
}

fn spill_colour(state: KeeperState) -> Color {
    let hue = lantern(state, 0.0).0;
    Color::linear_rgb(hue.x, hue.y, hue.z)
}

/// Puts a lantern's state in its glass and in the light it throws.
pub(super) fn show_lamp(
    lamp: On<Insert, Lamp>,
    mut commands: Commands,
    kit: Res<Kit>,
    lamps: Query<(&Lamp, Option<&Children>)>,
    mut spills: Query<&mut PointLight, With<Spill>>,
) {
    let Ok((Lamp(state), children)) = lamps.get(lamp.entity) else {
        return;
    };
    commands.entity(lamp.entity).try_insert(MeshMaterial3d(
        kit.lamps[state_index(*state)].clone(),
    ));
    for child in children.into_iter().flatten() {
        if let Ok(mut light) = spills.get_mut(*child) {
            light.color = spill_colour(*state);
        }
    }
}

/// A new lantern's light takes the colour of its lantern and the
/// strength of the hour.
pub(super) fn show_spill(
    spill: On<Add, Spill>,
    sky: Res<Sky>,
    set: Res<SkySettings>,
    lamps: Query<&Lamp>,
    mut spills: Query<
        (&mut PointLight, &mut Visibility, &ChildOf),
        With<Spill>,
    >,
) {
    let Ok((mut light, mut seen, lamp)) =
        spills.get_mut(spill.entity)
    else {
        return;
    };
    let state = lamps
        .get(lamp.parent())
        .map_or(KeeperState::Rest, |lamp| lamp.0);
    light.color = spill_colour(state);
    light.intensity = set.door_lumens * sky.look.candle;
    if light.intensity > 0.0 {
        *seen = Visibility::Inherited;
    }
}

/// Lanterns throw light as strongly as the windows burn, and none
/// by day.
pub(super) fn light_spills(
    sky: Res<Sky>,
    set: Res<SkySettings>,
    mut spills: Query<
        (&mut PointLight, &mut Visibility),
        With<Spill>,
    >,
) {
    let lumens = set.door_lumens * sky.look.candle;
    let want = if lumens > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for (mut light, mut seen) in &mut spills {
        light.intensity = lumens;
        light.range = set.door_reach;
        if *seen != want {
            *seen = want;
        }
    }
}

/// The door lanterns: one shared material per state, whatever the
/// number of houses.
pub(super) fn light_lanterns(
    clock: Res<Clock>,
    sky: Res<Sky>,
    kit: Res<Kit>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let look = &sky.look;
    for (state, handle) in
        KeeperState::ALL.into_iter().zip(&kit.lamps)
    {
        let (hue, strength) = lantern(state, clock.t);
        // A lantern at rest is a candle: lit only after dark.
        let strength = if state == KeeperState::Rest {
            strength * look.candle
        } else {
            strength
        };
        let glow = hue * strength;
        let glow = LinearRgba::rgb(glow.x, glow.y, glow.z);
        // A write marks the material modified, and every lantern
        // that wears it is prepared again.
        if let Some(mut m) = materials.get_mut(handle)
            && m.emissive != glow
        {
            m.emissive = glow;
        }
    }
}
