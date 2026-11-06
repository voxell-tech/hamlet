//! The city island's own buildings: the Hand's tower at its middle,
//! and the houses of the scholars about it, with the scholars that
//! wait on its pier for a house. The Hearth and the library stand
//! beside them on ground of their own.

pub(crate) mod houses;
mod newcomers;
#[cfg(test)]
mod tests;
mod tower;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::RequestRedraw;
use hamlet_client::realm::{Busy, HandId, Identity, Left, Scholar};

use crate::generator::land::{IslandId, Land};
use crate::generator::society::{Claim, Ground, Society};
use crate::library::Raising;
use crate::palette::Palette;
use crate::pick::{self, Footprint, Named, Thing, hit_box};
use crate::{Clock, RealmSystems, Sky, Stock};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, glass).add_systems(
        Update,
        (
            raise,
            houses::claim,
            houses::tend,
            newcomers::wait,
            newcomers::arrive,
            shape,
            shine,
        )
            .chain()
            .after(Raising)
            .in_set(RealmSystems::Act),
    );
}

/// A round of ground in the city that is kept level and clear, as
/// the ground of a town is: its middle and its radius.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Level(pub Vec2, pub f32);

/// The ground of the tower and of every house, for [`Land::hold`].
pub(crate) fn rounds(levels: &Query<&Level>) -> Vec<(Vec2, f32)> {
    levels.iter().map(|level| (level.0, level.1)).collect()
}

/// The Hand's tower.
#[derive(Component, Clone, Copy, Debug)]
pub struct Tower;

/// The light on the tower's peak, which burns while the Hand works.
#[derive(Component)]
struct Lamp;

/// The glass of the city's buildings, each made once.
#[derive(Resource, Clone)]
struct Glass {
    /// The lit windows of the scholars' houses.
    windows: Handle<StandardMaterial>,
    /// The windows of a house nobody is in.
    dark: Handle<StandardMaterial>,
    /// The shutters of a house its scholar has left.
    shut: Handle<StandardMaterial>,
    /// The tower's windows and lantern.
    lamp: Handle<StandardMaterial>,
}

fn glass(
    mut commands: Commands,
    pal: Res<Palette>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let lit = StandardMaterial {
        base_color: pal.glass_lit,
        ..default()
    };
    let shutters = pal.timber_dark;
    commands.insert_resource(Glass {
        windows: materials.add(lit.clone()),
        lamp: materials.add(lit),
        dark: materials.add(StandardMaterial {
            base_color: pal.pane_dark,
            perceptual_roughness: 0.3,
            ..default()
        }),
        shut: materials.add(StandardMaterial {
            base_color: Color::linear_rgb(
                shutters.x, shutters.y, shutters.z,
            ),
            perceptual_roughness: 0.92,
            ..default()
        }),
    });
}

/// Stands the Hand's tower at the middle of the city, its door
/// toward the other island.
fn raise(
    mut commands: Commands,
    land: Res<Land>,
    pal: Res<Palette>,
    stock: Res<Stock>,
    glass: Res<Glass>,
    mut society: ResMut<Society>,
    mut meshes: ResMut<Assets<Mesh>>,
    towers: Query<(), With<Tower>>,
) {
    if !towers.is_empty() {
        return;
    }
    let at = land.island(IslandId::City).centre;
    society.claims.push(Claim {
        place: Ground::Tower,
        at,
        radius: tower::GROUNDS + 3.0,
    });
    let toward = land.bearing();
    let (walls, glazing) = tower::build(&pal);
    let tower = commands
        .spawn((
            Tower,
            Level(at, tower::GROUNDS),
            Mesh3d(meshes.add(walls)),
            MeshMaterial3d(stock.matte.clone()),
            Transform::from_translation(land.on_ground(at))
                .with_rotation(Quat::from_rotation_y(
                    toward.x.atan2(toward.y),
                )),
            Visibility::Inherited,
            children![(
                Mesh3d(meshes.add(glazing)),
                MeshMaterial3d(glass.lamp.clone()),
                NotShadowCaster,
            )],
        ))
        .id();
    commands.spawn((
        Lamp,
        PointLight {
            color: pal.glass_lit,
            intensity: 0.0,
            range: LAMP_REACH,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, tower::LANTERN, 0.0),
        ChildOf(tower),
    ));
    let (wide, tall) = (tower::FOOT + 1.2, tower::HEIGHT + 2.0);
    let boxed = commands
        .spawn((
            hit_box(
                Thing::Hand,
                Vec3::new(wide * 2.0, tall, wide * 2.0),
                Vec3::Y * tall * 0.5,
                &mut meshes,
            ),
            ChildOf(tower),
        ))
        .id();
    // Its name is shown over it, and its ring lies just outside its
    // footing.
    commands.entity(boxed).insert((
        Named,
        Footprint {
            radius: tower::FOOTING + 1.0,
            height: tall,
            lift: tall * 0.5,
        },
    ));
    pick::watch(&mut commands, boxed);
}

