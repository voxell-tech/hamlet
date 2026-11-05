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
    app.add_systems(Update, raise.in_set(RealmSystems::Act));
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
                    0.0,
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
        Transform::from_xyz(city.x, water + 0.1, city.y)
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
