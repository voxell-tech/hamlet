//! Calls to an actor.

use std::time::{Duration, Instant};

use anyhow::Result;
use hamlet_actor_core::host::ActionTimedOut;
use serde::{Deserialize, Serialize};
use tracing::Instrument;

use super::Host;
use crate::cause::Cause;
use crate::directory::Address;

/// Who made a call, as the host stamps it. The callee reads it from
/// its call's connection parameters, which are the CBOR of an
/// `Option<Caller>`: a call that no actor and no client made carries
/// none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Caller {
    /// The actor at this address.
    Actor(Address),
    /// A client of the program that opened the host.
    Client,
}

impl Host {
    /// Calls the action `name` of the actor at `to`, started first if
    /// it is not running, as `get_or_start` starts it. The call ends
    /// within `deadline`, its start included, and the action within
    /// the shorter of what is left and the kind's action timeout:
    /// past either, it fails with `actor.action_timed_out`. Fails at
    /// once with `actor.not_registered` for a kind the directory does
    /// not hold, and with `actor.destroying` for an actor that is
    /// being destroyed. The host's log carries `cause`'s task.
    pub async fn call(
        &self,
        to: &Address,
        name: &str,
        args: Vec<u8>,
        deadline: Duration,
        cause: Cause,
    ) -> Result<Vec<u8>> {
        let actor_id = self.inner().located(&to.kind, &to.key)?;
        let span = tracing::debug_span!(
            "call",
            actor_id,
            action = name,
            task = cause.task()
        );
        async {
            let begun = Instant::now();
            let generation = tokio::time::timeout(
                deadline,
                self.inner().ensure_started(&actor_id, true),
            )
            .await
            .map_err(|_| ActionTimedOut.build())??;
            let left = deadline.saturating_sub(begun.elapsed());
            generation
                .actor
                .action(Vec::new(), name, args, left)
                .await
        }
        .instrument(span)
        .await
    }
}
