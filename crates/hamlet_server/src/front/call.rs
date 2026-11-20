//! The door's calls of the actors for its clients, and what a failed
//! call becomes for the client that asked.

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hamlet_actor::Action;
use hamlet_actor::action::encode_positional;
use hamlet_actor_error::ActorError;
use hamlet_host::{Address, Host, Subscription};
use hamlet_messages::Message;
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::chat::{Stop, Stopped};
use hamlet_messages::realm::client::{ReadWorld, Say, SayAnswer};
use hamlet_messages::realm::world::{Seen, Status};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{
    self as protocol, RequestId, ServerMessage,
};
use tokio::sync::watch;
use tokio::time::Instant;

use super::frames;
use super::route::number;
use crate::clients::check::{fault, reply};
use crate::society::refusal::Refusal;

/// How long a call of an actor for a client may take: longer than
/// the realm takes to answer a line, at most 8 s, and than a step of
/// a sign-in waits for its provider, at most 25 s.
const WITHIN: Duration = Duration::from_secs(60);

/// The time between tries of a call to an agent being made, and of
/// a line the realm did not take.
const AGAIN: Duration = Duration::from_millis(50);

/// Calls the actors as a client.
#[derive(Clone)]
pub(crate) struct Calls {
    host: Host,
    /// Where the request these calls are for stands among its
    /// connection's stops; none for the door's own calls.
    place: Option<Place>,
}

/// What became of a chief's line given to the realm.
pub(crate) enum Given {
    Taken,
    /// A stop its connection sent after it came before the agent
    /// took it.
    Dropped,
    Refused(String),
}

/// The stops one connection sent. A stop ends the waits of the
/// requests its connection sent before it.
#[derive(Clone)]
pub(crate) struct Stops(Arc<Sent>);

struct Sent {
    count: watch::Sender<u64>,
    /// Each stop's number, with an agent it dropped a line to.
    dropped: Mutex<BTreeSet<(u64, AgentNumber)>>,
}

impl Stops {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Sent {
            count: watch::Sender::new(0),
            dropped: Mutex::new(BTreeSet::new()),
        }))
    }

    /// The place of a request received now.
    pub(crate) fn place(&self) -> Place {
        Place {
            stops: self.clone(),
            after: *self.0.count.borrow(),
        }
    }

    /// Counts a stop received now.
    pub(crate) fn stop(&self) {
        self.0.count.send_modify(|count| *count += 1);
    }

    fn dropped(
        &self,
    ) -> MutexGuard<'_, BTreeSet<(u64, AgentNumber)>> {
        self.0
            .dropped
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// A request's place among its connection's stops: how many came
/// before it.
#[derive(Clone)]
pub(crate) struct Place {
    stops: Stops,
    after: u64,
}

impl Place {
    async fn stopped(&self) {
        let mut count = self.stops.0.count.subscribe();
        let _ = count.wait_for(|count| *count > self.after).await;
    }
}

impl Calls {
    pub(crate) fn new(host: Host) -> Self {
        Self { host, place: None }
    }

    /// These calls, for a request at `place`.
    pub(crate) fn at(&self, place: Place) -> Self {
        Self {
            host: self.host.clone(),
            place: Some(place),
        }
    }

    /// Resolves once the connection sent a stop after the request
    /// these calls are for.
    pub(crate) async fn stopped(&self) {
        match &self.place {
            Some(place) => place.stopped().await,
            None => std::future::pending().await,
        }
    }

    /// Keeps that the stop after the request these calls are for
    /// dropped its line to `agent`.
    pub(crate) fn drop_line(&self, agent: AgentNumber) {
        if let Some(place) = &self.place {
            place.stops.dropped().insert((place.after + 1, agent));
        }
    }

    /// The turn of `agent` the stop these calls are for ended by
    /// dropping a line its connection sent before it: one or none.
    pub(crate) fn dropped(&self, agent: AgentNumber) -> u32 {
        self.place.as_ref().map_or(0, |place| {
            u32::from(
                place.stops.dropped().remove(&(place.after, agent)),
            )
        })
    }

