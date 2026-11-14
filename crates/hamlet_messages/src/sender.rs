//! The taker's check of its caller, read from the host's stamp.

use std::fmt;

use anyhow::{Context, Result};
use hamlet_actor::{Actor, Ctx};
use hamlet_actor_error::ActorError;
use hamlet_address::Caller;
use serde::Serialize;

use crate::Message;
use crate::address::{Agent, LOCAL, Party, Realm, Workspace};
use crate::workspace::{AgentNumber, CallId};

/// A kind of actor that sends a [`Message`].
pub trait Sender: Sized {
    /// The sender of a call the host stamped `caller`, or `None` for
    /// a caller of another kind or key.
    fn of(caller: Option<&Caller>) -> Option<Self>;
}

impl Sender for Workspace {
    fn of(caller: Option<&Caller>) -> Option<Self> {
        is_local::<Self>(caller).then_some(Self)
    }
}

impl Sender for Realm {
    fn of(caller: Option<&Caller>) -> Option<Self> {
        is_local::<Self>(caller).then_some(Self)
    }
}

impl Sender for Agent {
    fn of(caller: Option<&Caller>) -> Option<Self> {
        let Some(Caller::Actor(from)) = caller else {
            return None;
        };
        if from.kind() != Self::KIND {
            return None;
        }
        agent_at(from.key()).map(Self)
    }
}

/// The chief, through a client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Client;

impl Sender for Client {
    fn of(caller: Option<&Caller>) -> Option<Self> {
        matches!(caller, Some(Caller::Client)).then_some(Self)
    }
}

/// The Hand: the agent that founds hamlets and seats mayors (H6,
/// H18).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hand;

impl Hand {
    pub const NUMBER: AgentNumber = AgentNumber(1);
}

impl Sender for Hand {
    fn of(caller: Option<&Caller>) -> Option<Self> {
        (Agent::of(caller)? == Agent(Self::NUMBER)).then_some(Self)
    }
}

fn is_local<P: Party>(caller: Option<&Caller>) -> bool {
    matches!(
        caller,
        Some(Caller::Actor(from))
            if from.kind() == P::KIND && from.key() == LOCAL
    )
}

/// The agent at `key`, which its number writes back exactly: one key
/// per agent.
fn agent_at(key: &str) -> Option<AgentNumber> {
    let number = key.parse::<u32>().ok()?;
    (number.to_string() == key).then_some(AgentNumber(number))
}

/// The sender of `message`, the call `ctx` handles, or the error of
/// [`wrong_caller`].
pub fn sender<M, A>(ctx: &Ctx<A>, _message: &M) -> Result<M::From>
where
    M: Message,
    A: Actor<ConnParams = Option<Caller>>,
{
    let caller =
        ctx.conn().context("the call has no connection")?.params()?;
    match M::From::of(caller.as_ref()) {
        Some(from) => Ok(from),
        None => Err(wrong_caller(M::NAME, caller)),
    }
}

/// Logs that `caller` may not send `action`, and builds the
/// [`WrongCaller`] its call fails with.
pub fn wrong_caller(
    action: &'static str,
    caller: Option<Caller>,
) -> anyhow::Error {
    let caller = Stamp(caller);
    tracing::warn!(%caller, action, "refused a call");
    WrongCaller { action, caller }.build()
}

/// The error of a call from a caller its message's
/// [`Message::From`] does not admit.
#[derive(Debug, ActorError, Serialize)]
#[error(
    "message",
    "wrong_caller",
    "The caller may not send this message.",
    "{caller} may not send '{action}'."
)]
pub struct WrongCaller {
    action: &'static str,
    #[serde(skip_serializing_if = "Stamp::is_none")]
    caller: Stamp,
}

/// Logs that `call`, an id of a call of `action`, was given before
/// to another request, and builds the [`IdReused`] its call fails
/// with.
pub fn id_reused(
    action: &'static str,
    call: &CallId,
) -> anyhow::Error {
    tracing::warn!(action, call = call.0, "refused a reused call id");
    IdReused {
        action,
        call: call.0.clone(),
    }
    .build()
}

/// The error of a call whose id was given before to a call with
/// another request (S19).
#[derive(Debug, ActorError, Serialize)]
#[error(
    "message",
    "id_reused",
    "The call id was used before for another request.",
    "The call id {call} of '{action}' was used before for another \
     request."
)]
pub struct IdReused {
    action: &'static str,
    call: String,
}

/// The host's stamp, absent for the program's own call.
#[derive(Debug, Serialize)]
#[serde(transparent)]
struct Stamp(Option<Caller>);

impl Stamp {
    fn is_none(&self) -> bool {
        self.0.is_none()
    }
}

impl fmt::Display for Stamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(caller) => caller.fmt(f),
            None => f.write_str("a call with no caller"),
        }
    }
}

#[cfg(test)]
mod tests {
    use hamlet_address::Address;

    use super::*;

    fn actor(address: Address) -> Option<Caller> {
        Some(Caller::Actor(address))
    }

    fn at(kind: &str, key: &str) -> Option<Caller> {
        actor(Address::new(kind, key).unwrap())
    }

    #[test]
    fn each_party_reads_back_from_its_address() {
        let workspace = actor(Workspace.address().unwrap());
        let realm = actor(Realm.address().unwrap());
        assert_eq!(
            Workspace::of(workspace.as_ref()),
            Some(Workspace)
        );
        assert_eq!(Realm::of(realm.as_ref()), Some(Realm));
        for number in [0, 7, u32::MAX] {
            let agent = Agent(AgentNumber(number));
            let caller = actor(agent.address().unwrap());
            assert_eq!(Agent::of(caller.as_ref()), Some(agent));
        }
    }

    #[test]
    fn an_agent_is_at_its_number_written_back_exactly() {
        for key in ["07", "hand", "errand-3", "-1"] {
            assert_eq!(
                Agent::of(at("agent", key).as_ref()),
                None,
                "{key}"
            );
        }
        // `+` is no address's, so only the key is read.
        assert_eq!(agent_at("+7"), None);
        assert_eq!(agent_at("7"), Some(AgentNumber(7)));
    }

    #[test]
    fn a_party_is_its_kind_and_key() {
        for caller in [at("realm", "other"), at("workspace", "local")]
        {
            assert_eq!(
                Realm::of(caller.as_ref()),
                None,
                "{caller:?}"
            );
        }
        assert_eq!(
            Workspace::of(at("realm", "local").as_ref()),
            None
        );
        assert_eq!(Agent::of(at("realm", "7").as_ref()), None);
    }

    #[test]
    fn a_client_and_no_caller_are_no_party() {
        for caller in [Some(Caller::Client), None] {
            let caller = caller.as_ref();
            assert_eq!(Workspace::of(caller), None);
            assert_eq!(Realm::of(caller), None);
            assert_eq!(Agent::of(caller), None);
        }
    }

    #[test]
    fn the_hand_is_agent_1_and_the_client_no_actor() {
        assert_eq!(Hand::of(at("agent", "1").as_ref()), Some(Hand));
        for caller in [
            at("agent", "2"),
            at("agent", "01"),
            Some(Caller::Client),
        ] {
            assert_eq!(Hand::of(caller.as_ref()), None, "{caller:?}");
        }
        assert_eq!(Client::of(Some(&Caller::Client)), Some(Client));
        for caller in [at("agent", "1"), None] {
            assert_eq!(
                Client::of(caller.as_ref()),
                None,
                "{caller:?}"
            );
        }
    }
}
