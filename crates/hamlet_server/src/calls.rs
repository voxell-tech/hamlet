//! A runtime's requests while a turn runs: whether a tool call may
//! run (C7, H11), and the call carried out once under its key (S19).
//! A chief's answer to what waits lets held calls go on.

mod answer;
mod deciding;
mod held;
pub(crate) mod looks;
mod objections;
pub(crate) mod place;
pub(crate) mod reviews;
pub(crate) mod society;

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Mutex, PoisonError};

use anyhow::Result;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::system::{Commands, Query};
use bevy_ecs::world::DeferredWorld;
use bevy_log::warn;
use futures::future::BoxFuture;
use hamlet_protocol::runtime::{
    self, ConfigureTools, RuntimeMessage, ServerMessage, ToolSpec,
    ToolSpecReplay,
};
use hamlet_store::ids::{Actor, AgentId, ConversationId};
use hamlet_store::marker::{self, Identity, Marked, Request};
use serde_json::{Map, Value, json};
use tokio::task::AbortHandle;

use self::held::State;
use self::looks::Look;
use self::objections::OBJECTING;
use crate::app::input::{AddHandler, InputMessage, InputSender};
use crate::app::tasks::{BackgroundResult, RunAgainRequest};
use crate::conversations::marks::Mark;
use crate::conversations::{
    self, Conversation, Kind, inputs, models, questions,
};
use crate::core::Core;
use crate::images::Limits;
use crate::images::fitting::kept_pictures;
use crate::knowledge::CHARACTER;
use crate::oversight::gate::{Session, TurnGate};
use crate::oversight::{self, Raise, Ruling, WaitKind};
use crate::work::catalogue::Society;
use crate::work::tools::shell::Dialect;
use crate::work::tools::{Network, Outcome, Sight, ToolSession};
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

/// Where ended tool runs go, and the system that takes them in.
pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_handler::<ToolRunEnded, _>(take_in_ran);
}

/// A tool run ended.
#[derive(bevy_ecs::message::Message)]
pub(crate) struct ToolRunEnded(pub Ran);

