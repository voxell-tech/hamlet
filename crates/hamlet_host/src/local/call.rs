//! Calls to an actor: from the program that opened the host, for a
//! client, and from another actor. The host stamps each call with its
//! caller, and the callee reads the stamp from its call's connection.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use hamlet_actor_core::ActorFactory;
use hamlet_actor_core::error::ActorRuntime;
use hamlet_actor_core::host::ActionTimedOut;
use hamlet_actor_error::ActorError;
use serde::Serialize;
use tracing::Instrument;

use super::access::Stamp;
use super::{Host, HostInner, Open, actor_id, host_shut_down};
use crate::address::{Address, Caller};
use crate::directory::Place;
use crate::event::HostEvent;
use crate::route::{CallerKind, Operation};

/// An operation the route table does not let its caller make.
#[derive(ActorError, Serialize)]
#[error(
    "actor",
    "refused",
    "Actor operation is refused.",
    "Actor operation '{operation}' is refused: {from} may not reach \
     the kind '{to}'."
)]
struct Refused {
    operation: String,
    from: CallerKind,
    to: String,
}

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
        self.inner()
            .call(Stamp::PROGRAM, to, name, args, deadline)
            .await
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
            .call(Stamp::client(), to, name, args, deadline)
            .await
    }
}

impl HostInner {
    /// Calls the action `name` of the actor at `to` for `stamp`,
    /// stamped on the call's connection, as [`Host::call`] says.
    pub(super) async fn call(
        self: &Arc<Self>,
        stamp: Stamp,
        to: &Address,
        name: &str,
        args: Vec<u8>,
        deadline: Duration,
    ) -> Result<Vec<u8>> {
        self.admit(
            Operation::Call {
                action: name.to_owned(),
            },
            stamp.caller(),
            to,
        )?;
        let Place::Local(_) = self.locate(to)?;
        let params = stamp.bytes()?;
        let span = tracing::debug_span!(
            "call",
            actor_id = actor_id(to).as_str(),
            action = name,
            caller = stamp.caller().map(tracing::field::display)
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

    /// Refuses an actor's `operation` of itself, which would wait for
    /// its own handler or its own start, and reports and refuses an
    /// operation the route table does not let its caller make.
    pub(super) fn admit(
        &self,
        operation: Operation,
        caller: Option<&Caller>,
        to: &Address,
    ) -> Result<()> {
        let Some(from) = caller else {
            return Ok(());
        };
        if let Caller::Actor(actor) = from
            && actor == to
        {
            return Err(ActorRuntime::InvalidOperation {
                operation: operation.to_string(),
                reason: format!("the actor {to} is its own target"),
            }
            .build());
        }
        if self.options.routes.allows(from, to.kind()) {
            return Ok(());
        }
        tracing::warn!(
            caller = %from,
            actor_id = actor_id(to).as_str(),
            ?operation,
            "the host refused an operation its route table does not \
             allow"
        );
        let refused = Refused {
            operation: operation.to_string(),
            from: CallerKind::of(from),
            to: to.kind().to_owned(),
        }
        .build();
        let _ = self.events.send(HostEvent::Refused {
            from: from.clone(),
            to: to.clone(),
            operation,
        });
        Err(refused)
    }
}

/// Fails for a kind that cannot read the stamp of an actor's call or
/// a client's, naming it. A kind decodes its calls' connection
/// parameters as it declared them, and one that cannot decode a stamp
/// would refuse every call that carries one.
pub(super) fn check_kinds(
    factories: &HashMap<String, ActorFactory>,
) -> Result<()> {
    let stamps = Stamp::sample_bytes()?;
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
