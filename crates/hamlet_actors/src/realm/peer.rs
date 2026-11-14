//! The one way from the realm to another actor. Nothing else in the
//! realm names the host's calls: a step, a delivery and a push each
//! go through [`Peers`], which the realm keeps from its `create`. A
//! call names its taker as a party of `hamlet_messages::address`, so
//! no line of the realm joins a kind and a key.

#![expect(dead_code, reason = "the scaffold: U3's steps call these")]

use std::time::Duration;

use anyhow::Result;
use hamlet_actor_error::ActorError;
use hamlet_host::HostRef;
use hamlet_messages::Message;
use hamlet_messages::address::Party;
use serde::Serialize;

/// When a call is made, against the answer to the realm's own caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum When {
    /// Before it: only `Say`'s `Submit`. With [`super::HIRING_WAIT`],
    /// its deadline keeps a `Say` inside the 10 s a caller gives.
    BeforeAnswer,
    /// After it, or in a step: as long as the workspace waits.
    AfterAnswer,
}

impl When {
    pub const fn deadline(self) -> Duration {
        match self {
            Self::BeforeAnswer => BEFORE_ANSWER,
            Self::AfterAnswer => Duration::from_secs(10),
        }
    }
}

/// The deadline of a call before the answer.
pub(super) const BEFORE_ANSWER: Duration = Duration::from_secs(4);

/// The host as the realm reaches it. Built once in `Realm::create`
/// from `HostRef::of(ctx.inner())`, and cloned into every task that
/// calls after a handler returned.
#[derive(Clone)]
pub(super) struct Peers {
    host: HostRef,
}

impl Peers {
    pub fn new(host: HostRef) -> Self {
        Self { host }
    }

    /// Calls `to` with `message`, as the realm.
    pub async fn call<M: Message>(
        &self,
        to: &M::To,
        message: &M,
        when: When,
    ) -> Result<M::Output> {
        crate::call(&self.host, to, message, when.deadline()).await
    }

    /// Makes the actor `at` with `input`. A key that holds an actor
    /// counts as made: an earlier run of the same step made it.
    pub async fn create<P: Party>(
        &self,
        at: &P,
        input: &impl Serialize,
    ) -> Result<()> {
        let mut bytes = Vec::new();
        ciborium::into_writer(input, &mut bytes)?;
        match self.host.create(&at.address()?, bytes).await {
            Err(error) if is_duplicate_key(&error) => Ok(()),
            other => other,
        }
    }

    /// The actor `at`, made with no input when it has none. Used for
    /// a profile's actor before its first call.
    pub async fn get_or_start<P: Party>(&self, at: &P) -> Result<()> {
        self.host.get_or_start(&at.address()?).await
    }
}

fn is_duplicate_key(error: &anyhow::Error) -> bool {
    let error = ActorError::extract(error);
    error.group() == "actor" && error.code() == "duplicate_key"
}
