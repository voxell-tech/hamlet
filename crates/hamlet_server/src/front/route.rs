//! What each client message becomes: a call of one actor and the
//! frame its answer becomes, a watch of a chat, a chief's line or
//! command, or the door's own answer.

use anyhow::{Context, Result};
use hamlet_actor::action::encode_positional;
use hamlet_host::Address;
use hamlet_messages::Message;
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::chat::{
    self, CheckConnection, NewSession, ReadHistory, ReadModels,
    ReadPrompt, Resume, Retry, Rewind,
};
use hamlet_messages::realm::documents::ReadCharter;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{
    self as protocol, AgentId, Check, ClientMessage, Read, RequestId,
    ServerMessage,
};

use super::answers::Reading;
use super::call::Stopping;
use super::host::HAND;
use super::view::Own;
use super::{commands, frames};
use crate::clients::check::{fault, reply};
use crate::society::refusal::Refusal;

/// What a client message becomes.
pub(crate) enum Route {
    /// Answered by the door, with no actor.
    Now(Box<ServerMessage>),
    Open(protocol::Open),
    Close(protocol::ChatRef),
    /// A chief's command or edit, which `commands` answers.
    Command(Asked),
    /// A stop, which `commands` answers, and the agents it stops as
    /// far as the door knows them before it runs.
    Stop(Asked, Stopping),
    /// An answer that names its wait, which `commands` answers once
    /// the stops sent before it are answered.
    Answer(Asked),
    /// A chief's line, whose reply is owed until the agent's work on
    /// it ends.
    Line(protocol::Say, AgentNumber),
    Call(Call),
    /// A check of the realm's model, whose link is testing until it
    /// answers, unless every agent runs the faux model.
    Check(Call),
    SignIn(protocol::Login),
    Read(Reading),
    Upload(Upload),
}

/// A chief's upload: its start or one of its parts.
pub(crate) enum Upload {
    Image(protocol::PutImage),
    ImagePart(protocol::PutImagePart),
    File(protocol::PutFile),
    FilePart(protocol::PutFilePart),
}

impl Upload {
    pub(crate) fn id(&self) -> &RequestId {
        match self {
            Self::Image(put) => &put.id,
            Self::ImagePart(part) => &part.id,
            Self::File(put) => &put.id,
            Self::FilePart(part) => &part.id,
        }
    }
}

/// A chief's command, edit, answer to a wait or stop, answered by
/// `commands`.
pub(crate) enum Asked {
    Command(protocol::Say),
    Edit(RequestId, protocol::Edit),
    Answer(protocol::Answer),
    /// A stop of one agent.
    Stop(RequestId, AgentId),
    StopAll(RequestId),
    MoveLodges(RequestId),
}

impl Asked {
    pub(crate) fn id(&self) -> &RequestId {
        match self {
            Self::Command(say) => &say.id,
            Self::Edit(id, _)
            | Self::Stop(id, _)
            | Self::StopAll(id)
            | Self::MoveLodges(id) => id,
            Self::Answer(answer) => &answer.id,
        }
    }
}

/// One call of one actor for request `request`, and the frame its
/// answer becomes.
pub(crate) struct Call {
    pub(crate) to: Address,
    pub(crate) action: &'static str,
    pub(crate) args: Vec<u8>,
    pub(crate) request: RequestId,
    pub(crate) answer: Words,
    pub(crate) after: After,
}

/// The frame an answer becomes, for the request that asked it.
pub(crate) type Words =
    fn(&RequestId, &[u8], &Own) -> Result<ServerMessage>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum After {
    Nothing,
    /// The realm may have changed: every client is shown it again.
    Show,
    /// As `Show`, and the agent's chat began again: the watchers of
    /// its current session read it again.
    Reread(AgentNumber),
    Began(AgentNumber),
}

