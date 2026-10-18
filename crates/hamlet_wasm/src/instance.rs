//! A running plugin: its component, instantiated with a store of its
//! own.

use std::path::Path;

use bevy::ecs::component::Component;
use hamlet_sdk::agents::{Agent, AgentList};
use hamlet_sdk::capability::{Capability, Granted};
use wasmtime::component::Component as Compiled;
use wasmtime::{Engine, Store};

use crate::fault::Fault;
use crate::host::{self, State};
use crate::interface::Hamlet;
use crate::limits::Limits;

/// On the entity of an installed plugin: its store and the exports
/// of its component. The store is `Send` and `Sync` because its
/// [`State`] is, so it needs no resource of its own.
#[derive(Component)]
pub struct Instance {
    pub(crate) store: Store<State>,
    pub(crate) exports: Hamlet,
}

impl Instance {
    /// Instantiates a compiled component.
    pub fn new(
        engine: &Engine,
        component: &Compiled,
        limits: &Limits,
    ) -> Result<Self, Fault> {
        let linker = host::linker(engine, component)?;
        let mut store = Store::new(engine, State::new(limits));
        store.limiter(|state| &mut state.limiter);
        store.epoch_deadline_trap();
        store.set_epoch_deadline(limits.call_ticks());
        let exports =
            Hamlet::instantiate(&mut store, component, &linker)
                .map_err(Fault::load)?;
        Ok(Self { store, exports })
    }

    /// This, for the plugin of this id, whose name its host
    /// functions give when they log.
    pub fn named(mut self, id: &str) -> Self {
        self.store.data_mut().plugin = id.into();
        self
    }

    /// Gives the store what its host functions answer from: the
    /// capabilities the plugin holds, and a copy of the agents when
    /// it may read them. Called before each call into the plugin, so
    /// a grant that changed is seen by the next.
    pub fn see(&mut self, granted: &Granted, agents: &AgentList) {
        let state = self.store.data_mut();
        if state.granted != *granted {
            state.granted.clone_from(granted);
        }
        let readable: &[Agent] =
            match granted.holds(Capability::ReadAgents) {
                true => &agents.0,
                false => &[],
            };
        if state.agents != readable {
            state.agents = readable.to_vec();
        }
    }

    /// Compiles the component in `file` and instantiates it.
    pub fn from_file(
        engine: &Engine,
        file: &Path,
        limits: &Limits,
    ) -> Result<Self, Fault> {
        let component =
            Compiled::from_file(engine, file).map_err(Fault::load)?;
        Self::new(engine, &component, limits)
    }
}
