//! The SDK's [`AgentList`] and [`SelectedAgent`], kept from the
//! core's agents and the selection, the core's marks on an agent, a
//! hamlet and a road that a plugin reads, and the core's names of
//! them.

use bevy::ecs::lifecycle::RemovedComponents;
use bevy::prelude::*;
pub use hamlet_core::agents::{
    Account, Activity, Busy, Citizen, Left, LivesIn, Mayor,
    MergedInto, Purpose, Road, Settlement, Treasury, Works, mayor,
    name_of, register,
};
use hamlet_sdk::agents::{Agent, AgentList, Kept, SelectedAgent};

use crate::convo::{AgentSelected, Me};

/// Keeps [`AgentList`] in step with the core's agents, and
/// [`SelectedAgent`] with the selection.
pub fn plugin(app: &mut App) {
    app.init_resource::<AgentList>()
        .init_resource::<AgentSelected>()
        .init_resource::<SelectedAgent>()
        .add_systems(
            Update,
            (
                keep.in_set(Kept).run_if(moved),
                keep_selected.in_set(Kept).run_if(
                    resource_changed::<AgentSelected>
                        .or_eager(resource_exists_and_changed::<Me>),
                ),
            ),
        );
}

/// Writes the selected agent's name when it differs.
fn keep_selected(
    selected: Res<AgentSelected>,
    me: Option<Res<Me>>,
    names: Query<&Name>,
    mut agent: ResMut<SelectedAgent>,
) {
    let now = match (selected.0, me) {
        (true, Some(me)) => names
            .get(me.agent)
            .ok()
            .map(ToString::to_string)
            .or_else(|| me.with.clone()),
        _ => None,
    };
    if agent.0 != now {
        agent.0 = now;
    }
}

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