/// What `message`, from a client that said hello, becomes.
pub(crate) fn route(message: ClientMessage) -> Result<Route> {
    let id = message.id().cloned();
    let route = match message {
        ClientMessage::Hello(_) => Route::Now(Box::new(fault(
            None,
            "This connection said hello already.",
        ))),
        ClientMessage::Say(say)
            if say.text.trim().starts_with('/') =>
        {
            if commands::parse::is_stop(&say.text) {
                let stopping =
                    if say.text.split_whitespace().nth(1).is_none() {
                        stopping(say.to)
                    } else {
                        Stopping::Every
                    };
                Route::Stop(Asked::Command(say), stopping)
            } else if commands::parse::names_its_wait(&say.text) {
                Route::Answer(Asked::Command(say))
            } else {
                Route::Command(Asked::Command(say))
            }
        }
        ClientMessage::Say(say) => match number(say.to) {
            Some(agent) => Route::Line(say, agent),
            None => no_one(&say.id, say.to),
        },
        ClientMessage::Edit(edit) => Route::Command(Asked::Edit(
            id.context("an edit has an id")?,
            edit,
        )),
        ClientMessage::Open(open) => Route::Open(open),
        ClientMessage::Close(close) => Route::Close(close.chat),
        ClientMessage::Answer(answer) => {
            Route::Answer(Asked::Answer(answer))
        }
        ClientMessage::Stop(protocol::Stop { agent: None, id }) => {
            Route::Stop(Asked::StopAll(id), Stopping::Every)
        }
        ClientMessage::Stop(protocol::Stop {
            agent: Some(agent),
            id,
        }) => Route::Stop(Asked::Stop(id, agent), stopping(agent)),
        ClientMessage::New(new) => {
            let (to, id) = (new.to, new.id.clone());
            agent_call(
                to,
                &id,
                NewSession(new),
                frames::began,
                After::Began,
            )?
        }
        ClientMessage::Resume(resume) => {
            let (to, id) = (resume.to, resume.id.clone());
            agent_call(
                to,
                &id,
                Resume(resume),
                frames::back,
                After::Reread,
            )?
        }
        ClientMessage::Retry(retry) => {
            let (to, id) = (retry.to, retry.id.clone());
            agent_call(
                to,
                &id,
                Retry(retry),
                frames::carrying_on,
                |_| After::Show,
            )?
        }
        ClientMessage::Rewind(rewind) => {
            let (to, id) = (rewind.chat.with, rewind.id.clone());
            agent_call(
                to,
                &id,
                Rewind(rewind),
                frames::rewound,
                After::Reread,
            )?
        }
        ClientMessage::Login(login) => Route::SignIn(login),
        ClientMessage::Check(Check::Model { id, model }) => {
            check(&id, &chat::Check { model })?
        }
        ClientMessage::Check(Check::Connection { id }) => {
            check(&id, &CheckConnection)?
        }
        ClientMessage::Read(Read::Prompt { agent, id }) => {
            agent_call(
                agent,
                &id,
                ReadPrompt,
                frames::prompt,
                |_| After::Nothing,
            )?
        }
        ClientMessage::Read(Read::History { agent, id }) => {
            agent_call(
                agent,
                &id,
                ReadHistory,
                frames::history,
                |_| After::Nothing,
            )?
        }
        ClientMessage::Read(Read::Models { id, .. }) => call(
            &address::Agent(HAND),
            &id,
            &ReadModels,
            frames::models,
            After::Nothing,
        )?,
        ClientMessage::Read(Read::Charter { id }) => call(
            &address::Realm,
            &id,
            &ReadCharter,
            frames::charter,
            After::Nothing,
        )?,
        ClientMessage::Read(Read::Papers { of, id, .. }) => {
            Route::Read(Reading::Papers { id, of })
        }
        ClientMessage::Read(Read::Image { id, image, size }) => {
            Route::Read(Reading::Image {
                id,
                image,
                size: size.unwrap_or_default(),
            })
        }
        ClientMessage::PutImage(put) => {
            Route::Upload(Upload::Image(put))
        }
        ClientMessage::PutImagePart(part) => {
            Route::Upload(Upload::ImagePart(part))
        }
        ClientMessage::PutFile(put) => {
            Route::Upload(Upload::File(put))
        }
        ClientMessage::PutFilePart(part) => {
            Route::Upload(Upload::FilePart(part))
        }
        ClientMessage::MoveLodges(asked) => {
            Route::Command(Asked::MoveLodges(asked.id))
        }
    };
    Ok(route)
}

/// The agent a client names by `agent`, if any agent can have that
/// number.
pub(crate) fn number(agent: AgentId) -> Option<AgentNumber> {
    u32::try_from(agent.0).ok().map(AgentNumber)
}

/// A stop of the agent a client names by `agent`.
fn stopping(agent: AgentId) -> Stopping {
    Stopping::Agents(number(agent).into_iter().collect())
}

/// The refusal of request `id` to agent `agent`, which no agent is.
fn no_one(id: &RequestId, agent: AgentId) -> Route {
    Route::Now(Box::new(reply(
        id,
        Err(Refusal::NoOne {
            address: agent.0.to_string(),
        }),
    )))
}

