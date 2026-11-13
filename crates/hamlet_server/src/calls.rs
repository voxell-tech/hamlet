//! What a runtime asks while a turn runs: whether a tool call may run
//! (C7, H11), and the call carried out once under its key (S19). A
//! chief's answer to what waits lets held calls go on.

mod answer;
mod deciding;
mod held;
pub(crate) mod looks;
mod objections;
pub(crate) mod place;
pub(crate) mod reviews;
mod society;

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Mutex, PoisonError};

use anyhow::Result;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::system::{Commands, Query};
use bevy_ecs::world::DeferredWorld;
use futures::future::BoxFuture;
use hamlet_protocol::runtime::{
    self, ConfigureTools, RuntimeMessage, ServerMessage, ToolSpec,
    ToolSpecReplay,
};
use hamlet_store::ids::{Actor, AgentId, ConversationId};
use hamlet_store::marker::{self, Identity, Marked, Request};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use self::held::State;
use self::looks::Look;
use self::objections::OBJECTING;
pub(crate) use self::society::library::LIBRARIAN;
pub(crate) use self::society::welcome;
use crate::core::Core;
use crate::images::Limits;
use crate::images::fitting::kept_pictures;
use crate::knowledge::CHARACTER;
use crate::later::{Again, Finished};
use crate::oversight::gate::{Asked, Session};
use crate::oversight::{self, Raise, Ruling, WaitKind};
use crate::talk::conversations::{Conversation, Kind};
use crate::talk::marks::Mark;
use crate::talk::{asking, conversations, inputs, models};
use crate::work::catalogue::Society;
use crate::work::tools::{Outcome, Sight, ToolSession};
use crate::workspace::serving::Lodging;

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

/// Where a call is answered: the runtime process that asked, and its
/// request.
#[derive(Clone, Debug)]
pub(crate) struct Reply {
    pub to: Entity,
    pub generation: u64,
    pub re: String,
}

/// A tool run that ended.
pub(crate) struct Ran {
    call: CallId,
    outcome: Outcome,
}

/// What carrying out a call comes to now.
enum Carried {
    /// Its outcome, to keep and answer.
    Done(Outcome),
    /// What its runtime is told while it cannot finish yet.
    Answer(Answer),
    /// It goes on off the loop, a look or a run, and is answered once
    /// that is done.
    Later,
}

/// What a runtime is answered about a call.
pub(crate) enum Answer {
    Allowed,
    Waiting(String),
    Refusal(String),
    Result(Outcome),
}

/// Where the tool runs that end are sent, as inputs.
#[derive(bevy_ecs::resource::Resource)]
pub(crate) struct Calls {
    ran: mpsc::UnboundedSender<Ran>,
}

/// A tool run in progress: an entity of its own, despawned when it
/// ends or its turn is stopped. Its task stops with it.
#[derive(Component)]
#[component(on_remove = stopped)]
pub(crate) struct Running {
    call: CallId,
    replies: Vec<Reply>,
    pub conversation: ConversationId,
    request: Request,
    /// The run's task, once started.
    abort: Option<AbortHandle>,
}

