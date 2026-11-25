//! The door's calls of the actors for its clients, and what a failed
//! call becomes for the client that asked.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_actor::Action;
use hamlet_actor::action::encode_positional;
use hamlet_actor_error::ActorError;
use hamlet_host::{Address, Host, Subscription};
use hamlet_messages::Message;
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::chat::{
    NewSession, Resume, Retry, Rewind,
};
use hamlet_messages::realm::client::{
    ReadWorld, Say, SayAnswer, SayRefusal,
};
use hamlet_messages::realm::lines::{LineIs, ReadLine, StopAgents};
use hamlet_messages::realm::world::{Seen, Status};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{
    self as protocol, RequestId, ServerMessage,
};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::clients::check::{fault, reply};
use crate::society::refusal::Refusal;

/// How long a call of an actor for a client may take: longer than
/// the realm takes to answer a line, at most 8 s, and than a step of
/// a sign-in waits for its provider, at most 25 s.
const WITHIN: Duration = Duration::from_secs(60);

/// The time between tries of a call to an agent being made.
const AGAIN: Duration = Duration::from_millis(50);

/// The wait before the second read of what became of a line; each
/// wait after doubles it, up to [`READ_AT_MOST`].
const FIRST_READ: Duration = Duration::from_millis(10);

const READ_AT_MOST: Duration = Duration::from_secs(1);

const CHOOSE_SESSION: [&str; 4] =
    [NewSession::NAME, Resume::NAME, Retry::NAME, Rewind::NAME];

/// Calls the actors as a client.
#[derive(Clone)]
pub(crate) struct Calls {
    host: Host,
    /// Where the request these calls are for stands among its
    /// connection's stops; none for calls no stop ends.
    place: Option<Place>,
    /// How many stops of each agent the door's clients have had
    /// answered.
    stopped: Arc<watch::Sender<BTreeMap<AgentNumber, u64>>>,
}

/// What the realm did with a chief's line.
pub(crate) enum Said {
    /// It keeps the line for its agent.
    Accepted,
    /// Its agent has left, in the realm's words.
    Left(String),
    Refused(String),
}

/// What became of a line the realm accepted.
pub(crate) enum Fate {
    Given,
    Withdrawn,
}

/// The agents a stop stops, as far as the door knows them when the
/// stop is received.
pub(crate) enum Stopping {
    Agents(Vec<AgentNumber>),
    /// Every agent: a stop of all of them, or of one it names by a
    /// name only the realm's view gives the number of.
    Every,
}

impl Stopping {
    fn stops(&self, agent: AgentNumber) -> bool {
        match self {
            Self::Agents(agents) => agents.contains(&agent),
            Self::Every => true,
        }
    }
}

/// What one connection sent: its stops, and its lines the realm
/// accepted that are not yet given. A stop ends the retry waits of
/// the calls to its agents of the requests its connection sent
/// before it.
#[derive(Clone)]
pub(crate) struct Sent(Arc<Record>);

struct Record {
    /// What each stop stops, in the order sent.
    stops: watch::Sender<Vec<Stopping>>,
    answered: watch::Sender<usize>,
    /// How many of the connection's lines to each agent the door has
    /// not yet read as given or withdrawn.
    lines: watch::Sender<BTreeMap<AgentNumber, usize>>,
}

/// A line its connection said to `agent`, counted as unsettled until
/// dropped.
pub(crate) struct Unsettled {
    sent: Sent,
    agent: AgentNumber,
}

impl Drop for Unsettled {
    fn drop(&mut self) {
        self.sent.0.lines.send_modify(|lines| {
            if let Some(count) = lines.get_mut(&self.agent) {
                *count -= 1;
                if *count == 0 {
                    lines.remove(&self.agent);
                }
            }
        });
    }
}

impl Sent {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Record {
            stops: watch::Sender::new(Vec::new()),
            answered: watch::Sender::new(0),
            lines: watch::Sender::new(BTreeMap::new()),
        }))
    }

    /// The place of a request received now.
    pub(crate) fn place(&self) -> Place {
        Place {
            sent: self.clone(),
            after: self.0.stops.borrow().len(),
        }
    }

    /// Keeps a stop of `stopping` received now, giving its place:
    /// after itself.
    pub(crate) fn stop(&self, stopping: Stopping) -> Place {
        self.0.stops.send_modify(|stops| stops.push(stopping));
        self.place()
    }

    /// Counts the oldest stop not answered as answered.
    pub(crate) fn answered(&self) {
        self.0.answered.send_modify(|answered| *answered += 1);
    }
}

/// A request's place among its connection's stops: how many came
/// before it.
#[derive(Clone)]
pub(crate) struct Place {
    sent: Sent,
    after: usize,
}

impl Place {
    /// Resolves once the connection sent a stop of `agent` after the
    /// request.
    async fn stopping(&self, agent: AgentNumber) {
        let mut stops = self.sent.0.stops.subscribe();
        let _ = stops
            .wait_for(|stops| {
                stops[self.after..]
                    .iter()
                    .any(|stop| stop.stops(agent))
            })
            .await;
    }

    async fn lines_settled(&self, agent: AgentNumber) {
        let mut lines = self.sent.0.lines.subscribe();
        let _ =
            lines.wait_for(|lines| !lines.contains_key(&agent)).await;
    }

