//! A runtime's requests while a turn runs: whether a tool call may
//! run (C7, H11), and the call carried out once under its key (S19).
//! A chief's answer to what waits lets held calls go on.

mod answer;
mod deciding;
mod execution;
mod held;
pub(crate) mod looks;
mod objections;
mod permissions;
pub(crate) mod place;
pub(crate) mod reviews;
pub(crate) mod society;
pub(crate) mod specs;
pub(crate) mod tool_runs;

use std::collections::BTreeMap;

use anyhow::Result;
use bevy_ecs::entity::Entity;
use hamlet_protocol::runtime::{self, ServerMessage};
use hamlet_store::ids::Actor;
use hamlet_store::marker::{Identity, Request};
use serde_json::{Map, Value};

use self::held::State;
use self::looks::Look;
use self::objections::OBJECTING;
use crate::attachments::images::fitting::kept_pictures;
use crate::conversations::marks::Mark;
use crate::conversations::{Conversation, Kind, inputs, questions};
use crate::core::Core;
use crate::society::oversight::gate::{Session, TurnGate};
use crate::society::oversight::{self, Raise, Ruling, WaitKind};
use crate::tools::builtin::Outcome;
use crate::tools::catalogue::Society;

/// The words of a call that waits for a chief.
const WAITING: &str = "This call waits for a chief's answer.";
/// The words of a call that nobody may approve.
const NOBODY: &str = "nobody is here to approve it; it did not run";
const IDENTITY: &str = "This call's identity does not match a \
                        verified request, so it did not run.";
/// The words of a call whose key was kept for another call.
const CONFLICT: &str =
    "This call's key was used for another call, so it did not run.";
/// The words of a call in a turn that was stopped.
const STOPPED: &str = "The chief stopped this turn.";
/// The words of a question no chief can answer.
const NO_CHIEF: &str =
    "No chief oversees this turn; go on with what you know.";

pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_plugins(tool_runs::plugin);
}

/// A tool call, by its runtime and key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CallId {
    pub runtime: String,
    pub key: String,
}

impl CallId {
    fn identity(&self) -> Identity<'_> {
        Identity {
            source: &self.runtime,
            key: &self.key,
        }
    }
}

/// The address of a call's answer: the runtime process that made
/// the call, and its request.
#[derive(Clone, Debug)]
pub(crate) struct Reply {
    pub to: Entity,
    pub generation: u64,
    pub request: String,
}

/// The result of carrying out a call now.
enum CallOutcome {
    /// Its outcome, to keep and answer.
    Done(Outcome),
    /// The answer its runtime gets while it cannot finish yet.
    Answer(Answer),
    /// It goes on off the loop, a look or a run, and is answered once
    /// that is done.
    Later,
}

/// A runtime's answer about a call.
pub(crate) enum Answer {
    Allowed,
    Waiting(String),
    Refusal(String),
    Result(Outcome),
}

/// A result in words.
pub(crate) fn said(text: impl Into<String>) -> Outcome {
    Outcome {
        text: text.into(),
        error: false,
        details: None,
        diagnostics: Vec::new(),
        images: Vec::new(),
    }
}

/// An error in words.
pub(crate) fn refused(text: impl Into<String>) -> Outcome {
    Outcome {
        error: true,
        ..said(text)
    }
}

/// One call as a runtime made it.
struct Call<'a> {
    /// Its runtime and key.
    call: CallId,
    conversation: Conversation,
    input: Option<&'a str>,
    tool: &'a str,
    /// The society tool `tool` names, read once here.
    society: Option<Society>,
    arguments: &'a BTreeMap<String, Value>,
    /// Its request as its marker keeps it.
    request: Request,
}

impl Call<'_> {
    fn args(&self) -> Value {
        Value::Object(
            self.arguments.clone().into_iter().collect::<Map<_, _>>(),
        )
    }
}

/// The gate's decision on a call.
enum Decision {
    Run {
        net: bool,
    },
    Block(String),
    Wait(&'static str),
    /// It decides once it has looked at this, off the loop.
    Look(Box<Look>),
}

impl Core<'_, '_> {
    /// Answers `reply` about the call `key`, unless its runtime
    /// started again since it made the call.
    fn tell_call(
        &mut self,
        reply: &Reply,
        key: &str,
        answer: Answer,
    ) {
        if self.runtimes.generation(reply.to)
            != Some(reply.generation)
        {
            return;
        }
        let request = reply.request.clone();
        let key = key.to_owned();
        let images_folder = self.realm.layout().images();
        self.runtimes.send(reply.to, |v, id| match answer {
            Answer::Allowed => {
                ServerMessage::Allowed(runtime::Allowed {
                    id,
                    re: request,
                    v,
                })
            }
            Answer::Waiting(words) => {
                ServerMessage::Waiting(runtime::Waiting {
                    id,
                    key,
                    re: request,
                    v,
                    words,
                })
            }
            Answer::Refusal(words) => {
                ServerMessage::Refusal(runtime::Refusal {
                    id,
                    re: request,
                    v,
                    words,
                })
            }
            Answer::Result(outcome) => {
                let images = kept_pictures(
                    images_folder.clone(),
                    &outcome.images,
                );
                ServerMessage::Result(runtime::Result {
                    details: outcome
                        .details
                        .map(|d| d.into_iter().collect()),
                    images,
                    diagnostics: (!outcome.diagnostics.is_empty())
                        .then_some(outcome.diagnostics),
                    error: outcome.error,
                    id,
                    re: request,
                    text: outcome.text,
                    v,
                })
            }
        });
    }

