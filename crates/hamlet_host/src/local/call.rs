//! Calls to an actor: from the program that opened the host, for a
//! client, and from another actor. The host stamps each call with its
//! caller, and the callee reads the stamp from its call's connection.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use hamlet_actor_core::error::ActorRuntime;
use hamlet_actor_core::host::ActionTimedOut;
use hamlet_actor_core::{ActorContext, ActorFactory};
use tracing::Instrument;

use super::access::current;
use super::{Host, HostInner, Open, actor_id, host_shut_down};
use crate::address::{Address, Caller};
use crate::directory::Place;

impl Host {
    /// Calls the action `name` of the actor at `to`, with no caller,
    /// started from its file first if it is not running. A call never
    /// makes an actor: for a key with no actor it fails with
    /// `actor.not_found` and leaves no file. Only `get_or_start` and
    /// [`Host::create`] make one. The call ends within `deadline`,
    /// its start and its connection's opening included, whatever the
    /// actor is doing, and the action within the shorter of what is
    /// left and the kind's action timeout: past either, it fails
    /// with `actor.action_timed_out`. Fails at once with
    /// `actor.not_registered` for a kind the directory does not hold,
    /// with `actor.destroying` for an actor that is being destroyed,
    /// and with `actor.shutdown_timeout` for one whose stop missed
    /// its deadline. The host logs each call at DEBUG with its
    /// caller, its target and its action.
    ///
    /// When the host begins to shut down, the call fails at once with
    /// `actor.stopping`, also when its action already runs. Its
    /// outcome is then unknown: the action may have run in part, or
    /// in full. So it is when a kill cuts a call off, and nothing
    /// calls again.
    pub async fn call(
        &self,
        to: &Address,
        name: &str,
        args: Vec<u8>,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        self.inner().call(None, to, name, args, deadline).await
    }

    /// Calls as [`Host::call`] does, stamped with
    /// [`Caller::Client`]: for a call a client of the program asked
    /// for.
    pub async fn call_for_client(
        &self,
        to: &Address,
        name: &str,
        args: Vec<u8>,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        self.inner()
            .call(Some(Caller::Client), to, name, args, deadline)
            .await
    }

    /// Calls as [`Host::call`] does, from the actor whose context is
    /// `ctx`, stamped with its address: an actor calls only as
    /// itself. Fails at once with `actor.invalid_operation` for a
    /// call to itself, which would wait for its own handler, and with
    /// `actor.stopping` from a generation that has ended.
    pub async fn call_from(
        ctx: &ActorContext,
        to: &Address,
        name: &str,
        args: Vec<u8>,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        let running = current(ctx)?;
        running
            .host
            .call(
                Some(Caller::Actor(running.at)),
                to,
                name,
                args,
                deadline,
            )
            .await
    }
}

impl HostInner {
    /// Calls the action `name` of the actor at `to` for `caller`,
    /// stamped on the call's connection, as [`Host::call`] says.
    async fn call(
        self: &Arc<Self>,
        caller: Option<Caller>,
        to: &Address,
        name: &str,
        args: Vec<u8>,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        admit("call", caller.as_ref(), to)?;
        let Place::Local(_) = self.locate(to)?;
        let params = stamp(caller.as_ref())?;
        let span = tracing::debug_span!(
            "call",
            actor_id = actor_id(to).as_str(),
            action = name,
            caller = caller.as_ref().map(tracing::field::display)
        );
        let mut shut = self.starts_end.subscribe();
        let call = async {
            let begun = Instant::now();
            let generation = tokio::time::timeout(
                deadline,
                self.ensure_started(to, &Open::Existing),
            )
            .await
            .map_err(|_| ActionTimedOut.build())??;
            let left = deadline.saturating_sub(begun.elapsed());
            generation.actor.action(params, name, args, left).await
        };
        async {
            tokio::select! {
                called = call => called,
                _ = shut.wait_for(Option::is_some) => {
                    Err(host_shut_down())
                }
            }
        }
        .instrument(span)
        .await
    }
}

/// Refuses an actor's `operation` of itself, which would wait for its
/// own handler or its own start.
pub(super) fn admit(
    operation: &str,
    caller: Option<&Caller>,
    to: &Address,
) -> Result<()> {
    match caller {
        Some(Caller::Actor(from)) if from == to => {
            Err(ActorRuntime::InvalidOperation {
                operation: operation.to_owned(),
                reason: format!("the actor {to} is its own target"),
            }
            .build())
        }
        _ => Ok(()),
    }
}

/// The connection parameters that stamp a call with `caller`: the
/// CBOR of the `Option<Caller>`, so none is CBOR null.
fn stamp(caller: Option<&Caller>) -> Result<Vec<u8>> {
    let mut params = Vec::new();
    ciborium::into_writer(&caller, &mut params)
        .context("encode the caller's stamp")?;
    Ok(params)
}

/// Fails for a kind that cannot read the stamp of an actor's call or
/// a client's, naming it. A kind decodes its calls' connection
/// parameters as it declared them, and one that cannot decode a stamp
/// would refuse every call that carries one.
pub(super) fn check_kinds(
    factories: &HashMap<String, ActorFactory>,
) -> Result<()> {
    let stamps =
        [Caller::Client, Caller::Actor(Address::new("k", "k")?)]
            .iter()
            .map(|caller| stamp(Some(caller)))
            .collect::<Result<Vec<_>>>()?;
    for (kind, factory) in factories {
        for stamp in &stamps {
            factory.check_conn_params(stamp).with_context(|| {
                format!(
                    "the kind `{kind}` cannot read who calls it: \
                     declare `type ConnParams = \
                     Option<hamlet_host::Caller>`"
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sample of an actor's stamp that `check_kinds` gives each
    /// kind has the bytes of a stamp the host writes.
    #[test]
    fn the_sample_stamp_has_fixed_bytes() {
        let sample = Caller::Actor(Address::new("k", "k").unwrap());
        assert_eq!(
            stamp(Some(&sample)).unwrap(),
            b"\xa1\x65actor\xa2\x64kind\x61k\x63key\x61k"
        );
    }
}
