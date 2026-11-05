//! The harbor: a pier on each island where its coast faces the other,
//! a boat moored at each, and the faint sea route between them.

mod shapes;
#[cfg(test)]
mod tests;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::generator::land::{IslandId, Land};
use crate::palette::Palette;
use crate::pick::{Centre, Following, Orbit, glide_to};
use crate::{RealmSystems, Stock};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (
            raise.in_set(RealmSystems::Act),
            bob.after(RealmSystems::Act),
        ),
    );
}

/// How high over the still water the route is drawn, clear of the
/// tallest waves, which the water raises on the graphics card alone.
const ROUTE_LIFT: f32 = 1.4;

/// How a moored boat rides the swell: its rest over the still water,
/// as the swell near a shore is low, and for each of its swells the
/// height in metres and the pace in radians a second.
const BOAT_REST: f32 = 0.12;
const SWELLS: [(f32, f32); 3] =
    [(0.22, 1.03), (0.09, 1.71), (0.04, 2.6)];

/// How far a boat rolls and pitches with the swell, in radians.
const ROLL: f32 = 0.05;
const PITCH: f32 = 0.025;

/// Rocks each moored boat on a swell of its own, worked out here and
/// not on the graphics card, so that whoever rides it rides with it
/// and its picking keeps to where it is drawn.
fn bob(time: Res<Time>, mut boats: Query<(&Boat, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (boat, mut at) in &mut boats {
        // Each island's boat a beat apart from the other's.
        let beat = boat.0 as usize as f32 * 2.3;
        let swell = |offset: f32| -> f32 {
            SWELLS
                .iter()
                .enumerate()
                .map(|(k, (height, pace))| {
                    height
                        * (pace * t + beat + offset * k as f32).sin()
                })
                .sum()
        };
        // The swell at its tallest, against which it leans its most.
        let most: f32 = SWELLS.iter().map(|(height, _)| height).sum();
        at.translation.y = BOAT_REST + swell(0.0);
        at.rotation = Quat::from_euler(
            EulerRot::YXZ,
            0.0,
            PITCH * swell(1.1) / most,
            ROLL * swell(2.2) / most,
        );
    }
}

/// The pier of an island. Its +Z points out to sea toward the other
/// island, and it stands at the water's edge.
#[derive(Component, Clone, Copy, Debug)]
pub struct Pier(pub IslandId);

/// The boat moored at the pier of an island.
#[derive(Component, Clone, Copy, Debug)]
pub struct Boat(pub IslandId);

/// The dashes of the sea route between the two piers.
#[derive(Component)]
struct Route;

/// Where a pier stands: its landfall on the coast and the way out to
/// sea.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Landfall {
    coast: Vec2,
    out: Vec2,
}

/// Where the piers stand, as they were last built.
#[derive(Resource, Clone, PartialEq, Debug)]
pub(crate) struct Harbor([Landfall; 2]);

impl Harbor {
    /// The place on the pier of `isle` that lies `along` metres to
    /// sea of its coast and `across` to the right of its middle,
    /// looking out to sea.
    pub(crate) fn spot(
        &self,
        isle: IslandId,
        along: f32,
        across: f32,
    ) -> Vec2 {
        let at = self.0[isle as usize];
        at.coast
            + at.out * along
            + Vec2::new(at.out.y, -at.out.x) * across
    }

    /// The turn about the vertical of something on the pier of `isle`
    /// that faces out to sea.
    pub(crate) fn seaward(&self, isle: IslandId) -> f32 {
        let out = self.0[isle as usize].out;
        out.x.atan2(out.y)
    }

    /// The height of the deck of a pier over the water.
    pub(crate) const DECK: f32 = shapes::DECK + 0.06;

    /// Where the `slot`th of those who wait on the pier of `isle`
    /// stands on it, in a file from near its head back toward land.
    pub(crate) fn queue(&self, isle: IslandId, slot: usize) -> Vec2 {
        let along = (shapes::SEAWARD - 4.0 - slot as f32 * 1.3)
            .max(-shapes::INLAND + 0.5);
        self.spot(isle, along, -0.4)
    }
}

fn landfall(land: &Land, isle: IslandId) -> Landfall {
    Landfall {
        coast: land.coast_toward_other(isle),
        out: land.toward_other(isle),
    }
}

/// Builds the piers, boats and route again whenever the land puts
/// either coast somewhere else.
fn raise(
    mut commands: Commands,
    land: Res<Land>,
    pal: Res<Palette>,
    stock: Res<Stock>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    standing: Query<Entity, Or<(With<Pier>, With<Route>)>>,
    harbor: Option<Res<Harbor>>,
) {
    if harbor.is_some() && !land.is_changed() {
        return;
    }
    let wanted =
        Harbor(IslandId::BOTH.map(|isle| landfall(&land, isle)));
    if harbor.is_some_and(|harbor| *harbor == wanted) {
        return;
    }
    for entity in &standing {
        commands.entity(entity).despawn();
    }
    let water = land.set.land.water_y;
    let pier = meshes.add(shapes::pier(&pal).build());
    for isle in IslandId::BOTH {
        let paint = pal.paints[match isle {
            IslandId::City => 2,
            IslandId::Hamlet => 1,
        }];
        let boat = meshes.add(shapes::boat(&pal, paint).build());
        let coast = wanted.spot(isle, 0.0, 0.0);
        commands.spawn((
            Pier(isle),
            Mesh3d(pier.clone()),
            MeshMaterial3d(stock.matte.clone()),
            Transform::from_xyz(coast.x, water, coast.y)
                .with_rotation(Quat::from_rotation_y(
                    wanted.seaward(isle),
                )),
            Visibility::Inherited,
            children![(
                Boat(isle),
                Mesh3d(boat),
                MeshMaterial3d(stock.matte.clone()),
                Transform::from_xyz(
                    shapes::ALONGSIDE,
                    BOAT_REST,
                    shapes::MOORED
                ),
            )],
        ));
    }
    // The route runs from just past one pier's head to just short of
    // the other's.
    let past = shapes::SEAWARD + shapes::OFFING;
    let [city, hamlet] =
        IslandId::BOTH.map(|isle| wanted.spot(isle, past, 0.0));
    let way = hamlet - city;
    commands.spawn((
        Route,
        Mesh3d(meshes.add(shapes::route(way.length(), 1.0).build())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 1.0, 1.0, 0.3),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::from_xyz(city.x, water + ROUTE_LIFT, city.y)
            .with_rotation(Quat::from_rotation_y(way.x.atan2(way.y))),
        NotShadowCaster,
    ));
    commands.insert_resource(wanted);
}

/// Glides the camera to the pier of the island it is not over, and
/// lets go of any thing it was following. The harbor control calls
/// it to cross from one island to the other.
pub fn glide_to_other_pier(world: &mut World) {
    let centre = world.resource::<Centre>().0;
    let focus = centre + world.resource::<Orbit>().pan;
    let there = world.resource::<Land>().island_at(focus).other();
    let Some(harbor) = world.get_resource::<Harbor>() else {
        return;
    };
    let at = harbor.spot(there, shapes::SEAWARD * 0.5, 0.0);
    world.resource_mut::<Following>().0 = None;
    glide_to(world, at);
}
