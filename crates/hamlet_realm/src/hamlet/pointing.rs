//! The boxes that make houses, robots and commons answer the pointer.
//! Their own meshes are kept only for drawing, so a ray cannot read
//! them.

use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use hamlet_core::agents::{Left, LivesIn, Mayor, Settlement};

use super::{Housed, Returning, Sited, first, view};
use crate::generator::house::Remains;
use crate::generator::land::Land;
use crate::generator::society::AgentId;
use crate::generator::{Common, House, Passage, Robot};
use crate::keeper::Act;
use crate::pick::{
    self, Footprint, Named, PickSettings, Thing, hit_box,
};
use crate::{WorldSystems, adopt};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (
            retitle.run_if(
                any_match_filter::<(
                    With<Housed>,
                    Or<(Changed<Housed>, Added<Mayor>, Added<Left>)>,
                )>
                    .or_eager(any_component_removed::<Mayor>),
            ),
            houses.run_if(any_match_filter::<Added<House>>),
            robots.run_if(any_match_filter::<Added<Robot>>),
            commons.run_if(any_match_filter::<Added<Common>>),
        )
            .chain()
            .after(view::aim)
            .before(WorldSystems),
    );
}

/// Every agent's standing with its hamlet.
#[derive(SystemParam)]
struct Standing<'w, 's> {
    hamlets: Query<'w, 's, &'static Settlement>,
    agents: Query<
        'w,
        's,
        (
            Entity,
            &'static Housed,
            &'static LivesIn,
            Has<Mayor>,
            Has<Left>,
            Has<Returning>,
        ),
    >,
    land: Res<'w, Land>,
}

/// One agent as the pointer finds it.
#[derive(Clone, Copy)]
struct Stood {
    entity: Entity,
    /// True when it is the mayor the window's own chat is with.
    mayor: bool,
    left: bool,
    returning: bool,
}

/// Every agent by the agent of the society it is drawn as.
struct Known {
    agents: HashMap<AgentId, Stood>,
    /// True when the house of an agent that left rests as a ruin.
    ruins: bool,
}

impl Standing<'_, '_> {
    fn known(&self) -> Known {
        let agents = self
            .agents
            .iter()
            .map(|(entity, housed, lives, mayor, left, returning)| {
                let stood = Stood {
                    entity,
                    mayor: mayor && first(&self.hamlets, lives),
                    left,
                    returning,
                };
                (housed.0, stood)
            })
            .collect();
        Known {
            agents,
            ruins: self.land.set.house.removed == Remains::Ruin,
        }
    }
}

impl Known {
    /// The thing the house or the robot of `agent` answers the
    /// pointer as. One that left answers as the ruin of its
    /// house, where a ruin is what its house rests as, and
    /// otherwise not at all.
    fn thing(&self, agent: AgentId, house: bool) -> Option<Thing> {
        let stood = self.agents.get(&agent)?;
        Some(match (stood.left, stood.mayor, house) {
            (true, _, true) if self.ruins => {
                Thing::Ruin(stood.entity)
            }
            (true, ..) => return None,
            (_, true, true) => Thing::Cottage,
            (_, true, false) => Thing::Keeper,
            (_, false, true) => Thing::ResidentHouse(stood.entity),
            (_, false, false) => Thing::Resident(stood.entity),
        })
    }
}

/// Keeps each box pointing at what its agent is now: a robot whose
/// agent left answers no pointer, and one that took or left the
/// office answers as what it is.
fn retitle(
    mut commands: Commands,
    standing: Standing,
    owners: Query<(Option<&House>, Option<&Robot>)>,
    boxes: Query<(Entity, &ChildOf, &Thing)>,
) {
    let known = standing.known();
    for (entity, parent, was) in &boxes {
        let now = match owners.get(parent.parent()) {
            Ok((Some(house), _)) => known.thing(house.agent, true),
            Ok((_, Some(robot))) => known.thing(robot.agent, false),
            _ => continue,
        };
        match now {
            None => commands.entity(entity).try_despawn(),
            Some(now) if now != *was => {
                commands.entity(entity).try_insert(now);
            }
            Some(_) => {}
        }
    }
}

/// Spawns a box under `parent` that is named and takes the room of
/// `footprint`.
fn named(
    commands: &mut Commands,
    parent: Entity,
    hit: impl Bundle,
    footprint: Footprint,
) {
    let boxed = commands.spawn(hit).id();
    commands.entity(boxed).try_insert((Named, footprint));
    adopt(commands, parent, boxed);
    pick::watch(commands, boxed);
}

fn houses(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    standing: Standing,
    pick: Res<PickSettings>,
    houses: Query<(Entity, &House), Added<House>>,
) {
    let known = standing.known();
    let (radius, height) = pick.house;
    for (entity, house) in &houses {
        // An archived house answers only as a ruin.
        let thing = known.thing(house.agent, true).filter(|thing| {
            !house.archived || matches!(thing, Thing::Ruin(_))
        });
        let Some(thing) = thing else {
            continue;
        };
        let size = Vec3::new(radius * 1.3, height, radius * 1.3);
        let middle = Vec3::Y * height * 0.5;
        named(
            &mut commands,
            entity,
            hit_box(thing, size, middle, &mut meshes),
            Footprint {
                radius,
                height,
                lift: height * 0.5,
            },
        );
    }
}

/// A restored agent's robot is seen coming back.
fn robots(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    standing: Standing,
    pick: Res<PickSettings>,
    robots: Query<(Entity, &Robot), Added<Robot>>,
) {
    let known = standing.known();
    for (entity, robot) in &robots {
        if let Some(stood) = known.agents.get(&robot.agent)
            && stood.returning
        {
            commands.entity(stood.entity).try_remove::<Returning>();
            commands
                .entity(entity)
                .try_insert(Passage::new(Act::Appear));
        }
        if let Some(thing) = known.thing(robot.agent, false) {
            let middle = Vec3::Y * pick.robot_box.y * 0.5;
            let boxed = commands
                .spawn(hit_box(
                    thing,
                    pick.robot_box,
                    middle,
                    &mut meshes,
                ))
                .id();
            adopt(&mut commands, entity, boxed);
            pick::watch(&mut commands, boxed);
        }
    }
}

fn commons(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    pick: Res<PickSettings>,
    sited: Query<(Entity, &Sited)>,
    commons: Query<(Entity, &Common), Added<Common>>,
) {
    let (radius, height) = pick.common;
    // Low, so it does not stand in front of the hall beside it.
    let slab = 0.3;
    for (entity, common) in &commons {
        let Some((hamlet, _)) =
            sited.iter().find(|(_, sited)| sited.0 == common.town)
        else {
            continue;
        };
        let size = Vec3::new(radius * 1.6, slab, radius * 1.6);
        let middle = Vec3::Y * slab * 0.5;
        named(
            &mut commands,
            entity,
            hit_box(Thing::Common(hamlet), size, middle, &mut meshes),
            Footprint {
                radius,
                height,
                lift: slab * 0.5,
            },
        );
    }
}
