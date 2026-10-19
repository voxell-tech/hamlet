//! A running plugin: its component, instantiated with a store of its
//! own.

use std::path::{Path, PathBuf};

use bevy::ecs::component::Component;
use hamlet_realm_api::survey::Survey;
use hamlet_sdk::agents::{Agent, AgentList};
use hamlet_sdk::capability::{Capability, Granted};
use hamlet_sdk::papers::Papers;
use wasmtime::component::Component as Compiled;
use wasmtime::{Engine, PoolConcurrencyLimitError, Store};

use crate::cache;
use crate::db::{self, Db};
use crate::fault::Fault;
use crate::files::{self, Files};
use crate::host::{self, Reach, State};
use crate::interface::Hamlet;
use crate::limits::Limits;
use crate::storage::{Caps, Storage};

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
        store.set_fuel(limits.call_fuel).map_err(Fault::load)?;
        store.set_hostcall_fuel(limits.lift_bytes);
        let exports =
            Hamlet::instantiate(&mut store, component, &linker)
                .map_err(|error| {
                    match error.is::<PoolConcurrencyLimitError>() {
                        true => Fault::Load(format!(
                            "the host runs {} plugins at most, and \
                             has no room for another",
                            limits.pool_plugins
                        )),
                        false => Fault::load(error),
                    }
                })?;
        Ok(Self { store, exports })
    }

    /// This, for the plugin of this id, whose name its host
    /// functions give when they log.
    pub fn named(mut self, id: &str) -> Self {
        self.store.data_mut().plugin = id.into();
        self
    }

    /// This, with its storage in `folder`.
    pub fn stored(
        mut self,
        folder: PathBuf,
        limits: &Limits,
    ) -> Self {
        self.store.data_mut().storage =
            Some(Storage::new(folder, Caps::of(limits)));
        self
    }

    /// This, with its files in `folder`.
    pub fn filed(mut self, folder: PathBuf, limits: &Limits) -> Self {
        self.store.data_mut().files =
            Some(Files::new(folder, files::Caps::of(limits)));
        self
    }

    /// This, with its database in `folder`.
    pub fn tabled(
        mut self,
        folder: PathBuf,
        limits: &Limits,
    ) -> Self {
        self.store.data_mut().db =
            Some(Db::new(folder, db::Caps::of(limits)));
        self
    }

    /// This, with `state` as the blob the host holds for it.
    pub fn holding(mut self, state: &[u8]) -> Self {
        self.store.data_mut().state = state.to_vec();
        self
    }

    /// Gives the store what its collection functions reach, when it
    /// differs. Called before each call into the plugin.
    pub fn reach(&mut self, reach: Reach) {
        let state = self.store.data_mut();
        if state.reach != reach {
            state.reach = reach;
        }
    }

    /// The blob the host holds for the plugin.
    pub fn state(&self) -> &[u8] {
        &self.store.data().state
    }

    /// Gives the store what its host functions answer from: the
    /// capabilities the plugin holds, and a copy of the agents and of
    /// the papers when it may read them. Called before each call into
    /// the plugin, so a grant that changed is seen by the next.
    pub fn see(
        &mut self,
        granted: &Granted,
        agents: &AgentList,
        papers: &Papers,
    ) {
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
        let none = Papers::default();
        let readable = match granted.holds(Capability::ReadPapers) {
            true => papers,
            false => &none,
        };
        if state.papers != *readable {
            state.papers.clone_from(readable);
        }
    }

    /// Gives the store a copy of the realm when the plugin may read
    /// it, as [`Instance::see`] does the agents.
    pub fn see_world(&mut self, granted: &Granted, survey: &Survey) {
        let none = Survey::default();
        let readable = match granted.holds(Capability::ReadWorld) {
            true => survey,
            false => &none,
        };
        let state = self.store.data_mut();
        if state.survey != *readable {
            state.survey.clone_from(readable);
        }
    }

    /// Instantiates the component in `file`, compiled or from the
    /// cache beside it.
    pub fn from_file(
        engine: &Engine,
        file: &Path,
        limits: &Limits,
    ) -> Result<Self, Fault> {
        let (component, _) = cache::component(engine, file)?;
        Self::new(engine, &component, limits)
    }
}
