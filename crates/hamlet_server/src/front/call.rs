//! The door's calls of the actors for its clients.

use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_actor::action::encode_positional;
use hamlet_host::Host;
use hamlet_messages::Message;
use hamlet_messages::address::Party;

/// How long a call of an actor for a client may take.
const WITHIN: Duration = Duration::from_secs(60);

/// Calls the actors as a client of the program.
#[derive(Clone)]
pub(crate) struct Calls {
    host: Host,
}

impl Calls {
    pub(crate) fn new(host: Host) -> Self {
        Self { host }
    }

    /// Sends `message` to `to` and decodes its answer. A refusal is
    /// in the answer, with its own words.
    pub(crate) async fn call<M: Message>(
        &self,
        to: &M::To,
        message: &M,
    ) -> Result<M::Output> {
        let answer = self
            .host
            .call_for_client(
                &to.address()?,
                M::NAME,
                encode_positional(message)?,
                WITHIN,
            )
            .await?;
        ciborium::from_reader(answer.as_slice()).with_context(|| {
            format!("decode the answer to {}", M::NAME)
        })
    }
}