    /// Resolves once each stop the connection sent before the request
    /// is answered.
    pub(crate) async fn stops_answered(&self) {
        let mut answered = self.sent.0.answered.subscribe();
        let _ = answered
            .wait_for(|answered| *answered >= self.after)
            .await;
    }
}

impl Calls {
    pub(crate) fn new(host: Host) -> Self {
        Self {
            host,
            place: None,
            stopped: Arc::new(watch::Sender::new(BTreeMap::new())),
        }
    }

    /// These calls, for a request at `place`.
    pub(crate) fn at(&self, place: Place) -> Self {
        Self {
            place: Some(place),
            ..self.clone()
        }
    }

    /// Counts a line to `agent` until the result is dropped; `None`
    /// for calls of no connection.
    pub(crate) fn unsettled(
        &self,
        agent: AgentNumber,
    ) -> Option<Unsettled> {
        let place = self.place.as_ref()?;
        place.sent.0.lines.send_modify(|lines| {
            *lines.entry(agent).or_default() += 1;
        });
        Some(Unsettled {
            sent: place.sent.clone(),
            agent,
        })
    }

    /// Waits before the next try of a call to `agent`; `false` when a
    /// stop of `agent` its connection sent after the request came
    /// first.
    async fn pause(&self, agent: AgentNumber) -> bool {
        let stopping = async {
            match &self.place {
                Some(place) => place.stopping(agent).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = tokio::time::sleep(AGAIN) => true,
            () = stopping => false,
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

    /// Gives the realm the chief's line `say`.
    pub(crate) async fn say(
        &self,
        say: protocol::Say,
    ) -> Result<Said> {
        Ok(match self.call(&address::Realm, &Say(say)).await? {
            SayAnswer::Accepted { .. } => Said::Accepted,
            SayAnswer::Refused {
                why: why @ SayRefusal::Left { .. },
            } => Said::Left(why.to_string()),
            SayAnswer::Refused { why } => {
                Said::Refused(why.to_string())
            }
        })
    }

    /// What became of the line `id` to `agent` that the realm
    /// accepted: read at once, again after waits from [`FIRST_READ`]
    /// doubling to [`READ_AT_MOST`], and at once when a stop of
    /// `agent` is answered.
    pub(crate) async fn given(
        &self,
        agent: AgentNumber,
        id: &RequestId,
    ) -> Result<Fate> {
        let mut stopped = self.stopped.subscribe();
        let mut wait = FIRST_READ;
        loop {
            let stops = stopped.borrow().get(&agent).copied();
            match self
                .call(&address::Realm, &ReadLine(id.clone()))
                .await?
            {
                LineIs::Given => return Ok(Fate::Given),
                LineIs::Withdrawn => return Ok(Fate::Withdrawn),
                LineIs::Waiting => {}
            }
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                _ = stopped.wait_for(|stopped| {
                    stopped.get(&agent).copied() != stops
                }) => {}
            }
            wait = (wait * 2).min(READ_AT_MOST);
        }
    }

    /// Stops `agents` through the realm in one act, giving the turns
    /// stopped.
    pub(crate) async fn stop(
        &self,
        agents: &[AgentNumber],
    ) -> Result<u32> {
        let stopped = self
            .call(
                &address::Realm,
                &StopAgents {
                    agents: agents.to_vec(),
                },
            )
            .await?;
        self.stopped.send_modify(|stopped| {
            for agent in agents {
                *stopped.entry(*agent).or_default() += 1;
            }
        });
        Ok(stopped.turns)
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
        if CHOOSE_SESSION.contains(&name) {
            self.after_earlier_lines(to).await;
        }
        self.again(to, starting, |left| {
            self.host.call_for_client(to, name, args.clone(), left)
        })
        .await
    }

    async fn after_earlier_lines(&self, to: &Address) {
        let (Some(place), Some(agent)) = (&self.place, agent_of(to))
        else {
            return;
        };
        let _ = tokio::time::timeout(WITHIN, async {
            tokio::select! {
                () = place.lines_settled(agent) => {}
                () = place.stopping(agent) => {}
            }
        })
        .await;
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
    /// `to` and no stop of it its connection sent after the request
    /// came.
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
                None => *shown.insert(self.shown(to).await?),
            };
            let Some(agent) = shown else {
                return Err(Refusal::NoOne {
                    address: to.key().to_owned(),
                }
                .into());
            };
            if !self.pause(agent).await {
                return Err(error);
            }
        }
    }

    /// The agent at `to`, when the realm shows it living or being
    /// hired.
    async fn shown(
        &self,
        to: &Address,
    ) -> Result<Option<AgentNumber>> {
        let Some(number) = agent_of(to) else {
            return Ok(None);
        };
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
        Ok(seen
            .agents
            .iter()
            .any(|agent| {
                agent.agent == number
                    && !matches!(agent.status, Status::Ended { .. })
            })
            .then_some(number))
    }
}

fn agent_of(to: &Address) -> Option<AgentNumber> {
    if to.kind() != address::Agent::KIND {
        return None;
    }
    to.key().parse().map(AgentNumber).ok()
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
