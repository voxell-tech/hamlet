//! The count of changes to what the panels show, and checks on it.

use bevy::app::{App, PreUpdate};
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::name::Name;
use bevy::ecs::query::{Added, Changed, Or, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy_fynix::{Signal, derived, resource};
use hamlet_client::realm::{
    Account, Activity, Busy, Bylaws, HamletAgent, HamletId, Lead,
    Learned, Left, Purpose, Treasury, Works,
};
use hamlet_client::waits::Waiting;

use crate::PanelSystems;

type AgentMoved = (
    With<HamletAgent>,
    Or<(
        Changed<Name>,
        Changed<Purpose>,
        Changed<Account>,
        Changed<Activity>,
        Added<Busy>,
        Added<Learned>,
        Added<Left>,
        Added<Lead>,
    )>,
);

type HamletMoved = (
    With<HamletId>,
    Or<(
        Changed<Bylaws>,
        Changed<Works>,
        Changed<Treasury>,
        Changed<Name>,
    )>,
);

/// The agents' components whose going changes what is shown.
type AgentLost<'w, 's> = (
    RemovedComponents<'w, 's, Busy>,
    RemovedComponents<'w, 's, Learned>,
    RemovedComponents<'w, 's, Left>,
    RemovedComponents<'w, 's, Account>,
    RemovedComponents<'w, 's, HamletAgent>,
    RemovedComponents<'w, 's, Lead>,
);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Roll>()
        .add_systems(PreUpdate, turn_roll.in_set(PanelSystems::Bind));
}

/// The changes to what the panels show.
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
    pub(crate) fn hamlets(&self) -> u64 {
        self.hamlets
    }
}

/// A check that fires when an agent changed.
pub(crate) fn agents_moved()
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

/// A check that fires when a hamlet or its record changed.
pub(crate) fn hamlets_moved()
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
pub(crate) fn rolled<T>(
    read: impl Fn(&World) -> T + Send + Sync + 'static,
) -> Signal<T> {
    derived(read).when(agents_moved())
}

/// A check that fires when the resource `R` has changed.
pub fn moved<R: Resource>()
-> impl FnMut(&World) -> bool + Send + Sync + 'static {
    let mut signal = resource::<R, _>(|_| ());
    move |world: &World| signal.changed(world)
}

/// A check that fires when either of two does.
pub fn either(
    mut one: impl FnMut(&World) -> bool + Send + Sync + 'static,
    mut other: impl FnMut(&World) -> bool + Send + Sync + 'static,
) -> impl FnMut(&World) -> bool + Send + Sync + 'static {
    move |world: &World| {
        // Both look, so that neither holds a change over.
        let (one, other) = (one(world), other(world));
        one || other
    }
}

pub(crate) fn turn_roll(
    agents: Query<(), AgentMoved>,
    hamlets: Query<(), HamletMoved>,
    waits: Query<(), Changed<Waiting>>,
    (
        mut idle,
        mut taken_home,
        mut back,
        mut closed,
        mut gone,
        mut unseated,
    ): AgentLost,
    mut razed: RemovedComponents<HamletId>,
    mut answered: RemovedComponents<Waiting>,
    mut roll: ResMut<Roll>,
) {
    let lost = idle.read().count()
        + taken_home.read().count()
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
