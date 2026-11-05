//! The scholars who are not let into the city yet, each a small
//! figure standing on the city's pier until it has a house.

use bevy::prelude::*;
use hamlet_client::realm::Scholar;
use hamlet_protocol::client::Arrival;

use crate::generator::land::{IslandId, Land};
use crate::harbor::Harbor;
use crate::keeper::{self, Kit, Who};
use crate::pick::{self, Named, PickSettings, Thing, hit_box};

/// The figure of a scholar waiting on the pier.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct Newcomer(pub(super) Entity);

/// Stands a figure on the pier for each scholar that waits, in the
/// order of their ids, and takes down the figure of one that is let
/// in or has left.
pub(super) fn wait(
    mut commands: Commands,
    harbor: Option<Res<Harbor>>,
    land: Res<Land>,
    kit: Res<Kit>,
    pick: Res<PickSettings>,
    scholars: Query<(Entity, &Scholar)>,
    mut figures: Query<(Entity, &Newcomer, &mut Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(harbor) = harbor else {
        return;
    };
    let mut waiting = scholars
        .iter()
        .filter(|(_, scholar)| {
            scholar.0.arrival != Arrival::Housed
                && scholar.0.left.is_none()
        })
        .map(|(entity, scholar)| (scholar.0.id.0, entity))
        .collect::<Vec<_>>();
    waiting.sort();
    for (entity, figure, _) in &figures {
        if !waiting.iter().any(|(_, scholar)| *scholar == figure.0) {
            commands.entity(entity).despawn();
        }
    }
    let deck = land.set.land.water_y + Harbor::DECK;
    for (slot, (number, scholar)) in waiting.into_iter().enumerate() {
        let at = harbor.queue(IslandId::City, slot);
        // Facing the land, as one waits to be let in.
        let facing = harbor.seaward(IslandId::City)
            + std::f32::consts::PI
            + 0.25 * (slot as f32 % 3.0 - 1.0);
        let stands = Transform::from_xyz(at.x, deck, at.y)
            .with_rotation(Quat::from_rotation_y(facing));
        if let Some((_, _, mut transform)) = figures
            .iter_mut()
            .find(|(_, figure, _)| figure.0 == scholar)
        {
            transform.set_if_neq(stands);
            continue;
        }
        let robot = keeper::spawn_agent(
            &mut commands,
            &kit,
            &mut materials,
            Who::agent(number as u32 * 5 + 1),
            stands,
            None,
        );
        commands.entity(robot).insert(Newcomer(scholar));
        let boxed = commands
            .spawn((
                hit_box(
                    Thing::Scholar(scholar),
                    pick.robot_box,
                    Vec3::Y * pick.robot_box.y * 0.5,
                    &mut meshes,
                ),
                Named,
                ChildOf(robot),
            ))
            .id();
        pick::watch(&mut commands, boxed);
    }
}