/// A call to the agent a client names by `to`.
fn agent_call<M: Message<To = address::Agent>>(
    to: AgentId,
    id: &RequestId,
    message: M,
    answer: Words,
    after: fn(AgentNumber) -> After,
) -> Result<Route> {
    let Some(agent) = number(to) else {
        return Ok(no_one(id, to));
    };
    call(&address::Agent(agent), id, &message, answer, after(agent))
}

fn call<M: Message>(
    to: &M::To,
    id: &RequestId,
    message: &M,
    answer: Words,
    after: After,
) -> Result<Route> {
    Ok(Route::Call(made(to, id, message, answer, after)?))
}

/// The Hand's check of a model, which its link shows.
fn check<M: Message<To = address::Agent>>(
    id: &RequestId,
    message: &M,
) -> Result<Route> {
    Ok(Route::Check(made(
        &address::Agent(HAND),
        id,
        message,
        frames::checked,
        After::Nothing,
    )?))
}

fn made<M: Message>(
    to: &M::To,
    id: &RequestId,
    message: &M,
    answer: Words,
    after: After,
) -> Result<Call> {
    Ok(Call {
        to: to.address()?,
        action: M::NAME,
        args: encode_positional(message)?,
        request: id.clone(),
        answer,
        after,
    })
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::client::{ReplyKind, Say};

    use super::*;

    fn say(text: &str, to: u64) -> ClientMessage {
        ClientMessage::Say(Say {
            id: RequestId::new(),
            to: AgentId(to),
            text: text.into(),
            files: Vec::new(),
            images: Vec::new(),
        })
    }

    #[test]
    fn a_line_is_owed_and_a_line_with_a_slash_is_a_command() {
        assert!(matches!(
            route(say("Hello there.", 2)).unwrap(),
            Route::Line(_, AgentNumber(2))
        ));
        assert!(matches!(
            route(say("  /new", 2)).unwrap(),
            Route::Command(Asked::Command(_))
        ));
    }

    #[test]
    fn a_line_to_a_number_no_agent_can_have_is_refused() {
        let Route::Now(answer) =
            route(say("Hello.", u64::MAX)).unwrap()
        else {
            panic!("a line to no agent is answered at once");
        };
        let ServerMessage::Reply(refused) = *answer else {
            panic!("a reply");
        };
        assert_eq!(refused.kind, ReplyKind::Error);
        assert!(refused.text.contains(&u64::MAX.to_string()));
    }

    #[test]
    fn stops_keep_their_turn_and_named_answers_do_not() {
        let stop = |message| {
            matches!(route(message).unwrap(), Route::Stop(..))
        };
        let named = |message| {
            matches!(route(message).unwrap(), Route::Answer(_))
        };
        for agent in [None, Some(AgentId(3))] {
            assert!(stop(ClientMessage::Stop(protocol::Stop {
                agent,
                id: RequestId::new(),
            })));
        }
        for text in ["/stop", " /stop all", "/stop hire#3"] {
            assert!(stop(say(text, 1)), "{text}");
        }
        assert!(named(say("/deny #4", 1)));
        for text in ["/deny", "/allow the oldest", "/name-hamlet h n"]
        {
            assert!(!named(say(text, 1)), "{text}");
        }
    }

    #[test]
    fn a_new_session_has_its_watchers_read_the_chat_again() {
        let Route::Call(call) =
            route(ClientMessage::New(protocol::New {
                id: RequestId::new(),
                to: AgentId(1),
            }))
            .unwrap()
        else {
            panic!("`new` is a call of the agent");
        };
        assert_eq!(call.action, "new_session");
        assert_eq!(call.after, After::Began(AgentNumber(1)));
    }

    #[test]
    fn the_models_are_read_from_the_hand_and_the_charter_from_the_realm()
     {
        let Route::Call(models) =
            route(ClientMessage::Read(Read::Models {
                id: RequestId::new(),
                refresh: None,
            }))
            .unwrap()
        else {
            panic!("the models are a call");
        };
        assert_eq!(models.to.to_string(), "agent/1");
        let Route::Call(charter) =
            route(ClientMessage::Read(Read::Charter {
                id: RequestId::new(),
            }))
            .unwrap()
        else {
            panic!("the charter is a call");
        };
        assert_eq!(charter.to.to_string(), "realm/local");
        assert_eq!(charter.action, "read_charter");
    }
}
