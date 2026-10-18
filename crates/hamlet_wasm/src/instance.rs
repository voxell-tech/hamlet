//! A running plugin: its component, instantiated with a store of its
//! own.

use std::path::Path;

use bevy::ecs::component::Component;
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
        let linker = host::linker(engine).map_err(Fault::load)?;
        let mut store = Store::new(engine, State::new(limits));
        store.limiter(|state| &mut state.limiter);
        store.epoch_deadline_trap();
        store.set_epoch_deadline(limits.call_ticks());
        let exports =
            Hamlet::instantiate(&mut store, component, &linker)
                .map_err(Fault::load)?;
        Ok(Self { store, exports })
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