    /// Waits before a call's next try; `false` when a stop its
    /// connection sent after the request came first.
    async fn pause(&self) -> bool {
        tokio::select! {
            () = tokio::time::sleep(AGAIN) => true,
            () = self.stopped() => false,
        }
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

    /// Gives the realm the chief's line `say`, sent again with its
    /// request id until the call's deadline while the realm does not
    /// take it, as when the agent is cut off, and dropped when a stop
    /// its connection sent after it comes first.
    pub(crate) async fn say(
        &self,
        say: protocol::Say,
    ) -> Result<Given> {
        let agent = number(say.to);
        let realm = address::Realm.address()?;
        let args = encode_positional(&Say(say))?;
        let end = Instant::now() + WITHIN;
        loop {
            let answer =
                self.send(&realm, Say::NAME, args.clone()).await?;
            if let Some(words) = frames::refusal_words(&answer)? {
                return Ok(Given::Refused(words));
            }
            let taken: SayAnswer =
                ciborium::from_reader(answer.as_slice())?;
            if !matches!(taken, SayAnswer::NotTaken { .. }) {
                return Ok(Given::Taken);
            }
            if Instant::now() >= end {
                bail!(
                    "the realm did not take the line within {} s",
                    WITHIN.as_secs()
                );
            }
            if !self.pause().await {
                if let Some(agent) = agent {
                    self.drop_line(agent);
                }
                return Ok(Given::Dropped);
            }
        }
    }

    /// Stops the running turns of `agent`, giving how many, a line
    /// the stop dropped counted as one. It is tried once: an agent
    /// the host cannot reach now, as while it is being hired or its
    /// Node host starts again, runs no turn.
    pub(crate) async fn stop(
        &self,
        agent: AgentNumber,
        stop: protocol::Stop,
    ) -> Result<u32> {
        let answer = self
            .host
            .call_for_client(
                &address::Agent(agent).address()?,
                Stop::NAME,
                encode_positional(&Stop(stop))?,
                WITHIN,
            )
            .await;
        let turns = match answer {
            Ok(answer) => {
                ciborium::from_reader::<Stopped, _>(
                    answer.as_slice(),
                )?
                .turns
            }
            Err(error) if starting(&error) || lost(&error) => 0,
            Err(error) => return Err(error),
        };
        Ok(turns.max(self.dropped(agent)))
    }

    /// Calls the action `name` of the actor at `to` with `args`. An
    /// agent the realm shows that the host does not have yet, or has
    /// not started, or whose Node host is not up, is called again
    /// until the call's deadline, as while it is being hired; one the
    /// realm does not show is refused as no one.
    pub(crate) async fn send(
        &self,
        to: &Address,
        name: &str,
        args: Vec<u8>,
    ) -> Result<Vec<u8>> {
        self.again(to, starting, |left| {
            self.host.call_for_client(to, name, args.clone(), left)
        })
        .await
    }

    /// Subscribes to `event` of the actor at `to`, as a client, with
    /// the tries of [`Calls::send`], and again when the Node host
    /// went down before it was answered.
    pub(crate) async fn subscribe(
        &self,
        to: &Address,
        event: &str,
    ) -> Result<Subscription> {
        let retried =
            |error: &anyhow::Error| starting(error) || lost(error);
        self.again(to, retried, |_| {
            self.host.subscribe_for_client(to, event)
        })
        .await
    }

    /// Runs `attempt` with the time left until the deadline, again
    /// while it fails as `retried` says, the realm shows the agent at
    /// `to` and no stop its connection sent after the request came.
    async fn again<T, F>(
        &self,
        to: &Address,
        retried: impl Fn(&anyhow::Error) -> bool,
        mut attempt: impl FnMut(Duration) -> F,
    ) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        let end = Instant::now() + WITHIN;
        let mut shown = None;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            let error = match attempt(left).await {
                Err(error) if retried(&error) && left > AGAIN => {
                    error
                }
                done => return done,
            };
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
            if !self.pause().await {
                return Err(error);
            }
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
/// yet, or its Node host is not up: the call was not made.
fn starting(error: &anyhow::Error) -> bool {
    actor_error(error).is_some_and(|found| {
        matches!(
            (found.group(), found.code()),
            ("actor", "not_found" | "not_ready") | ("link", "down")
        )
    })
}

/// Whether `error` says the Node host went down before the call was
/// answered.
fn lost(error: &anyhow::Error) -> bool {
    actor_error(error).is_some_and(|found| {
        (found.group(), found.code()) == ("link", "lost")
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