/// Takes in a tool run that ended.
fn take_in_ran(
    mut ended: InputMessage<ToolRunEnded>,
    mut core: Core,
) -> bevy_ecs::error::Result {
    if let Some(ToolRunEnded(ran)) = ended.take() {
        core.ran(ran)?;
    }
    Ok(())
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

/// A tool run that ended.
pub(crate) struct Ran {
    call: CallId,
    outcome: Outcome,
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

/// A tool run in progress: an entity of its own, despawned when it
/// ends or its turn is stopped. Its task stops with it.
#[derive(Component)]
#[component(on_remove = stopped)]
pub(crate) struct ToolRun {
    call: CallId,
    replies: Vec<Reply>,
    pub conversation: ConversationId,
    request: Request,
    /// The run's task, once started.
    abort: Option<AbortHandle>,
}

/// The work of a run not started yet: it starts once the input that
/// requested it is kept, so a failed input runs nothing.
#[derive(Component)]
pub(crate) struct UnstartedToolRun(
    Mutex<Option<BoxFuture<'static, ()>>>,
);

/// Starts the work of every run its input kept.
pub(crate) fn start(
    mut runs: Query<(Entity, &mut ToolRun, &mut UnstartedToolRun)>,
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
        commands.entity(entity).remove::<UnstartedToolRun>();
    }
}

/// The replies and request an ended run still owes.
struct EndedToolRun {
    replies: Vec<Reply>,
    request: Request,
}

/// A run's entity went: its task, if still running, stops.
fn stopped(world: DeferredWorld, context: HookContext) {
    if let Some(abort) = world
        .get::<ToolRun>(context.entity)
        .and_then(|running| running.abort.as_ref())
    {
        abort.abort();
    }
}

/// Every tool run in progress.
pub(crate) type ToolRuns<'w, 's> =
    Query<'w, 's, (Entity, &'static mut ToolRun)>;

/// The run of `work` for `call`, to spawn; it starts once its input
/// is kept, and its end goes to `inputs`.
fn tool_run(
    inputs: &InputSender,
    call: CallId,
    conversation: ConversationId,
    reply: Reply,
    request: Request,
    work: impl Future<Output = Outcome> + Send + 'static,
) -> (ToolRun, UnstartedToolRun) {
    let inputs = inputs.clone();
    let id = call.clone();
    let work = async move {
        let outcome = work.await;
        let _ = inputs.send(ToolRunEnded(Ran { call: id, outcome }));
    };
    let running = ToolRun {
        call,
        replies: vec![reply],
        conversation,
        request,
        abort: None,
    };
    (running, UnstartedToolRun(Mutex::new(Some(Box::pin(work)))))
}

/// The tools the server adds to every runtime's own: `ask_chief` and
/// the society's.
pub(crate) fn specs() -> Vec<ToolSpec> {
    let mut specs = vec![spec(
        "ask_chief",
        "Put a question to the chief that you cannot go on without. \
         Waits for the answer, so use it only when no agent and no \
         file can tell you. Use object to argue against an \
         instruction.",
        json!({
            "type": "object",
            "properties": {
                "message": {"type": "string"},
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": questions::MOST_QUESTIONS,
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": {"type": "string"},
                            "description": {"type": "string"},
                            "options": {
                                "type": "array",
                                "maxItems": questions::MOST_OPTIONS,
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

fn is_offered(
    conversation: &Conversation,
    tool: &str,
) -> Result<bool> {
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

    /// The call a runtime made, with its key reserved for its
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

    fn reply_of(&self, from: Entity, request: &str) -> Reply {
        Reply {
            to: from,
            generation: self.runtimes.generation(from).unwrap_or(0),
            request: request.into(),
        }
    }

    /// Answers a runtime whether the call `request` may run, before
    /// it is kept as begun.
    pub(crate) fn allow_call(
        &mut self,
        from: Entity,
        request: runtime::Allow,
    ) -> Result<()> {
        let again = RuntimeMessage::Allow(request.clone());
        let reply = self.reply_of(from, &request.id);
        let call = match self.admit(
            from,
            &request.conversation,
            &request.key,
            request.input.as_deref(),
            &request.tool,
            &request.arguments,
        )? {
            Ok(call) => call,
            Err(answer) => {
                self.tell_call(&reply, &request.key, answer);
                return Ok(());
            }
        };
        if call.society != Some(Society::Object)
            && self.is_withdrawn(&call)?
        {
            self.tell_call(
                &reply,
                &request.key,
                Answer::Refusal(held::WITHDRAWN.into()),
            );
            return Ok(());
        }
        if self.is_objecting(&call)? {
            self.tell_call(
                &reply,
                &request.key,
                Answer::Waiting(OBJECTING.into()),
            );
            return Ok(());
        }
        let answer = if !is_offered(&call.conversation, call.tool)?
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
        self.tell_call(&reply, &request.key, answer);
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
        let again = RunAgainRequest {
            from,
            generation: self.runtimes.generation(from).unwrap_or(0),
            message: again,
        };
        let id = call.call.clone();
        self.later.spawn(async move {
            BackgroundResult::Looked {
                call: id,
                found: look.run(secrets).await,
                again,
            }
        });
    }

    /// Carries out the call `request`: its kept result when it ran,
    /// else as the gate decides now.
    pub(crate) fn operation(
        &mut self,
        from: Entity,
        request: runtime::Operation,
    ) -> Result<()> {
        let again = RuntimeMessage::Operation(request.clone());
        let reply = self.reply_of(from, &request.id);
        let call = match self.admit(
            from,
            &request.conversation,
            &request.key,
            request.input.as_deref(),
            &request.tool,
            &request.arguments,
        )? {
            Ok(call) => call,
            Err(answer) => {
                self.tell_call(&reply, &request.key, answer);
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
            self.tell_call(&reply, &request.key, answer);
            return Ok(());
        }
        match self.carry_out(&call, &reply, from, again)? {
            CallOutcome::Done(outcome) => {
                let db = self.realm.store().connection();
                marker::settle(
                    db,
                    call.call.identity(),
                    &call.request,
                    &outcome,
                    crate::app::clock::now(),
                )?;
                self.looks.forget(&call.call);
                self.tell_call(
                    &reply,
                    &request.key,
                    Answer::Result(outcome),
                );
            }
            CallOutcome::Answer(answer) => {
                self.tell_call(&reply, &request.key, answer);
            }
            CallOutcome::Later => {}
        }
        Ok(())
    }

    /// The answer kept for `call` from when it was made before: its
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
    ) -> Result<CallOutcome> {
        let tool = call.tool;
        if !is_offered(&call.conversation, tool)? {
            return Ok(CallOutcome::Done(unavailable(tool)));
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
                Ok(outcome) => CallOutcome::Done(outcome),
                Err(answer) => CallOutcome::Answer(answer),
            });
        }
        if self.is_withdrawn(call)? {
            return Ok(CallOutcome::Done(refused(held::WITHDRAWN)));
        }
        if self.is_objecting(call)? {
            return Ok(CallOutcome::Answer(Answer::Waiting(
                OBJECTING.into(),
            )));
        }
        Ok(match self.decide(call)? {
            Decision::Look(look) => {
                self.look(call, look, from, again);
                CallOutcome::Later
            }
            Decision::Block(words) => {
                CallOutcome::Answer(Answer::Refusal(words))
            }
            Decision::Wait(words) => {
                CallOutcome::Answer(Answer::Waiting(words.into()))
            }
            Decision::Run { net } => match call.society {
                Some(society) => match self.society(call, society)? {
                    society::Step::Done(outcome) => {
                        CallOutcome::Done(outcome)
                    }
                    society::Step::Wait(words) => {
                        CallOutcome::Answer(Answer::Waiting(words))
                    }
                    society::Step::Look(look) => {
                        self.look(call, look, from, again);
                        CallOutcome::Later
                    }
                },
                None => {
                    let request = call.request.clone();
                    match self.run_tool(
                        call,
                        reply.clone(),
                        request,
                        Network::of(net),
                    )? {
                        Some(outcome) => CallOutcome::Done(outcome),
                        None => CallOutcome::Later,
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
        net: Network,
    ) -> Result<Option<Outcome>> {
        let once = self.is_once_only(call.tool);
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let agent = call.conversation.agent;
        let at =
            place::of_conversation(db, &layout, &call.conversation)?;
        if let Some(refusal) = character_refusal(&at.place, call) {
            return Ok(Some(refused(refusal)));
        }
        if once {
            marker::begin(
                db,
                call.call.identity(),
                &request,
                crate::app::clock::now(),
            )?;
        }
        // A shell call shows in its agent's commands terminal.
        let shown = match Dialect::of_tool(call.tool) {
            Some(_) => {
                let command = call
                    .arguments
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                Some(self.commands_terminal(agent, &at, command)?)
            }
            None => None,
        };
        let db = self.realm.store().connection();
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
        let running = tool_run(
            &self.inputs,
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
                    return refused(
                        hamlet_sandbox::scrub::scrub_text(
                            &secrets,
                            &format!(
                                "The remote workspace could not \
                                 take the lodge in: {error:#}"
                            ),
                        ),
                    );
                }
                let mut outcome = session
                    .run(&secrets, &name, &arguments, shown.as_ref())
                    .await;
                if let Some(shown) = &shown {
                    let code = outcome
                        .details
                        .as_ref()
                        .and_then(|details| details.get("exitCode"))
                        .and_then(Value::as_i64);
                    shown.ended_run(code);
                }
                if once
                    && let Some(lodging) = &lodging
                    && let Err(error) = lodging.pull().await
                {
                    let words = hamlet_sandbox::scrub::scrub_text(
                        &secrets,
                        &format!("{error:#}"),
                    );
                    warn!("could not bring a lodge home: {words}");
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

    /// The session the tools of `call`'s agent run in: the remote
    /// workspace the realm works in, with the lodge's remote copy
    /// kept in step, else this machine.
    fn tools_of(
        &mut self,
        call: &Call<'_>,
        place: crate::work::tools::Place,
        network: Network,
    ) -> Result<(ToolSession, Option<Lodging>)> {
        let agent = call.conversation.agent;
        let remote = match call.conversation.kind {
            // A dream works on this machine's checkout of its
            // scholar.
            Kind::Dream => None,
            _ => self.remote.session(
                self.realm.store().connection(),
                agent,
                network,
            )?,
        };
        Ok(match remote {
            Some(session) => (
                ToolSession::remote(place, session),
                self.remote.lodging(
                    &mut self.realm,
                    &self.writing,
                    agent,
                )?,
            ),
            None => (
                ToolSession::local(
                    place,
                    network,
                    self.tools.sandbox.0,
                    self.tools.shells.clone(),
                    self.tools.leftovers.of(agent.0),
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

    /// Tells whether `tool` may not run twice: what writes, runs or
    /// is added by hand.
    fn is_once_only(&self, tool: &str) -> bool {
        self.tools
            .catalogue
            .iter()
            .find(|spec| spec.name == tool)
            .is_none_or(|spec| spec.replay == ToolSpecReplay::Unsafe)
    }

    /// Keeps the outcome of a tool run that ended and answers each
    /// runtime that made it.
    pub(crate) fn ran(&mut self, ran: Ran) -> Result<()> {
        let Some((entity, mut found)) = self
            .runs
            .iter_mut()
            .find(|(_, running)| running.call == ran.call)
        else {
            return Ok(());
        };
        let running = EndedToolRun {
            replies: std::mem::take(&mut found.replies),
            request: found.request.clone(),
        };
        self.commands.entity(entity).despawn();
        crate::app::fault::point("ran");
        let db = self.realm.store().connection();
        marker::settle(
            db,
            ran.call.identity(),
            &running.request,
            &ran.outcome,
            crate::app::clock::now(),
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
                    scholar: None,
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
