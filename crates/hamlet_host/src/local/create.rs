//! The explicit create of an actor, with its input, and an actor's
//! create and get-or-start of another actor. Only a create and
//! `get_or_start` make an actor, and only a create gives it an input.

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor_core::ActorContext;
use hamlet_actor_error::ActorError;

use super::access::current;
use super::call::{Caller, admit};
use super::{Host, HostInner, Open};
use crate::directory::Address;

/// A create of a key that holds an actor.
#[derive(ActorError)]
#[error("actor", "duplicate_key", "Actor key already exists.")]
pub(super) struct DuplicateKey;

impl Host {
    /// Makes the actor of `kind` and `key` and starts it with
    /// `input`, the bytes its kind reads as its create input: the
    /// CBOR of its `Input` for a kind of the authoring layer. The
    /// input is kept with the actor and given to each start, and the
    /// core reads it only while the actor has no state, so the kind
    /// reads it once. Returns the actor's id.
    ///
    /// Fails with `actor.duplicate_key` and changes nothing for a key
    /// that holds an actor, running, asleep or stopping. Waits first
    /// for a start under way, and for boot's read of the actor's
    /// file. Fails at once with `actor.destroying` for an actor that
    /// is being destroyed; once the destroy ends, a create makes the
    /// actor anew. Fails at once with `actor.shutdown_timeout` for an
    /// actor whose stop missed its deadline. A destroy that a kill
    /// cut off is finished first. So none of the old actor's data
    /// comes back. Fails at once with `actor.not_registered` for a
    /// kind the directory does not hold.
    ///
    /// A create whose own start fails, for an input the kind cannot
    /// read or any other reason, returns that failure and leaves
    /// nothing of the actor: its files are removed, and the key is
    /// free for another create. Only a statement of that start that
    /// still holds the file past the close's grace leaves the actor
    /// made, to start or destroy as any other: the create returns the
    /// start's failure all the same, without waiting for the
    /// statement. Until the statement ends, a start or a create of
    /// the key cannot open the file and fails at its start deadline.
    /// Once it ends, a create fails with `actor.duplicate_key`, and
    /// the next boot starts the actor, since its file stays marked
    /// running. A create that waited
    /// for another create's start that failed then makes the actor
    /// itself; one that waited for any other start that failed fails
    /// with that start's error. A kill during the create's start
    /// leaves the actor made, with its input, which each start gives
    /// the core until one starts it: boot starts it, and tries again
    /// after each start retry for as long as its start fails.
    pub async fn create(
        &self,
        kind: &str,
        key: &str,
        input: Vec<u8>,
    ) -> Result<String> {
        self.inner().start_by_key(kind, key, Open::New(input)).await
    }

    /// Makes the actor of `kind` and `key` as [`Host::create`] does,
    /// for the actor whose context is `ctx`. The actor it makes is
    /// not told who made it. Fails at once with
    /// `actor.invalid_operation` for the actor's own key, which would
    /// wait for its own start, and with `actor.stopping` from a
    /// generation that has ended.
    pub async fn create_from(
        ctx: &ActorContext,
        kind: &str,
        key: &str,
        input: Vec<u8>,
    ) -> Result<String> {
        reach(ctx, "create", kind, key)?
            .start_by_key(kind, key, Open::New(input))
            .await
    }

    /// The actor for `kind` and `key`, as [`Host::get_or_start`]
    /// answers it, for the actor whose context is `ctx`. Fails as
    /// [`Host::create_from`] does.
    pub async fn get_or_start_from(
        ctx: &ActorContext,
        kind: &str,
        key: &str,
    ) -> Result<String> {
        reach(ctx, "get_or_start", kind, key)?
            .start_by_key(kind, key, Open::OrCreate)
            .await
    }
}

impl HostInner {
    /// Starts the actor of `kind` and `key` as `open` lets it, and
    /// returns its id.
    pub(super) async fn start_by_key(
        self: &Arc<Self>,
        kind: &str,
        key: &str,
        open: Open,
    ) -> Result<String> {
        let actor_id = self.located(kind, key)?;
        self.ensure_started(&actor_id, &open).await?;
        Ok(actor_id)
    }
}

/// The host of the actor whose context is `ctx`, for its `operation`
/// of the actor of `kind` and `key`. Fails as [`admit`] does, and
/// from a generation that has ended.
fn reach(
    ctx: &ActorContext,
    operation: &str,
    kind: &str,
    key: &str,
) -> Result<Arc<HostInner>> {
    let from = current(ctx)?;
    admit(
        operation,
        Some(&Caller::Actor(from.address()?)),
        &Address::new(kind, key),
    )?;
    Ok(from.host)
}