/// The work of a run not started yet: it starts once the input that
/// asked for it is kept, so a failed input runs nothing.
#[derive(Component)]
pub(crate) struct Starting(Mutex<Option<BoxFuture<'static, ()>>>);

/// Starts the work of every run its input kept.
pub(crate) fn start(
    mut runs: Query<(Entity, &mut Running, &mut Starting)>,
    mut commands: Commands,
) {
    for (entity, mut running, mut starting) in &mut runs {
        let work = starting
            .0
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(work) = work {
            running.abort = Some(tokio::spawn(work).abort_handle());
        }
        commands.entity(entity).remove::<Starting>();
    }
}

/// What a run that ended still owes.
struct Ended {
    replies: Vec<Reply>,
    request: Request,
}

/// A run's entity went: its task, if still running, stops.
fn stopped(world: DeferredWorld, context: HookContext) {
    if let Some(abort) = world
        .get::<Running>(context.entity)
        .and_then(|running| running.abort.as_ref())
    {
        abort.abort();
    }
}

/// Every tool run in progress.
pub(crate) type Runs<'w, 's> =
    Query<'w, 's, (Entity, &'static mut Running)>;

impl Calls {
    pub fn new(ran: mpsc::UnboundedSender<Ran>) -> Self {
        Self { ran }
    }

    /// The run of `work` for `call`, to spawn; it starts once its
    /// input is kept.
    fn run(
        &self,
        call: CallId,
        conversation: ConversationId,
        reply: Reply,
        request: Request,
        work: impl Future<Output = Outcome> + Send + 'static,
    ) -> (Running, Starting) {
        let ran = self.ran.clone();
        let id = call.clone();
        let work = async move {
            let outcome = work.await;
            let _ = ran.send(Ran { call: id, outcome });
        };
        let running = Running {
            call,
            replies: vec![reply],
            conversation,
            request,
            abort: None,
        };
        (running, Starting(Mutex::new(Some(Box::pin(work)))))
    }
}

/// The tools the server adds to every runtime's own: `ask_chief` and
/// the society's.
pub(crate) fn specs() -> Vec<ToolSpec> {
    let mut specs = vec![spec(
        "ask_chief",
        "Ask the chief a question you cannot go on without. Waits \
         for the answer, so ask only when no agent and no file can \
         tell you. Use object to argue against an instruction.",
        json!({
            "type": "object",
            "properties": {
                "message": {"type": "string"},
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": asking::MOST_QUESTIONS,
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": {"type": "string"},
                            "description": {"type": "string"},
                            "options": {
                                "type": "array",
                                "maxItems": asking::MOST_OPTIONS,
                                "items": {"anyOf": [
                                    {"type": "string"},
                                    {
                                        "type": "object",
                                        "properties": {
                                            "title": {"type": "string"},
                                            "description": {"type": "string"}
                                        },
                                        "required": ["title"]
                                    }
                                ]}
                            },
                            "text_input": {
                                "type": "boolean",
                                "description": "Allow custom words. Defaults to true."
                            }
                        },
                        "required": ["question"]
                    }
                }
            },
            "required": ["questions"]
        }),
        ToolSpecReplay::Safe,
    )];
    specs.extend(society::args::specs());
    specs
}

/// A tool's spec from its JSON Schema.
pub(crate) fn spec(
    name: &str,
    description: &str,
    parameters: Value,
    replay: ToolSpecReplay,
) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: description.into(),
        parameters: match parameters {
            Value::Object(map) => map.into_iter().collect(),
            _ => BTreeMap::new(),
        },
        replay,
        retain: None,
    }
}

/// Whether `conversation` offers `tool`.
fn offers(conversation: &Conversation, tool: &str) -> Result<bool> {
    Ok(match conversation.tools()? {
        None => true,
        Some(ConfigureTools::Only { only }) => {
            only.iter().any(|name| name == tool)
        }
        Some(ConfigureTools::Remove { remove }) => {
            !remove.iter().any(|name| name == tool)
        }
    })
}

/// The result of a call to a tool its agent is not offered.
fn unavailable(tool: &str) -> Outcome {
    Outcome::thrown(
        "tool_unavailable",
        format!("The tool {tool} is not yours to use."),
    )
}

/// A result in words.
pub(crate) fn said(text: impl Into<String>, error: bool) -> Outcome {
    Outcome {
        text: text.into(),
        error,
        details: None,
        diagnostics: Vec::new(),
        images: Vec::new(),
    }
}

/// One call as a runtime asked it.
struct Call<'a> {
    /// Its runtime and key.
    call: CallId,
    conversation: Conversation,
    input: Option<&'a str>,
    tool: &'a str,
    /// The society tool `tool` names, read once here.
    society: Option<Society>,
    arguments: &'a BTreeMap<String, Value>,
    /// What its marker keeps it as.
    request: Request,
}

impl Call<'_> {
    fn args(&self) -> Value {
        Value::Object(
            self.arguments.clone().into_iter().collect::<Map<_, _>>(),
        )
    }
}

