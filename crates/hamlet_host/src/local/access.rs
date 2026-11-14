//! What an actor reaches of the host that runs it: the realm folder,
//! the values the program that opened the host gave it, and the other
//! actors, which it calls, creates and starts as itself.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_actor_core::{ActorContext, ActorLifecycle};

use super::{
    HostInner, HostOptions, LocalHandle, Open, host_shut_down,
};
use crate::address::{Address, Caller};

/// An actor's view of the host that runs it, from its `create` on:
/// the realm folder, the values the program that opened the host gave
/// it, and the other actors.
///
/// Each of [`HostRef::call`], [`HostRef::create`] and
/// [`HostRef::get_or_start`] is made as this actor, stamped with its
/// address: an actor reaches the others only as itself. Each fails at
/// once with `actor.stopping` once the actor's generation has ended,
/// also when its key was made again, and with
/// `actor.invalid_operation` on the actor's own address, which would
/// wait for its own handler or its own start.
///
/// It holds the host weakly, so an actor that keeps it never keeps
/// its host open. A clone may go to a task the actor registers: it is
/// checked when the task uses it.
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

    /// The actor at `at`, as
    /// [`Host::get_or_start`](super::Host::get_or_start) answers it,
    /// for this actor.
    pub async fn get_or_start(&self, at: &Address) -> Result<()> {
        let (host, stamp) = self.current()?;
        host.start(stamp, at, Open::OrCreate).await
    }

    /// The host and this actor's stamp, while the host runs this
    /// view's generation. The only place the host makes an actor's
    /// stamp, so one exists only for a generation that is current.
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
        Ok((host, Stamp(Some(Caller::Actor(self.me.clone())))))
    }
}

/// Who an operation is for, as the host stamps it: none for the
/// program, a client, or an actor. An actor's is made only by
/// [`HostRef`], after its generation's check.
pub(crate) struct Stamp(Option<Caller>);

impl Stamp {
    pub(crate) const PROGRAM: Self = Self(None);

    pub(crate) fn client() -> Self {
        Self(Some(Caller::Client))
    }

    pub(crate) fn caller(&self) -> Option<&Caller> {
        self.0.as_ref()
    }

    /// The call's connection parameters: the CBOR of the
    /// `Option<Caller>`, so none is CBOR null.
    pub(crate) fn bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        ciborium::into_writer(&self.caller(), &mut bytes)
            .context("encode the caller's stamp")?;
        Ok(bytes)
    }

    /// The bytes of a client's stamp and of an actor's, as
    /// [`Stamp::bytes`] writes them, for the host's check of each
    /// kind at its open.
    pub(super) fn sample_bytes() -> Result<[Vec<u8>; 2]> {
        let actor =
            Self(Some(Caller::Actor(Address::new("k", "k")?)));
        Ok([Self::client().bytes()?, actor.bytes()?])
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

    /// The bytes other hosts and other languages read: none is CBOR
    /// null, a client the text `client`, and an actor a map from
    /// `actor` to its address; the samples for the check at open are
    /// the same bytes.
    #[test]
    fn a_stamp_has_fixed_bytes() {
        let actor = Stamp(Some(Caller::Actor(
            Address::new("asker", "a").unwrap(),
        )));
        assert_eq!(Stamp::PROGRAM.bytes().unwrap(), [0xf6]);
        assert_eq!(Stamp::client().bytes().unwrap(), b"\x66client");
        assert_eq!(
            actor.bytes().unwrap(),
            b"\xa1\x65actor\xa2\x64kind\x65asker\x63key\x61a"
        );
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
