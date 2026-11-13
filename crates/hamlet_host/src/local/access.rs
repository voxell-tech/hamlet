//! What an actor reaches of the host that runs it: the realm folder
//! and the values the program that opened the host gave it.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use hamlet_actor_core::ActorContext;

use super::{HostOptions, LocalHandle, host_shut_down};

/// What an actor reaches of the host that runs it, from its `create`
/// on: the realm folder and the values the program that opened the
/// host gave it. It does not hold the host, so an actor that keeps it
/// never keeps its host open.
#[derive(Clone)]
pub struct HostRef {
    folder: PathBuf,
    values: Values,
}

impl HostRef {
    /// The host that runs the actor of `ctx`. Fails for an actor that
    /// runs in no host of this crate, and once its host is gone.
    pub fn of(ctx: &ActorContext) -> Result<Self> {
        let handle =
            ctx.host().context("the actor runs in no host")?;
        let handle: &dyn Any = &*handle;
        let handle = handle
            .downcast_ref::<LocalHandle>()
            .context("the actor runs in no Hamlet host")?;
        let host =
            handle.host.upgrade().ok_or_else(host_shut_down)?;
        Ok(Self {
            folder: host.dir.clone(),
            values: host.options.values.clone(),
        })
    }

    /// The realm folder the host was opened on.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The value of type `T` the host was given, if one was.
    pub fn value<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.values.get()
    }
}

/// The values a host gives its actors, at most one of each type.
#[derive(Clone, Default)]
pub struct Values(Arc<HashMap<TypeId, Value>>);

/// One value, with its type's name for the host's debug output.
#[derive(Clone)]
struct Value {
    type_name: &'static str,
    value: Arc<dyn Any + Send + Sync>,
}

impl Values {
    /// The value of type `T`, if one was given.
    fn get<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        let value = self.0.get(&TypeId::of::<T>())?.value.clone();
        value.downcast().ok()
    }
}

impl fmt::Debug for Values {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set()
            .entries(self.0.values().map(|value| value.type_name))
            .finish()
    }
}

impl HostOptions {
    /// These options, with `value` given to every actor of the host,
    /// in place of any value of its type given before.
    #[must_use]
    pub fn with_value<T: Any + Send + Sync>(
        mut self,
        value: Arc<T>,
    ) -> Self {
        Arc::make_mut(&mut self.values.0).insert(
            TypeId::of::<T>(),
            Value {
                type_name: std::any::type_name::<T>(),
                value,
            },
        );
        self
    }
}