/// What the gate decided of a call.
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
    /// started again since it asked.
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
        let re = reply.re.clone();
        let key = key.to_owned();
        let images_folder = self.realm.layout().images();
        self.runtimes.send(reply.to, |v, id| match answer {
            Answer::Allowed => {
                ServerMessage::Allowed(runtime::Allowed { id, re, v })
            }
            Answer::Waiting(words) => {
                ServerMessage::Waiting(runtime::Waiting {
                    id,
                    key,
                    re,
                    v,
                    words,
                })
            }
            Answer::Refusal(words) => {
                ServerMessage::Refusal(runtime::Refusal {
                    id,
                    re,
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
                    re,
                    text: outcome.text,
                    v,
                })
            }
        });
    }

    /// The call a runtime asked, with its key reserved for its
    /// request; the reply to give when the call cannot go on.
    fn admit<'a>(
        &mut self,
        from: Entity,
        conversation: &str,
        key: &str,
        input: Option<&'a str>,
        tool: &'a str,
        arguments: &'a BTreeMap<String, Value>,
    ) -> Result<std::result::Result<Call<'a>, Answer>> {
        let runtime = self.runtimes.id(from)?;
        let db = self.realm.store().connection();
        let Some(kept) =
            conversations::by_handle(db, &runtime, conversation)?
        else {
            return Ok(Err(Answer::Refusal(NOBODY.into())));
        };
        let request =
            Request::new("tool", &json!([input, tool, arguments]))?;
        if !held::reserve(
            db,
            &runtime,
            key,
            kept.id,
            request.digest(),
            input,
            tool,
        )? {
            return Ok(Err(Answer::Refusal(IDENTITY.into())));
        }
        Ok(Ok(Call {
            call: CallId {
                runtime,
                key: key.into(),
            },
            conversation: kept,
            input,
            tool,
            society: Society::named(tool),
            arguments,
            request,
        }))
    }

    fn reply_of(&self, from: Entity, re: &str) -> Reply {
        Reply {
            to: from,
            generation: self.runtimes.generation(from).unwrap_or(0),
            re: re.into(),
        }
    }

    /// Whether the call `asked` may run, before it is kept as begun.
    pub(crate) fn allow_call(
        &mut self,
        from: Entity,
        asked: runtime::Allow,
    ) -> Result<()> {
        let again = RuntimeMessage::Allow(asked.clone());
        let reply = self.reply_of(from, &asked.id);
        let call = match self.admit(
            from,
            &asked.conversation,
            &asked.key,
            asked.input.as_deref(),
            &asked.tool,
            &asked.arguments,
        )? {
            Ok(call) => call,
            Err(answer) => {
                self.tell_call(&reply, &asked.key, answer);
                return Ok(());
            }
        };
        if call.society != Some(Society::Object)
            && self.withdrawn(&call)?
        {
            self.tell_call(
                &reply,
                &asked.key,
                Answer::Refusal(held::WITHDRAWN.into()),
            );
            return Ok(());
        }
        if self.objecting(&call)? {
            self.tell_call(
                &reply,
                &asked.key,
                Answer::Waiting(OBJECTING.into()),
            );
            return Ok(());
        }
        let answer = if !offers(&call.conversation, call.tool)?
            || matches!(
                call.society,
                Some(Society::AskChief | Society::Object)
            ) {
            Answer::Allowed
        } else {
            match self.decide(&call)? {
                Decision::Run { .. } => Answer::Allowed,
                Decision::Block(words) => Answer::Refusal(words),
                Decision::Wait(words) => {
                    Answer::Waiting(words.into())
                }
                Decision::Look(look) => {
                    self.look(&call, look, from, again);
                    return Ok(());
                }
            }
        };
        self.tell_call(&reply, &asked.key, answer);
        Ok(())
    }

    /// Looks at what `call` waits on, on a task; once found, the
    /// runtime's request `again` runs again and finds it.
    fn look(
        &mut self,
        call: &Call<'_>,
        look: Box<Look>,
        from: Entity,
        again: RuntimeMessage,
    ) {
        let secrets = self.credentials.secrets();
        let again = Again {
            from,
            generation: self.runtimes.generation(from).unwrap_or(0),
            message: again,
        };
        let id = call.call.clone();
        self.later.spawn(async move {
            Finished::Looked {
                call: id,
                found: look.run(secrets).await,
                again,
            }
        });
    }

    /// Carries out the call `asked`: its kept result when it ran,
    /// else as the gate decides now.
    pub(crate) fn operation(
        &mut self,
        from: Entity,
        asked: runtime::Operation,
    ) -> Result<()> {
        let again = RuntimeMessage::Operation(asked.clone());
        let reply = self.reply_of(from, &asked.id);
        let call = match self.admit(
            from,
            &asked.conversation,
            &asked.key,
            asked.input.as_deref(),
            &asked.tool,
            &asked.arguments,
        )? {
            Ok(call) => call,
            Err(answer) => {
                self.tell_call(&reply, &asked.key, answer);
                return Ok(());
            }
        };
        // A call that runs already answers this request too.
        if let Some((_, mut running)) = self
            .runs
            .iter_mut()
            .find(|(_, running)| running.call == call.call)
        {
            running.replies.push(reply);
            return Ok(());
        }
        if let Some(answer) = self.kept_answer(&call)? {
            self.tell_call(&reply, &asked.key, answer);
            return Ok(());
        }
        match self.carry_out(&call, &reply, from, again)? {
            Carried::Done(outcome) => {
                let db = self.realm.store().connection();
                marker::settle(
                    db,
                    call.call.identity(),
                    &call.request,
                    &outcome,
                    crate::clock::now(),
                )?;
                self.looks.forget(&call.call);
                self.tell_call(
                    &reply,
                    &asked.key,
                    Answer::Result(outcome),
                );
            }
            Carried::Answer(answer) => {
                self.tell_call(&reply, &asked.key, answer);
            }
            Carried::Later => {}
        }
        Ok(())
    }

    /// What `call` was answered with when it was asked before: its
    /// outcome, or that a restart cut it off; none when it is new.
    fn kept_answer(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Option<Answer>> {
        let db = self.realm.store().connection();
        let tool = call.tool;
        Ok(
            match marker::read::<Outcome>(
                db,
                call.call.identity(),
                &call.request,
            )? {
                Marked::Begun => {
                    Some(Answer::Result(Outcome::thrown(
                        "tool_interrupted",
                        format!(
                            "Tool {tool} was interrupted by a \
                             restart and may have partially run"
                        ),
                    )))
                }
                Marked::Outcome(outcome) => {
                    Some(Answer::Result(outcome))
                }
                Marked::Conflict => {
                    Some(Answer::Refusal(CONFLICT.into()))
                }
                Marked::Unmarked => None,
            },
        )
    }

    /// Carries out the new call `call` as far as it goes now:
    /// answered by a chief or the gate, run, or looked at off the
    /// loop first.
    fn carry_out(
        &mut self,
        call: &Call<'_>,
        reply: &Reply,
        from: Entity,
        again: RuntimeMessage,
    ) -> Result<Carried> {
        let tool = call.tool;
        if !offers(&call.conversation, tool)? {
            return Ok(Carried::Done(unavailable(tool)));
        }
        if let Some(chief @ (Society::AskChief | Society::Object)) =
            call.society
        {
            let ruled = if chief == Society::Object {
                self.object(call)?
            } else {
                self.ask_chief(call)?
            };
            return Ok(match ruled {
                Ok(outcome) => Carried::Done(outcome),
                Err(answer) => Carried::Answer(answer),
            });
        }
        if self.withdrawn(call)? {
            return Ok(Carried::Done(said(held::WITHDRAWN, true)));
        }
        if self.objecting(call)? {
            return Ok(Carried::Answer(Answer::Waiting(
                OBJECTING.into(),
            )));
        }
        Ok(match self.decide(call)? {
            Decision::Look(look) => {
                self.look(call, look, from, again);
                Carried::Later
            }
            Decision::Block(words) => {
                Carried::Answer(Answer::Refusal(words))
            }
            Decision::Wait(words) => {
                Carried::Answer(Answer::Waiting(words.into()))
            }
            Decision::Run { net } => match call.society {
                Some(society) => match self.society(call, society)? {
                    society::Step::Done(outcome) => {
                        Carried::Done(outcome)
                    }
                    society::Step::Wait(words) => {
                        Carried::Answer(Answer::Waiting(words))
                    }
                    society::Step::Look(look) => {
                        self.look(call, look, from, again);
                        Carried::Later
                    }
                },
                None => {
                    let request = call.request.clone();
                    match self.run_tool(
                        call,
                        reply.clone(),
                        request,
                        net,
                    )? {
                        Some(outcome) => Carried::Done(outcome),
                        None => Carried::Later,
                    }
                }
            },
        })
    }

    /// Runs the built-in or added tool of `call` with the network
    /// `net`, after marking it begun when it may not run again. Gives
    /// the outcome of a call refused before it runs.
    fn run_tool(
        &mut self,
        call: &Call<'_>,
        reply: Reply,
        request: Request,
        net: bool,
    ) -> Result<Option<Outcome>> {
        let once = self.runs_once(call.tool);
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let agent = call.conversation.agent;
        let at =
            place::of_conversation(db, &layout, &call.conversation)?;
        if let Some(refusal) = character_refusal(&at.place, call) {
            return Ok(Some(said(refusal, true)));
        }
        if once {
            marker::begin(
                db,
                call.call.identity(),
                &request,
                crate::clock::now(),
            )?;
        }
        let change =
            held::kept(db, &call.call.runtime, &call.call.key)?
                .and_then(|kept| kept.change)
                .and_then(|change| {
                    serde_json::from_str::<Value>(&change).ok()
                });
        let (session, lodging) =
            self.tools_of(call, at.place, net)?;
        let session = session.seeing(self.sight_of(agent)?);
        let secrets = self.credentials.secrets();
        let arguments =
            call.arguments.clone().into_iter().collect::<Map<_, _>>();
        let name = call.tool.to_owned();
        let running = self.calls.run(
            call.call.clone(),
            call.conversation.id,
            reply,
            request,
            async move {
                // A remote lodge is in step before the call runs
                // there, and comes home after one that may write it,
                // before the agent's next call reads it.
                if let Some(lodging) = &lodging
                    && let Err(error) = lodging.in_step().await
                {
                    return said(
                        hamlet_sandbox::scrub::scrub_text(
                            &secrets,
                            &format!(
                                "The remote workspace could not \
                                 take the lodge in: {error:#}"
                            ),
                        ),
                        true,
                    );
                }
                let mut outcome =
                    session.run(&secrets, &name, &arguments).await;
                if once
                    && let Some(lodging) = &lodging
                    && let Err(error) = lodging.pull().await
                {
                    crate::diagnostic::tell(
                        crate::diagnostic::Secrets::Listed(&secrets),
                        format!(
                            "could not bring a lodge home: {error:#}"
                        ),
                    );
                }
                if let Some(change) = change {
                    outcome
                        .details
                        .get_or_insert_with(Map::new)
                        .insert("change".into(), change);
                }
                outcome
            },
        );
        self.commands.spawn(running);
        Ok(None)
    }

    /// Where the tools of `call`'s agent run: in the remote workspace
    /// the realm works in, with what keeps its lodge's remote copy in
    /// step, else on this machine. A dream works on this machine's
    /// checkout of its scholar, so it runs here.
    fn tools_of(
        &mut self,
        call: &Call<'_>,
        place: crate::work::tools::Place,
        net: bool,
    ) -> Result<(ToolSession, Option<Lodging>)> {
        let agent = call.conversation.agent;
        let remote = match call.conversation.kind {
            Kind::Dream => None,
            _ => self.remote_session(agent, net)?,
        };
        Ok(match remote {
            Some(session) => (
                ToolSession::remote(place, session),
                self.lodging(agent)?,
            ),
            None => (
                ToolSession::local(
                    place,
                    net,
                    self.sandbox.0,
                    self.shells.clone(),
                    self.leftovers.of(agent.0),
                ),
                None,
            ),
        })
    }

    /// The images the model `agent` runs now takes.
    fn sight_of(&mut self, agent: AgentId) -> Result<Sight> {
        let folder = self.realm.layout().images();
        let db = self.realm.store().connection();
        let model = models::choose(
            db,
            &self.env.settings,
            &self.env.overrides,
            agent,
        )?
        .model;
        Ok(Sight {
            folder,
            limits: self
                .providers
                .images(db, &model)?
                .map(|takes| Limits::of(&takes)),
        })
    }

    /// Whether `tool` may not run twice: what writes, runs or is
    /// added by hand.
    fn runs_once(&self, tool: &str) -> bool {
        self.catalogue
            .iter()
            .find(|spec| spec.name == tool)
            .is_none_or(|spec| spec.replay == ToolSpecReplay::Unsafe)
    }

    /// Keeps the outcome of a tool run that ended and answers each
    /// runtime that asked.
    pub(crate) fn ran(&mut self, ran: Ran) -> Result<()> {
        let Some((entity, mut found)) = self
            .runs
            .iter_mut()
            .find(|(_, running)| running.call == ran.call)
        else {
            return Ok(());
        };
        let running = Ended {
            replies: std::mem::take(&mut found.replies),
            request: found.request.clone(),
        };
        self.commands.entity(entity).despawn();
        crate::fault::point("ran");
        let db = self.realm.store().connection();
        marker::settle(
            db,
            ran.call.identity(),
            &running.request,
            &ran.outcome,
            crate::clock::now(),
        )?;
        self.looks.forget(&ran.call);
        for reply in running.replies {
            self.tell_call(
                &reply,
                &ran.call.key,
                Answer::Result(ran.outcome.clone()),
            );
        }
        Ok(())
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

    /// Asks the chief the questions of `call`, or gives the chief's
    /// answer once there is one. `Err` is the runtime's answer while
    /// it waits or when it may not ask.
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
            }) => return Ok(Ok(said(text, false))),
            Some(State::Ruled { .. }) => {
                return Ok(Ok(said(
                    "The chief did not answer; go on with what you \
                     know.",
                    false,
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
        if !self.chief_oversees(call)? {
            return Ok(Ok(said(NO_CHIEF, false)));
        }
        let questions = match asking::read(&serde_json::to_string(
            call.arguments,
        )?) {
            Ok(questions) => questions,
            Err(unasked) => {
                return Ok(Ok(said(unasked.to_string(), true)));
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
        let mut asked =
            Asked::of(db, call.conversation.id, call.input)?;
        if asked.questions >= most {
            let times = if most == 1 {
                "once".into()
            } else {
                format!("{most} times")
            };
            return Ok(Ok(said(
                format!(
                    "You have asked the chief {times} this turn; go \
                     on with what you know."
                ),
                false,
            )));
        }
        asked.questions += 1;
        let (runtime, key) =
            (call.call.runtime.clone(), call.call.key.clone());
        let conversation = call.conversation.id;
        self.change(Actor::Realm, |cx, _| {
            asked.keep(cx.db, conversation)?;
            let wait = oversight::raise(
                cx,
                Raise {
                    kind: WaitKind::Question,
                    danger: None,
                    agent,
                    scholar: None,
                    conversation: Some(conversation),
                    task,
                    about: asking::about(&questions),
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

    /// Whether a chief oversees the turn that makes `call`: a chief's
    /// line, or a retry or reflection in a chief's chat.
    fn chief_oversees(&mut self, call: &Call<'_>) -> Result<bool> {
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

/// The refusal of a write or edit of `CHARACTER.md` at the lodge's
/// root, which is the scholar's own.
fn character_refusal(
    place: &crate::work::tools::Place,
    call: &Call<'_>,
) -> Option<String> {
    if !matches!(call.tool, "write" | "edit") {
        return None;
    }
    let path = call.arguments.get("path")?.as_str()?;
    let path = std::path::Path::new(path);
    let full = if path.is_absolute() {
        path.to_owned()
    } else {
        place.folder.as_deref().unwrap_or(&place.lodge).join(path)
    };
    (full == place.lodge.join(CHARACTER)).then(|| {
        format!(
            "{CHARACTER} is your scholar's character, which only a \
             chief changes. Keep what you learn in memory/ instead."
        )
    })
}
