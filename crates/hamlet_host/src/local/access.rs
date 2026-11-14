//! What an actor reaches of the host that runs it: the realm folder
//! and the values the program that opened the host gave it.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use hamlet_actor_core::{ActorContext, ActorLifecycle};

use super::{HostInner, HostOptions, LocalHandle, host_shut_down};
use crate::address::Address;

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
    /// The `HostRef` of the host that runs the actor of `ctx`. Fails
    /// for an actor that runs in no host of this crate, and once its
    /// host is gone.
    pub fn of(ctx: &ActorContext) -> Result<Self> {
        let host = running(ctx)?.host;
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

/// The generation of an actor that a host of this crate runs.
pub(super) struct Running {
    pub(super) host: Arc<HostInner>,
    pub(super) actor_id: String,
    /// The generation's id, as the host's map holds it.
    pub(super) generation: u64,
}

impl Running {
    /// The actor's address.
    pub(super) fn address(&self) -> Result<Address> {
        let (kind, key) =
            self.actor_id.split_once('/').with_context(|| {
                format!("actor id {} has no kind", self.actor_id)
            })?;
        Ok(Address::new(kind, key)?)
    }
}

/// The generation that `ctx` belongs to, and its host. Fails for an
/// actor that runs in no host of this crate, and once its host is
/// gone.
pub(super) fn running(ctx: &ActorContext) -> Result<Running> {
    let handle = ctx.host().context("the actor runs in no host")?;
    let handle: &dyn Any = &*handle;
    let handle = handle
        .downcast_ref::<LocalHandle>()
        .context("the actor runs in no Hamlet host")?;
    Ok(Running {
        host: handle.host.upgrade().ok_or_else(host_shut_down)?,
        actor_id: handle.actor_id.clone(),
        generation: handle.generation,
    })
}

/// The generation that `ctx` belongs to, and its host, while the
/// host's map holds that generation. Fails with `actor.stopping` once
/// the generation has ended, also when its key was made again.
pub(super) fn current(ctx: &ActorContext) -> Result<Running> {
    let running = running(ctx)?;
    let ended = !running
        .host
        .actors
        .lock()
        .lives
        .get(&running.actor_id)
        .is_some_and(|life| life.is_generation(running.generation));
    if ended {
        return Err(ActorLifecycle::Stopping.build().context(
            format!(
                "generation {} of actor {} has ended",
                running.generation, running.actor_id
            ),
        ));
    }
    Ok(running)
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
