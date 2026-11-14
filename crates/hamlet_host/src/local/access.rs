use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_actor_core::{ActorContext, ActorLifecycle};

use super::{
    Host, HostInner, HostOptions, LocalHandle, Open, host_shut_down,
};
use crate::address::{Address, Caller};
use crate::event::HostEvent;
use crate::link::Vetted;

/// An actor's view of the host that runs it, from its `create` on:
/// the realm folder, the values the program that opened the host gave
/// it, and the other actors.
///
/// Each of [`HostRef::call`], [`HostRef::create`] and
/// [`HostRef::get_or_start`] is made as this actor, stamped with its
/// address. Each fails at once with `actor.stopping` once the actor's
/// generation has ended, also when its key was made again, with
/// `actor.invalid_operation` on the actor's own address, and with
/// `actor.refused` for a kind the route table does not let its kind
/// reach.
///
/// An actor that keeps it never keeps its host open.
#[derive(Clone)]
pub struct HostRef {
    host: Weak<HostInner>,
    me: Address,
    generation: u64,
    folder: PathBuf,
    values: Values,
}

impl HostRef {
    /// The view of the actor whose context is `ctx`. Fails for an
    /// actor that runs in no host of this crate, and once its host is
    /// gone.
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
            host: handle.host.clone(),
            me: handle.at.clone(),
            generation: handle.generation,
            folder: host.dir.clone(),
            values: host.options.values.clone(),
        })
    }

    /// The address of the actor this view belongs to.
    pub fn own_address(&self) -> &Address {
        &self.me
    }

    /// The realm folder the host was opened on.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The value of type `T` the host was given, if one was.
    pub fn value<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.values.get()
    }

    /// Calls the action `name` of the actor at `to` as
    /// [`Host::call`](super::Host::call) does, as this actor.
    pub async fn call(
        &self,
        to: &Address,
        name: &str,
        args: Vec<u8>,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        let (host, stamp) = self.current()?;
        host.call(stamp, to, name, args, deadline).await
    }

    /// Makes the actor at `at` as
    /// [`Host::create`](super::Host::create) does, for this actor.
    /// The actor it makes is not told who made it.
    pub async fn create(
        &self,
        at: &Address,
        input: Vec<u8>,
    ) -> Result<()> {
        let (host, stamp) = self.current()?;
        host.start(stamp, at, Open::New(input)).await
    }

    /// Starts the actor at `at` as
    /// [`Host::get_or_start`](super::Host::get_or_start) does, for
    /// this actor.
    pub async fn get_or_start(&self, at: &Address) -> Result<()> {
        let (host, stamp) = self.current()?;
        host.start(stamp, at, Open::OrCreate).await
    }

    fn current(&self) -> Result<(Arc<HostInner>, Stamp)> {
        let host = self.host.upgrade().ok_or_else(host_shut_down)?;
        if !host.runs(&self.me, self.generation) {
            return Err(ActorLifecycle::Stopping.build().context(
                format!(
                    "generation {} of actor {} has ended",
                    self.generation, self.me
                ),
            ));
        }
        let stamp = Stamp {
            caller: Some(Caller::Actor(self.me.clone())),
            origin: Origin::Here,
        };
        Ok((host, stamp))
    }
}

/// Who an operation is for, as the host stamps it (none for the
/// program, a client, or an actor), and where it entered this host.
/// An actor's stamp is made only by `HostRef::current`, after its
/// generation's check, and by [`Stamp::from_peer`], after the link's.
pub(crate) struct Stamp {
    caller: Option<Caller>,
    origin: Origin,
}

/// Where an operation entered this host.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    /// Here: the program, a client of it, or an actor of this host.
    Here,
    /// In the other host of the link, which this host's link let in.
    /// It runs here or fails: it never crosses the link again.
    Peer,
}

impl Stamp {
    pub(crate) const PROGRAM: Self = Self {
        caller: None,
        origin: Origin::Here,
    };

    pub(crate) fn client() -> Self {
        Self {
            caller: Some(Caller::Client),
            origin: Origin::Here,
        }
    }

    /// The stamp of an operation the other host asked for. Takes only
    /// what the link vetted.
    pub(crate) fn from_peer(from: Vetted) -> Self {
        Self {
            caller: from.into_caller(),
            origin: Origin::Peer,
        }
    }

    pub(crate) fn caller(&self) -> Option<&Caller> {
        self.caller.as_ref()
    }

    pub(crate) fn origin(&self) -> Origin {
        self.origin
    }

    /// The call's connection parameters.
    pub(crate) fn bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        ciborium::into_writer(&self.caller(), &mut bytes)
            .context("encode the caller's stamp")?;
        Ok(bytes)
    }

    pub(super) fn sample_bytes() -> Result<[Vec<u8>; 2]> {
        let actor = Self {
            caller: Some(Caller::Actor(Address::new("k", "k")?)),
            origin: Origin::Here,
        };
        Ok([Self::client().bytes()?, actor.bytes()?])
    }
}

/// The link's view of the host: what an operation the link let in
/// reaches. It holds the host weakly, so the link never keeps the
/// host open, and it names the runtime the host's actors run on,
/// which is where a peer's operation runs.
#[derive(Clone)]
pub(crate) struct Inbound {
    host: Weak<HostInner>,
    runtime: tokio::runtime::Handle,
}

impl Inbound {
    /// Fails outside a tokio runtime.
    pub(crate) fn of(host: &Host) -> Result<Self> {
        Ok(Self {
            host: Arc::downgrade(host.inner()),
            runtime: tokio::runtime::Handle::try_current().context(
                "a host with a link opens inside the tokio runtime \
                 its actors run on",
            )?,
        })
    }

    pub(crate) fn runtime(&self) -> &tokio::runtime::Handle {
        &self.runtime
    }

    /// Runs a call the other host asked for, on the host's one call
    /// path: the route check again, then this host's own actor.
    /// `actor.stopping` once the host is gone.
    pub(crate) async fn call(
        &self,
        from: Vetted,
        to: &Address,
        name: &str,
        args: Vec<u8>,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        let host = self.host.upgrade().ok_or_else(host_shut_down)?;
        host.call(Stamp::from_peer(from), to, name, args, deadline)
            .await
    }

    /// Runs a create the other host asked for, on the host's one
    /// start path.
    pub(crate) async fn create(
        &self,
        from: Vetted,
        at: &Address,
        input: Vec<u8>,
    ) -> Result<()> {
        let host = self.host.upgrade().ok_or_else(host_shut_down)?;
        host.start(Stamp::from_peer(from), at, Open::New(input))
            .await
    }

    /// Passes on an event of the Node host's to this host's
    /// [`Host::events`].
    pub(crate) fn report(&self, event: HostEvent) {
        if let Some(host) = self.host.upgrade() {
            let _ = host.events.send(event);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_stamps_have_fixed_bytes() {
        assert_eq!(
            Stamp::sample_bytes().unwrap(),
            [
                b"\x66client".to_vec(),
                b"\xa1\x65actor\xa2\x64kind\x61k\x63key\x61k"
                    .to_vec()
            ]
        );
    }
}
