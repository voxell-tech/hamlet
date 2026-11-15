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
        crate::call(&self.host, to, message, when.deadline()).await
    }

    pub async fn create<P: Party>(
        &self,
        at: &P,
        input: &impl Serialize,
    ) -> Result<()> {
        match self.host.create(&at.address()?, cbor(input)?).await {
            Err(error) if is_duplicate_key(&error) => Ok(()),
            other => other,
        }
    }

    pub async fn get_or_start<P: Party>(&self, at: &P) -> Result<()> {
        self.host.get_or_start(&at.address()?).await
    }
}

fn is_duplicate_key(error: &anyhow::Error) -> bool {
    let error = ActorError::extract(error);
    error.group() == "actor" && error.code() == "duplicate_key"
}
