//! The explicit create of an actor, with its input. Only a create and
//! `get_or_start` make an actor, and only a create gives it an input.

use anyhow::Result;
use hamlet_actor_error::ActorError;

use super::{Host, Open};

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
    /// The actor is made once its file holds its row, before its
    /// first start ends. A create whose start fails returns that
    /// failure and leaves the actor made: a later call or
    /// `get_or_start` starts it, and that start gives the core the
    /// input. A create that waited for another start that failed
    /// fails with that start's error.
    pub async fn create(
        &self,
        kind: &str,
        key: &str,
        input: Vec<u8>,
    ) -> Result<String> {
        let actor_id = self.inner().located(kind, key)?;
        self.inner()
            .ensure_started(&actor_id, &Open::New(input))
            .await?;
        Ok(actor_id)
    }
}
