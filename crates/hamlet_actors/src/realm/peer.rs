//! The one way from the realm to another actor.

use std::time::Duration;

use anyhow::Result;
use hamlet_actor_error::ActorError;
use hamlet_host::HostRef;
use hamlet_messages::Message;
use hamlet_messages::address::Party;
use serde::Serialize;

use crate::cbor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum When {
    BeforeAnswer,
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

pub(super) const BEFORE_ANSWER: Duration = Duration::from_secs(4);

#[derive(Clone)]
pub(super) struct Peers {
    host: HostRef,
}

impl Peers {
    pub fn new(host: HostRef) -> Self {
        Self { host }
    }

    pub async fn call<M: Message>(
        &self,
        to: &M::To,
        message: &M,
        when: When,
    ) -> Result<M::Output> {
        crate::call(&self.host, to, message, when.deadline())
            .await
            .map_err(marked)
    }

    pub async fn create<P: Party>(
        &self,
        at: &P,
        input: &impl Serialize,
    ) -> Result<()> {
        match self.host.create(&at.address()?, cbor(input)?).await {
            Err(error) if is_duplicate_key(&error) => Ok(()),
            other => other.map_err(marked),
        }
    }

    pub async fn get_or_start<P: Party>(&self, at: &P) -> Result<()> {
        self.host.get_or_start(&at.address()?).await
    }
}

/// The mark of a callee's own error in answer to a call of the
/// realm's.
#[derive(Debug)]
struct Refused;

impl std::fmt::Display for Refused {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str("the callee refused the call")
    }
}

/// Marks `error` as the callee's own unless it says the callee was
/// not reached: an actor starting or stopping, a call past its
/// deadline, a link that is down or a database that is closed.
fn marked(error: anyhow::Error) -> anyhow::Error {
    let answer = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<ActorError>());
    match answer {
        Some(answer)
            if !matches!(
                answer.group(),
                "actor" | "link" | "sqlite"
            ) =>
        {
            error.context(Refused)
        }
        _ => error,
    }
}

/// The refusal `why` a callee answered, as an error of its own.
pub(super) fn refusal(why: impl std::fmt::Display) -> anyhow::Error {
    anyhow::Error::msg(why.to_string()).context(Refused)
}

/// Whether `error` is a callee's own answer to a call of the
/// realm's.
pub(super) fn refused(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Refused>().is_some()
}

fn is_duplicate_key(error: &anyhow::Error) -> bool {
    let error = ActorError::extract(error);
    error.group() == "actor" && error.code() == "duplicate_key"
}

#[cfg(test)]
mod tests {
    use hamlet_actor_core::ActorLifecycle;
    use hamlet_messages::sender::wrong_caller;

    use super::*;

    #[test]
    fn only_a_callees_own_error_is_marked_refused() {
        let own = marked(wrong_caller("submit", None));
        let starting = marked(ActorLifecycle::NotReady.build());
        let realms =
            marked(anyhow::anyhow!("a query of the realm's"));

        assert!(refused(&own));
        assert!(!refused(&starting));
        assert!(!refused(&realms));
    }
}
