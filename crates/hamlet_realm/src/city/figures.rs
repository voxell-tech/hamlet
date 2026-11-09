//! The figure of each scholar that has a house in the city: a robot
//! that goes about the yard of its house as a hamlet's does, and that
//! the hand can lift.

use bevy::prelude::*;
use hamlet_client::realm::{Busy, Identity, Scholar};

use super::houses::{self, DOOR_OUT};
use crate::generator::land::Land;
use crate::generator::society::AgentId;
use crate::generator::{Robot, house};
use crate::keeper::{self, Kit, Who};
use crate::pick::{self, Named, PickSettings, Thing, hit_box};

/// The robot of a scholar, standing where its house does.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub(super) struct Figure {
    scholar: Entity,
    at: Vec2,
    turn: f32,
}

/// The id a figure's robot answers to, which no hamlet's agent has.
fn robot_id(number: u64) -> AgentId {
    AgentId(u32::MAX - number as u32)
}

/// Stands a figure at the house of every scholar that has not left,
/// and takes down the figure of a house that moved or is gone.
pub(super) fn stand(
    mut commands: Commands,
    land: Res<Land>,
    kit: Res<Kit>,
    pick: Res<PickSettings>,
    scholars: Query<(Entity, &Scholar)>,
    figures: Query<(Entity, &Figure)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let wanted = houses::wanted(&land, &scholars, &[])
        .into_iter()
        .filter(|home| !home.shut)
        .map(|home| Figure {
            scholar: home.scholar,
            at: home.at,
            turn: home.turn,
        })
        .collect::<Vec<_>>();
    for (entity, figure) in &figures {
        if !wanted.contains(figure) {
            commands.entity(entity).despawn();
        }
    }
    for want in wanted {
        if figures.iter().any(|(_, figure)| *figure == want) {
            continue;
        }
        let Ok((_, scholar)) = scholars.get(want.scholar) else {
            continue;
        };
        let number = scholar.0.id.0;
        let yard = house::yard_of(
            &land,
            AgentId(number as u32),
            false,
            0.0,
            0,
        );
        let robot = Robot::in_city(
            &land,
            robot_id(number),
            yard,
            want.at,
            want.turn,
            DOOR_OUT,
        );
        let figure = keeper::spawn_agent(
            &mut commands,
            &kit,
            &mut materials,
            Who::agent(number as u32 * 5 + 1),
            Transform::from_translation(robot.porch())
                .with_rotation(Quat::from_rotation_y(want.turn)),
            None,
        );
        commands.entity(figure).insert((robot, want));
        let boxed = commands
            .spawn((
                hit_box(
                    Thing::Scholar(want.scholar),
                    pick.robot_box,
                    Vec3::Y * pick.robot_box.y * 0.5,
                    &mut meshes,
                ),
                Named,
                ChildOf(figure),
            ))
            .id();
        pick::watch(&mut commands, boxed);
    }
}

/// Has each figure pace its yard while any of its scholar's hires is
/// busy.
pub(super) fn work(
    scholars: Query<&Scholar>,
    hires: Query<(&Identity, Has<Busy>)>,
    mut figures: Query<(&Figure, &mut Robot)>,
) {
    for (figure, mut robot) in &mut figures {
        let Ok(scholar) = scholars.get(figure.scholar) else {
            continue;
        };
        let busy = hires.iter().any(|(id, busy)| {
            busy && scholar.0.hires.contains(&id.0)
        });
        if robot.is_busy() != busy {
            robot.set_busy(busy);
        }
    }
}