/// Grows the city island about everything that stands on it: the
/// places that hold ground and the scholars' houses.
fn shape(
    mut land: ResMut<Land>,
    society: Res<Society>,
    scholars: Query<(Entity, &Scholar)>,
) {
    let mut spots = society
        .claims
        .iter()
        .filter(|claim| claim.place.island() == IslandId::City)
        .map(|claim| claim.at)
        .chain(houses::spots(&land, &scholars))
        .collect::<Vec<_>>();
    spots.sort_by(|a, b| {
        a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y))
    });
    spots.dedup();
    if land.city_spots() != spots {
        land.fill_city(&spots);
    }
}

/// The reach of the light on the tower's peak, and its brightness
/// at its fullest.
const LAMP_REACH: f32 = 90.0;
const LAMP_LUMENS: f32 = 6_000_000.0;

/// Seconds the light takes to come up or go down.
const EASE: f32 = 1.2;

/// How bright the tower's glass is at rest by day, and the gains
/// after dark and while the Hand works.
const TOWER: (f32, f32, f32) = (3.0, 12.0, 14.0);

/// The brightness of a lit window by day and the gain after dark.
const WINDOWS: (f32, f32) = (1.2, 16.0);

/// Burns the tower's light while the Hand works, and the windows of
/// the scholars' houses after dark.
fn shine(
    time: Res<Time>,
    clock: Res<Clock>,
    sky: Res<Sky>,
    pal: Res<Palette>,
    glass: Res<Glass>,
    hand: Option<Res<HandId>>,
    agents: Query<(&Identity, Has<Busy>), Without<Left>>,
    mut lamps: Query<&mut PointLight, With<Lamp>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut redraw: MessageWriter<RequestRedraw>,
    mut level: Local<f32>,
) {
    let busy = hand.is_some_and(|hand| {
        agents.iter().any(|(id, busy)| busy && id.0 == hand.0)
    });
    let target = if busy { 1.0 } else { 0.0 };
    let step = time.delta_secs() / EASE;
    *level += (target - *level).clamp(-step, step);
    // A flame wavers a little.
    let burn = *level * (0.93 + 0.07 * (clock.t * 5.0).sin());
    for mut lamp in &mut lamps {
        let lumens = LAMP_LUMENS * burn;
        if lamp.intensity != lumens {
            lamp.intensity = lumens;
        }
    }
    let glow = pal.glass_lit.to_linear();
    let (rest, dark, full) = TOWER;
    let (day, night) = WINDOWS;
    for (handle, gain) in [
        (&glass.lamp, rest + dark * sky.look.candle + full * burn),
        (&glass.windows, day + night * sky.look.candle),
    ] {
        let to = glow * gain;
        // Read first, so that the glass is not marked changed every
        // frame it is at rest.
        let differs = materials
            .get(handle)
            .is_some_and(|glass| glass.emissive != to);
        if differs && let Some(mut glass) = materials.get_mut(handle)
        {
            glass.emissive = to;
        }
    }
    if *level > 0.0 && *level < 1.0 || busy {
        redraw.write(RequestRedraw);
    }
}