    /// The task of the turn that makes `call`.
    fn task_of(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Option<hamlet_store::ids::TaskId>> {
        let Some(submission) = call.input else {
            return Ok(None);
        };
        let db = self.realm.store().connection();
        Ok(inputs::by_submission(
            db,
            call.conversation.id,
            submission,
        )?
        .and_then(|input| input.task))
    }

    /// Puts the questions of `call` to the chief, or gives the
    /// chief's answer once there is one. `Err` is the runtime's
    /// answer while it waits or when it may put no more questions.
    fn ask_chief(
        &mut self,
        call: &Call<'_>,
    ) -> Result<std::result::Result<Outcome, Answer>> {
        if call.conversation.cancelling {
            return Ok(Err(Answer::Refusal(STOPPED.into())));
        }
        let db = self.realm.store().connection();
        match held::kept(db, &call.call.runtime, &call.call.key)?
            .map(|kept| kept.state)
        {
            Some(State::Blocked(words)) => {
                return Ok(Err(Answer::Refusal(words)));
            }
            Some(State::Ruled {
                ruling: Ruling::Reply,
                text,
                ..
            }) => return Ok(Ok(said(text))),
            Some(State::Ruled { .. }) => {
                return Ok(Ok(said(
                    "The chief did not answer; go on with what you \
                     know.",
                )));
            }
            Some(State::Held(_)) => {
                return Ok(Err(Answer::Waiting(WAITING.into())));
            }
            Some(State::Objected) => {
                return Ok(Err(Answer::Waiting(OBJECTING.into())));
            }
            Some(State::Open | State::Allowed { .. }) | None => {}
        }
        if !self.is_overseen(call)? {
            return Ok(Ok(said(NO_CHIEF)));
        }
        let questions = match questions::read(&serde_json::to_string(
            call.arguments,
        )?) {
            Ok(questions) => questions,
            Err(invalid) => {
                return Ok(Ok(refused(invalid.to_string())));
            }
        };
        let mut questions = serde_json::to_value(questions)?;
        self.credentials.scrub(&mut questions);
        let questions = serde_json::from_value(questions)?;
        let layout = self.realm.layout().clone();
        let agent = call.conversation.agent;
        let task = self.task_of(call)?;
        let db = self.realm.store().connection();
        let at = place::of(db, &layout, agent)?;
        let most = society::limits_of(db, at.hamlet.as_ref())?
            .asking
            .questions;
        let mut gate =
            TurnGate::of(db, call.conversation.id, call.input)?;
        if gate.questions >= most {
            let times = if most == 1 {
                "once".into()
            } else {
                format!("{most} times")
            };
            return Ok(Ok(said(format!(
                "You have put questions to the chief {times} this \
                 turn; go on with what you know."
            ))));
        }
        gate.questions += 1;
        let (runtime, key) =
            (call.call.runtime.clone(), call.call.key.clone());
        let conversation = call.conversation.id;
        self.change(Actor::Realm, |cx, _| {
            gate.keep(cx.db, conversation)?;
            let wait = oversight::raise(
                cx,
                Raise {
                    kind: WaitKind::Question,
                    danger: None,
                    agent,
                    profile: None,
                    conversation: Some(conversation),
                    task,
                    about: questions::about(&questions),
                    change: None,
                    answers: vec![
                        hamlet_protocol::client::Choice::Reply,
                        hamlet_protocol::client::Choice::Dismiss,
                    ],
                    questions: Some(questions),
                },
            )?;
            held::hold(cx.db, &runtime, &key, wait, None)
        })?;
        Ok(Err(Answer::Waiting(WAITING.into())))
    }

    /// Tells whether a chief oversees the turn that makes `call`: a
    /// chief's line, or a retry or reflection in a chief's chat.
    fn is_overseen(&mut self, call: &Call<'_>) -> Result<bool> {
        let Some(submission) = call.input else {
            return Ok(false);
        };
        let db = self.realm.store().connection();
        let Some(input) = inputs::by_submission(
            db,
            call.conversation.id,
            submission,
        )?
        else {
            return Ok(false);
        };
        Ok(match input.mark()? {
            Mark::Chief { .. } => true,
            Mark::Retry { .. } | Mark::Reflection { .. } => {
                call.conversation.kind == Kind::Chat
            }
            _ => false,
        })
    }
}

/// The chat session of `conversation`, whose edits and commands a
/// chief may trust.
pub(crate) fn chat_session(
    conversation: &Conversation,
) -> Option<Session> {
    match (
        conversation.kind,
        conversation.chief,
        conversation.session,
    ) {
        (Kind::Chat, Some(chief), Some(session)) => Some(Session {
            agent: conversation.agent,
            chief,
            session,
        }),
        _ => None,
    }
}
