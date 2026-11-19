//! The door's calls of the actors for its clients, and what a failed
//! call becomes for the client that asked.

use std::future::Future;
use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_actor::Action;
use hamlet_actor::action::encode_positional;
use hamlet_actor_error::ActorError;
use hamlet_host::{Address, Host, Subscription};
use hamlet_messages::Message;
use hamlet_messages::address::{self, Party};
use hamlet_messages::realm::client::ReadWorld;
use hamlet_messages::realm::world::{Seen, Status};
use hamlet_protocol::client::{RequestId, ServerMessage};
use tokio::time::Instant;

use crate::clients::check::{fault, reply};
use crate::society::refusal::Refusal;

/// How long a call of an actor for a client may take: longer than
/// the realm takes to answer a line, at most 8 s, and than a step of
/// a sign-in waits for its provider, at most 25 s.
const WITHIN: Duration = Duration::from_secs(60);

/// The time between tries of a call to an agent being made.
const AGAIN: Duration = Duration::from_millis(50);

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
            .send(
                &to.address()?,
                M::NAME,
                encode_positional(message)?,
            )
            .await?;
        ciborium::from_reader(answer.as_slice()).with_context(|| {
            format!("decode the answer to {}", M::NAME)
        })
    }

    /// Calls the action `name` of the actor at `to` with `args`. An
    /// agent the realm shows that the host does not have yet, or has
    /// not started, is called again until the call's deadline, as
    /// while it is being hired; one the realm does not show is
    /// refused as no one.
    pub(crate) async fn send(
        &self,
        to: &Address,
        name: &str,
        args: Vec<u8>,
    ) -> Result<Vec<u8>> {
        self.again(to, |left| {
            self.host.call_for_client(to, name, args.clone(), left)
        })
        .await
    }

    /// Subscribes to `event` of the actor at `to`, as a client, with
    /// the tries of [`Calls::send`].
    pub(crate) async fn subscribe(
        &self,
        to: &Address,
        event: &str,
    ) -> Result<Subscription> {
        self.again(to, |_| self.host.subscribe_for_client(to, event))
            .await
    }

    /// Runs `attempt` with the time left until the deadline, again
    /// while it fails because an agent the realm shows is not there
    /// or not started yet.
    async fn again<T, F>(
        &self,
        to: &Address,
        mut attempt: impl FnMut(Duration) -> F,
    ) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        let end = Instant::now() + WITHIN;
        let mut shown = None;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match attempt(left).await {
                Err(error) if starting(&error) && left > AGAIN => {}
                done => return done,
            }
            let shown = match shown {
                Some(shown) => shown,
                None => *shown.insert(self.shows(to).await?),
            };
            if !shown {
                return Err(Refusal::NoOne {
                    address: to.key().to_owned(),
                }
                .into());
            }
            tokio::time::sleep(AGAIN).await;
        }
    }

    /// Whether the realm shows the agent at `to` living or being
    /// hired.
    async fn shows(&self, to: &Address) -> Result<bool> {
        if to.kind() != address::Agent::KIND {
            return Ok(false);
        }
        let answer = self
            .host
            .call_for_client(
                &address::Realm.address()?,
                ReadWorld::NAME,
                encode_positional(&ReadWorld)?,
                WITHIN,
            )
            .await?;
        let seen: Seen = ciborium::from_reader(answer.as_slice())?;
        Ok(seen.agents.iter().any(|agent| {
            agent.agent.0.to_string() == to.key()
                && !matches!(agent.status, Status::Ended { .. })
        }))
    }
}

/// Whether `error` says the actor called is not there or not started
/// yet.
fn starting(error: &anyhow::Error) -> bool {
    actor_error(error).is_some_and(|found| {
        found.group() == "actor"
            && matches!(found.code(), "not_found" | "not_ready")
    })
}

fn actor_error(error: &anyhow::Error) -> Option<&ActorError> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<ActorError>())
}

/// The words of a call refused by failing it, as an agent refuses a
/// page of a chat that is not there.
pub(crate) fn refused_words(error: &anyhow::Error) -> Option<String> {
    if let Some(refusal) = error.downcast_ref::<Refusal>() {
        return Some(refusal.to_string());
    }
    actor_error(error)
        .filter(|found| {
            found.group() == "user" && found.code() == "refused"
        })
        .map(|found| found.message().to_owned())
}

/// The answer to request `to` whose call failed with `error`: the
/// refusal's words, or a fault of the server.
pub(crate) fn failed(
    to: &RequestId,
    error: &anyhow::Error,
) -> ServerMessage {
    match refused_words(error) {
        Some(words) => reply(to, Err(Refusal::Usage { words })),
        None => server_failed(Some(to), error),
    }
}

/// The fault of a request the server failed.
pub(crate) fn server_failed(
    to: Option<&RequestId>,
    error: &anyhow::Error,
) -> ServerMessage {
    fault(to, failure(error))
}

/// The words of a failure of the server.
pub(crate) fn failure(error: &anyhow::Error) -> String {
    format!("The server failed: {error}")
}
