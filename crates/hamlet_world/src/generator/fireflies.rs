//! Fireflies over each town by night.

use bevy::prelude::*;
use hamlet_vfx::sparks::Sparks;

use super::land::Land;
use super::society::{Society, TownId};
use crate::Sky;
use crate::sky::SkySettings;

/// The fireflies of a town.
#[derive(Component)]
pub(super) struct Fireflies(TownId);

/// Keeps a swarm of fireflies over each town by night.
pub(super) fn keep(
    mut commands: Commands,
    sparks: Option<Res<Sparks>>,
    sky: Res<Sky>,
    sky_set: Res<SkySettings>,
    land: Res<Land>,
    society: Res<Society>,
    swarms: Query<(Entity, &Fireflies, &Transform)>,
) {
    let out = sparks.as_ref().is_some_and(|s| s.any())
        && sky_set.night(sky.hour);
    // A swarm stays where its town stands.
    let strayed = |of: TownId, at: &Transform| {
        society.town(of).is_none_or(|town| {
            land.sites[town.site].at != at.translation.xz()
        })
    };
    let mut gone = Vec::new();
    for (swarm, of, at) in &swarms {
        if !out || strayed(of.0, at) {
            commands.entity(swarm).try_despawn();
            gone.push(of.0);
        }
    }
    let Some(sparks) = sparks.filter(|_| out) else {
        return;
    };
    for town in &society.towns {
        let flies = swarms.iter().any(|(_, of, _)| {
            of.0 == town.id && !gone.contains(&of.0)
        });
        if flies {
            continue;
        }
        let Some(swarm) = sparks.swarm() else {
            return;
        };
        let at = land.sites[town.site].at;
        commands.spawn((
            Fireflies(town.id),
            swarm,
            Transform::from_xyz(at.x, land.surface(at), at.y),
        ));
    }
}
