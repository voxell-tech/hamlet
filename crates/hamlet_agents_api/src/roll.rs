//! The count of changes to the agents and the hamlets the panels
//! show, and the checks a signal reads again by.

use bevy::app::{App, Update};
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::name::Name;
use bevy::ecs::query::{Added, Changed, Or, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy_fynix::{Signal, derived};
use hamlet_core::agents::{
    Account, Activity, Busy, Citizen, Left, Mayor, Purpose, Seen,
    Settlement, Treasury,
};
use hamlet_core::oversight::Waiting;

type AgentMoved = (
    With<Citizen>,
    Or<(
        Changed<Name>,
        Changed<Purpose>,
        Changed<Account>,
        Changed<Activity>,
        Added<Busy>,
        Added<Left>,
        Added<Mayor>,
    )>,
);

type HamletMoved = (
    With<Settlement>,
    Or<(Changed<Seen>, Changed<Treasury>, Changed<Name>)>,
);

/// The agents' components whose going changes what is shown.
type AgentLost<'w, 's> = (
    RemovedComponents<'w, 's, Busy>,
    RemovedComponents<'w, 's, Left>,
    RemovedComponents<'w, 's, Account>,
    RemovedComponents<'w, 's, Citizen>,
    RemovedComponents<'w, 's, Mayor>,
);

pub fn plugin(app: &mut App) {
    app.init_resource::<Roll>().add_systems(Update, turn_roll);
}

/// Counts the changes to what the panels show, so a panel reads
/// again only then.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Roll {
    /// The agents.
    agents: u64,
    /// The hamlets and their files.
    hamlets: u64,
    /// The things that wait for the chief.
    waits: u64,
}

impl Roll {
    /// The count of changes to the hamlets and their files.
    pub fn hamlets(&self) -> u64 {
        self.hamlets
    }
}

/// A check that fires when an agent changed.
pub fn agents_moved()
-> impl FnMut(&World) -> bool + Send + Sync + 'static {
    let mut seen = None;
    move |world: &World| {
        let now =
            world.get_resource::<Roll>().map(|roll| roll.agents);
        let moved = seen != Some(now);
        seen = Some(now);
        moved
    }
}

/// A check that fires when the record, the ledger or the limits of a
/// hamlet moved, or a hamlet came or went.
pub fn hamlets_moved()
-> impl FnMut(&World) -> bool + Send + Sync + 'static {
    let mut seen = None;
    move |world: &World| {
        let now =
            world.get_resource::<Roll>().map(|roll| roll.hamlets);
        let moved = seen != Some(now);
        seen = Some(now);
        moved
    }
}

/// A signal over the agents, read again when they changed.
pub fn rolled<T>(
    read: impl Fn(&World) -> T + Send + Sync + 'static,
) -> Signal<T> {
    derived(read).when(agents_moved())
}

fn turn_roll(
    agents: Query<(), AgentMoved>,
    hamlets: Query<(), HamletMoved>,
    waits: Query<(), Changed<Waiting>>,
    (mut idle, mut back, mut closed, mut gone, mut unseated): AgentLost,
    mut razed: RemovedComponents<Settlement>,
    mut answered: RemovedComponents<Waiting>,
    mut roll: ResMut<Roll>,
) {
    let lost = idle.read().count()
        + back.read().count()
        + closed.read().count()
        + gone.read().count()
        + unseated.read().count();
    if lost > 0 || !agents.is_empty() {
        roll.agents += 1;
    }
    if razed.read().count() > 0 || !hamlets.is_empty() {
        roll.hamlets += 1;
    }
    if answered.read().count() > 0 || !waits.is_empty() {
        roll.waits += 1;
    }
}
