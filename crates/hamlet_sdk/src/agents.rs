//! The agents and their hamlets, as a plugin that holds
//! `read.agents` reads them.

use bevy::ecs::lifecycle::RemovedComponents;
use bevy::prelude::*;
use hamlet_core::agents::{Citizen, Left, LivesIn, Settlement};

/// Keeps [`AgentList`] in step with the core's agents.
pub fn plugin(app: &mut App) {
    app.init_resource::<AgentList>()
        .add_systems(Update, keep.in_set(Kept).run_if(moved));
}

/// The system that writes [`AgentList`].
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Kept;

/// A living agent and the name of its hamlet.
#[derive(Clone, PartialEq, Debug)]
pub struct Agent {
    pub name: String,
    pub hamlet: String,
}

/// The living agents, by name, each with its hamlet.
#[derive(Resource, Default, PartialEq, Clone, Debug)]
pub struct AgentList(pub Vec<Agent>);

/// Whether an agent came, went, was renamed or moved.
fn moved(
    changed: Query<
        (),
        Or<(
            Added<Citizen>,
            Added<Left>,
            Changed<Name>,
            Changed<LivesIn>,
        )>,
    >,
    mut gone: RemovedComponents<Citizen>,
    mut back: RemovedComponents<Left>,
) -> bool {
    !changed.is_empty()
        || gone.read().count() > 0
        || back.read().count() > 0
}

/// Writes the list when it differs from the agents there are.
fn keep(
    living: Query<(&Name, &LivesIn), (With<Citizen>, Without<Left>)>,
    hamlets: Query<&Name, With<Settlement>>,
    mut list: ResMut<AgentList>,
) {
    let mut now = living
        .iter()
        .map(|(name, lives)| Agent {
            name: name.to_string(),
            hamlet: hamlets
                .get(lives.0)
                .map(ToString::to_string)
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    now.sort_by(|a, b| {
        (&a.hamlet, &a.name).cmp(&(&b.hamlet, &b.name))
    });
    if list.0 != now {
        list.0 = now;
    }
}
