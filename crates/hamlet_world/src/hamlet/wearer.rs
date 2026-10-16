//! Each agent's robot named after its agent, so that it wears what
//! the agent wears.

use std::collections::HashMap;

use bevy::prelude::*;

use super::Housed;
use crate::generator::Robot;
use crate::generator::society::AgentId;
use crate::keeper::Wearer;

/// Keeps each robot's [`Wearer`] the name of its agent.
pub(super) fn name_robots(
    robots: Query<(Entity, &Robot, Option<&Wearer>)>,
    agents: Query<(&Housed, &Name)>,
    renamed: Query<
        (),
        (With<Housed>, Or<(Changed<Housed>, Changed<Name>)>),
    >,
    added: Query<(), Added<Robot>>,
    mut commands: Commands,
) {
    if added.is_empty() && renamed.is_empty() {
        return;
    }
    let names = agents
        .iter()
        .map(|(housed, name)| (housed.0, name.as_str()))
        .collect::<HashMap<AgentId, &str>>();
    for (robot, of, wearer) in &robots {
        match (names.get(&of.agent), wearer) {
            (Some(name), Some(wearer)) if wearer.0 == *name => {}
            (Some(name), _) => {
                commands
                    .entity(robot)
                    .try_insert(Wearer(name.to_string()));
            }
            (None, Some(_)) => {
                commands.entity(robot).try_remove::<Wearer>();
            }
            (None, None) => {}
        }
    }
}
