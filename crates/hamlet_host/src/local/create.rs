//! The explicit create of an actor, with its input, and an actor's
//! create and get-or-start of another actor. Only a create and
//! `get_or_start` make an actor, and only a create gives it an input.

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor_core::ActorContext;
use hamlet_actor_error::ActorError;

use super::access::current;
use super::call::admit;
use super::{Host, HostInner, Open, actor_id};
use crate::address::{Address, Caller};
use crate::directory::Place;

/// A create of a key that holds an actor.
#[derive(ActorError)]
#[error("actor", "duplicate_key", "Actor key already exists.")]
pub(super) struct DuplicateKey;

impl Host {
    /// Makes the actor at `at` and starts it with
    /// `input`, the bytes its kind reads as its create input: the
    /// CBOR of its `Input` for a kind of the authoring layer. The
    /// input is kept with the actor and given to each start, and the
    /// core reads it only while the actor has no state, so the kind
    /// reads it once.
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
    /// running. A create that waited for another create's start that
    /// failed then makes the actor itself; one that waited for any
    /// other start that failed fails with that start's error. A kill
    /// during the create's start leaves the actor made, with its
    /// input, which each start gives the core until one starts it:
    /// boot starts it, and tries again after each start retry for as
    /// long as its start fails.
    ///
    /// A start that fails for a reason of the kind's own, an error of
    /// its `create` hook say, fails with `core.internal_error`, for a
    /// create, a call or any start that waited for it. The authoring
    /// layer hands the core only the error's [`ActorError`], and the
    /// message of an error with no code of its own is hidden unless
    /// `HAMLET_ACTOR_EXPOSE_ERRORS=1`. The reason stays in the
    /// error's metadata, as its `error`:
    /// `ActorError::extract(&error).metadata()`. `ActorError`'s
    /// `Serialize` writes that metadata, so code that sends the error
    /// to a client sends the reason with it unless it drops the
    /// metadata first.
    ///
    /// [`ActorError`]: struct@hamlet_actor_error::ActorError
    pub async fn create(
        &self,
        at: &Address,
        input: Vec<u8>,
    ) -> Result<()> {
        self.inner().start(at, Open::New(input)).await
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
        let (host, at) = reach(ctx, "create", kind, key)?;
        host.start(&at, Open::New(input)).await?;
        Ok(actor_id(&at))
    }

    /// The actor for `kind` and `key`, as [`Host::get_or_start`]
    /// answers it, for the actor whose context is `ctx`. Fails as
    /// [`Host::create_from`] does.
    pub async fn get_or_start_from(
        ctx: &ActorContext,
        kind: &str,
        key: &str,
    ) -> Result<String> {
        let (host, at) = reach(ctx, "get_or_start", kind, key)?;
        host.start(&at, Open::OrCreate).await?;
        Ok(actor_id(&at))
    }
}

impl HostInner {
    /// Starts the actor at `at` as `open` lets it.
    pub(super) async fn start(
        self: &Arc<Self>,
        at: &Address,
        open: Open,
    ) -> Result<()> {
        let Place::Local(_) = self.locate(at)?;
        self.ensure_started(at, &open).await.map(drop)
    }
}

/// The host of the actor whose context is `ctx`, and the address of
/// `kind` and `key`, for its `operation` of that actor. Fails as
/// [`admit`] does, and from a generation that has ended.
fn reach(
    ctx: &ActorContext,
    operation: &str,
    kind: &str,
    key: &str,
) -> Result<(Arc<HostInner>, Address)> {
    let from = current(ctx)?;
    let at = Address::new(kind, key)?;
    admit(operation, Some(&Caller::Actor(from.at)), &at)?;
    Ok((from.host, at))
}
