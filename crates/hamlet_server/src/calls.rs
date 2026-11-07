//! What a runtime asks while a turn runs: whether a tool call may run
//! (C7, H11), and the call carried out once under its key (S19). A
//! chief's answer to what waits lets held calls go on.

mod answer;
mod held;
mod objections;
pub(crate) mod place;
pub(crate) mod reviews;
mod society;

use std::collections::{BTreeMap, HashMap};
use std::future::Future;

use anyhow::Result;
use hamlet_protocol::runtime::{
    self, ConfigureTools, ServerMessage, ToolSpec, ToolSpecReplay,
};
use hamlet_store::ids::{Actor, AgentId, ConversationId};
use hamlet_store::marker::{self, Identity, Marked, Request};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use self::held::State;
use self::objections::OBJECTING;
pub(crate) use self::society::library::LIBRARIAN;
pub(crate) use self::society::welcome;
use crate::core::Core;
use crate::images::{Limits, kept_pictures};
use crate::knowledge::CHARACTER;
use crate::oversight::gate::{
    self, Asked, Session, Standing, Verdict,
};
use crate::oversight::{self, Raise, Ruling, WaitKind};
use crate::talk::conversations::{Conversation, Kind};
use crate::talk::marks::Mark;
use crate::talk::{asking, conversations, inputs, models};
use crate::work::cut::change_between;
use crate::work::tools::{Dialect, Outcome, Sight, ToolSession};

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
    pub index: usize,
    pub generation: u64,
    pub re: String,
}

/// A tool run that ended.
pub(crate) struct Ran {
    call: CallId,
    outcome: Outcome,
}

/// What a runtime is answered about a call.
pub(crate) enum Answer {
    Allowed,
    Waiting(String),
    Refusal(String),
    Result(Outcome),
}

/// The tool runs in progress.
pub(crate) struct Calls {
    running: HashMap<CallId, Running>,
    ran: mpsc::UnboundedSender<Ran>,
}

struct Running {
    replies: Vec<Reply>,
    conversation: ConversationId,
    request: Request,
    /// The agent whose remote lodge the call may write, to pull once
    /// it ends.
    pull: Option<AgentId>,
    abort: AbortHandle,
}

impl Calls {
    pub fn new(ran: mpsc::UnboundedSender<Ran>) -> Self {
        Self {
            running: HashMap::new(),
            ran,
        }
    }

    fn run(
        &mut self,
        call: CallId,
        conversation: ConversationId,
        reply: Reply,
        request: Request,
        pull: Option<AgentId>,
        work: impl Future<Output = Outcome> + Send + 'static,
    ) {
        let ran = self.ran.clone();
        let id = call.clone();
        let task = tokio::spawn(async move {
            let outcome = work.await;
            let _ = ran.send(Ran { call: id, outcome });
        });
        self.running.insert(
            call,
            Running {
                replies: vec![reply],
                conversation,
                request,
                pull,
                abort: task.abort_handle(),
            },
        );
    }

    /// Answers `reply` too when `call` runs already.
    fn join(&mut self, call: &CallId, reply: &Reply) -> bool {
        match self.running.get_mut(call) {
            Some(running) => {
                running.replies.push(reply.clone());
                true
            }
            None => false,
        }
    }

    /// Stops the tool runs of `conversation`.
    pub fn stop(&mut self, conversation: ConversationId) {
        self.running.retain(|_, running| {
            let keep = running.conversation != conversation;
            if !keep {
                running.abort.abort();
            }
            keep
        });
    }

    /// The conversations whose tools run now.
    pub fn conversations(&self) -> Vec<ConversationId> {
        let mut out = self
            .running
            .values()
            .map(|running| running.conversation)
            .collect::<Vec<_>>();
        out.sort();
        out.dedup();
        out
    }

    pub fn stop_all(&mut self) {
        for running in self.running.values() {
            running.abort.abort();
        }
        self.running.clear();
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
    specs.extend(society::specs());
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
#[derive(Debug)]
enum Decision {
    Run { net: bool },
    Block(String),
    Wait(&'static str),
}

impl Core {
    /// Answers `reply` about the call `key`, unless its runtime
    /// started again since it asked.
    async fn tell_call(
        &mut self,
        reply: &Reply,
        key: &str,
        answer: Answer,
    ) {
        if self.runtimes.generation(reply.index)
            != Some(reply.generation)
        {
            return;
        }
        let re = reply.re.clone();
        let key = key.to_owned();
        let images_folder = self.realm.layout().images();
        self.runtimes
            .send(reply.index, |v, id| match answer {
                Answer::Allowed => {
                    ServerMessage::Allowed(runtime::Allowed {
                        id,
                        re,
                        v,
                    })
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
                        diagnostics: (!outcome
                            .diagnostics
                            .is_empty())
                        .then_some(outcome.diagnostics),
                        error: outcome.error,
                        id,
                        re,
                        text: outcome.text,
                        v,
                    })
                }
            })
            .await;
    }

    /// The call a runtime asked, with its key reserved for its
    /// request; the reply to give when the call cannot go on.
    fn admit<'a>(
        &mut self,
        index: usize,
        conversation: &str,
        key: &str,
        input: Option<&'a str>,
        tool: &'a str,
        arguments: &'a BTreeMap<String, Value>,
    ) -> Result<std::result::Result<Call<'a>, Answer>> {
        let runtime = self.runtimes.list[index].id.clone();
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
            arguments,
            request,
        }))
    }

    fn reply_of(&self, index: usize, re: &str) -> Reply {
        Reply {
            index,
            generation: self.runtimes.generation(index).unwrap_or(0),
            re: re.into(),
        }
    }

    /// Whether the call `asked` may run, before it is kept as begun.
    pub(crate) async fn allow_call(
        &mut self,
        index: usize,
        asked: runtime::Allow,
    ) -> Result<()> {
        let reply = self.reply_of(index, &asked.id);
        let call = match self.admit(
            index,
            &asked.conversation,
            &asked.key,
            asked.input.as_deref(),
            &asked.tool,
            &asked.arguments,
        )? {
            Ok(call) => call,
            Err(answer) => {
                self.tell_call(&reply, &asked.key, answer).await;
                return Ok(());
            }
        };
        if call.tool != "object" && self.withdrawn(&call)? {
            self.tell_call(
                &reply,
                &asked.key,
                Answer::Refusal(held::WITHDRAWN.into()),
            )
            .await;
            return Ok(());
        }
        if self.objecting(&call)? {
            self.tell_call(
                &reply,
                &asked.key,
                Answer::Waiting(OBJECTING.into()),
            )
            .await;
            return Ok(());
        }
        let answer = if !offers(&call.conversation, call.tool)?
            || call.tool == "ask_chief"
            || call.tool == "object"
        {
            Answer::Allowed
        } else {
            match self.decide(&call).await? {
                Decision::Run { .. } => Answer::Allowed,
                Decision::Block(words) => Answer::Refusal(words),
                Decision::Wait(words) => {
                    Answer::Waiting(words.into())
                }
            }
        };
        self.tell_call(&reply, &asked.key, answer).await;
        Ok(())
    }

    /// Carries out the call `asked`: its kept result when it ran,
    /// else as the gate decides now.
    pub(crate) async fn operation(
        &mut self,
        index: usize,
        asked: runtime::Operation,
    ) -> Result<()> {
        let reply = self.reply_of(index, &asked.id);
        let call = match self.admit(
            index,
            &asked.conversation,
            &asked.key,
            asked.input.as_deref(),
            &asked.tool,
            &asked.arguments,
        )? {
            Ok(call) => call,
            Err(answer) => {
                self.tell_call(&reply, &asked.key, answer).await;
                return Ok(());
            }
        };
        if self.calls.join(&call.call, &reply) {
            return Ok(());
        }
        let db = self.realm.store().connection();
        let tool = call.tool;
        let answer = match marker::read::<Outcome>(
            db,
            call.call.identity(),
            &call.request,
        )? {
            Marked::Begun => Some(Answer::Result(Outcome::thrown(
                "tool_interrupted",
                format!(
                    "Tool {tool} was interrupted by a restart and \
                     may have partially run"
                ),
            ))),
            Marked::Outcome(outcome) => Some(Answer::Result(outcome)),
            Marked::Conflict => {
                Some(Answer::Refusal(CONFLICT.into()))
            }
            Marked::Unmarked => None,
        };
        if let Some(answer) = answer {
            self.tell_call(&reply, &asked.key, answer).await;
            return Ok(());
        }
        let outcome = if !offers(&call.conversation, tool)? {
            Some(unavailable(tool))
        } else if tool == "ask_chief" || tool == "object" {
            let ruled = if tool == "object" {
                self.object(&call).await?
            } else {
                self.ask_chief(&call).await?
            };
            match ruled {
                Ok(outcome) => Some(outcome),
                Err(answer) => {
                    self.tell_call(&reply, &asked.key, answer).await;
                    return Ok(());
                }
            }
        } else if self.withdrawn(&call)? {
            Some(said(held::WITHDRAWN, true))
        } else if self.objecting(&call)? {
            self.tell_call(
                &reply,
                &asked.key,
                Answer::Waiting(OBJECTING.into()),
            )
            .await;
            return Ok(());
        } else {
            match self.decide(&call).await? {
                Decision::Block(words) => {
                    self.tell_call(
                        &reply,
                        &asked.key,
                        Answer::Refusal(words),
                    )
                    .await;
                    return Ok(());
                }
                Decision::Wait(words) => {
                    self.tell_call(
                        &reply,
                        &asked.key,
                        Answer::Waiting(words.into()),
                    )
                    .await;
                    return Ok(());
                }
                Decision::Run { net } => {
                    if crate::work::catalogue::SOCIETY.contains(&tool)
                    {
                        match self.society(&call).await? {
                            society::Step::Done(outcome) => {
                                crate::fault::point("carried");
                                Some(outcome)
                            }
                            society::Step::Wait(words) => {
                                self.tell_call(
                                    &reply,
                                    &asked.key,
                                    Answer::Waiting(words),
                                )
                                .await;
                                return Ok(());
                            }
                        }
                    } else if let Some(terminal) =
                        crate::terminals::Tool::named(tool)
                    {
                        self.run_terminal(
                            &call,
                            reply.clone(),
                            terminal,
                            net,
                        )?
                    } else {
                        self.run_tool(
                            &call,
                            reply.clone(),
                            call.request.clone(),
                            net,
                        )
                        .await?
                    }
                }
            }
        };
        if let Some(outcome) = outcome {
            let db = self.realm.store().connection();
            marker::settle(
                db,
                call.call.identity(),
                &call.request,
                &outcome,
                crate::clock::now(),
            )?;
            self.tell_call(
                &reply,
                &asked.key,
                Answer::Result(outcome),
            )
            .await;
        }
        Ok(())
    }

    /// Runs the built-in or added tool of `call` with the network
    /// `net`, after marking it begun when it may not run again. Gives
    /// the outcome of a call refused before it runs.
    async fn run_tool(
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
        let commands = match Dialect::of_tool(call.tool) {
            Some(_) => {
                let command = call
                    .arguments
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                Some(self.commands(agent, &at, command)?)
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
        let (session, pull) =
            match self.tools_of(call, at.place, net).await? {
                Ok(found) => found,
                Err(words) => return Ok(Some(said(words, true))),
            };
        let session = session.seeing(self.sight_of(agent)?);
        let secrets = self.credentials.secrets();
        let arguments =
            call.arguments.clone().into_iter().collect::<Map<_, _>>();
        let name = call.tool.to_owned();
        self.calls.run(
            call.call.clone(),
            call.conversation.id,
            reply,
            request,
            (pull && once).then_some(agent),
            async move {
                let mut outcome = session
                    .run(
                        &secrets,
                        &name,
                        &arguments,
                        commands.as_ref(),
                    )
                    .await;
                if let Some(commands) = &commands {
                    let code = outcome
                        .details
                        .as_ref()
                        .and_then(|details| details.get("exitCode"))
                        .and_then(Value::as_i64);
                    commands.ended_run(code);
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
        Ok(None)
    }

    /// Runs the terminal tool `tool` of `call` with the network
    /// `net`, after marking it begun when it may not run again.
    fn run_terminal(
        &mut self,
        call: &Call<'_>,
        reply: Reply,
        tool: crate::terminals::Tool,
        net: bool,
    ) -> Result<Option<Outcome>> {
        if self.runs_once(call.tool) {
            marker::begin(
                self.realm.store().connection(),
                call.call.identity(),
                &call.request,
                crate::clock::now(),
            )?;
        }
        let work = crate::terminals::call(
            self,
            call.conversation.agent,
            tool,
            call.arguments,
            net,
        )?;
        self.calls.run(
            call.call.clone(),
            call.conversation.id,
            reply,
            call.request.clone(),
            None,
            work,
        );
        Ok(None)
    }

    /// Where the tools of `call`'s agent run: in the remote workspace
    /// the realm works in, whose lodge is pulled home after a call
    /// that may write it, else on this machine. A dream works on this
    /// machine's checkout of its scholar, so it runs here. Gives the
    /// words of a lodge the workspace could not take in.
    async fn tools_of(
        &mut self,
        call: &Call<'_>,
        place: crate::work::tools::Place,
        net: bool,
    ) -> Result<Result<(ToolSession, bool), String>> {
        let agent = call.conversation.agent;
        let remote = match call.conversation.kind {
            Kind::Dream => None,
            _ => self.remote_session(agent, net)?,
        };
        Ok(Ok(match remote {
            Some(session) => {
                if let Err(error) = self.lodge_in_step(agent).await {
                    return Ok(Err(self.credentials.scrub_text(
                        &format!(
                            "The remote workspace could not take \
                             the lodge in: {error:#}"
                        ),
                    )));
                }
                (ToolSession::remote(place, session), true)
            }
            None => (
                ToolSession::local(
                    place,
                    net,
                    self.sandbox,
                    self.shells.clone(),
                    self.leftovers.of(agent.0),
                ),
                false,
            ),
        }))
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
    pub(crate) async fn ran(&mut self, ran: Ran) -> Result<()> {
        let Some(running) = self.calls.running.remove(&ran.call)
        else {
            return Ok(());
        };
        crate::fault::point("ran");
        let db = self.realm.store().connection();
        marker::settle(
            db,
            ran.call.identity(),
            &running.request,
            &ran.outcome,
            crate::clock::now(),
        )?;
        // The lodge comes home before the agent's next call reads it.
        if let Some(agent) = running.pull
            && let Err(error) = self.pull_lodge(agent).await
        {
            crate::diagnostic::tell(
                crate::diagnostic::Secrets::Of(&self.credentials),
                format!("could not bring a lodge home: {error:#}"),
            );
        }
        for reply in running.replies {
            self.tell_call(
                &reply,
                &ran.call.key,
                Answer::Result(ran.outcome.clone()),
            )
            .await;
        }
        Ok(())
    }

    /// The gate's decision of `call`: the one kept, else one made now
    /// and kept, raising a wait when it asks a chief.
    async fn decide(&mut self, call: &Call<'_>) -> Result<Decision> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        if let Some(kept) =
            held::kept(db, &call.call.runtime, &call.call.key)?
        {
            match kept.state {
                State::Blocked(words) => {
                    return Ok(Decision::Block(words));
                }
                State::Ruled { ruling, net, .. }
                    if ruling.is_allowing() =>
                {
                    return Ok(Decision::Run { net });
                }
                State::Ruled {
                    ruling: Ruling::Deny,
                    text,
                    ..
                } => {
                    return Ok(Decision::Block(format!(
                        "the chief refused it: {}",
                        if text.is_empty() {
                            "no reason given"
                        } else {
                            &text
                        }
                    )));
                }
                State::Ruled {
                    ruling: Ruling::Stopped,
                    ..
                } => {
                    return Ok(Decision::Block(STOPPED.into()));
                }
                State::Ruled { .. } => {
                    return Ok(Decision::Block(NOBODY.into()));
                }
                State::Held(_) => return Ok(Decision::Wait(WAITING)),
                State::Objected => {
                    return Ok(Decision::Wait(OBJECTING));
                }
                State::Allowed { .. } | State::Open => {}
            }
        }
        if call.conversation.cancelling {
            let db = self.realm.store().connection();
            held::block(
                db,
                &call.call.runtime,
                &call.call.key,
                STOPPED,
                None,
            )?;
            return Ok(Decision::Block(STOPPED.into()));
        }
        let agent = call.conversation.agent;
        let at =
            place::of_conversation(db, &layout, &call.conversation)?;
        let added = self
            .added
            .iter()
            .find(|manifest| manifest.spec.name == call.tool)
            .map(crate::work::added::Manifest::has_net);
        let mut classified =
            gate::call_of(call.tool, &call.args(), added);
        let session =
            match self.tools_of(call, at.place.clone(), false).await?
            {
                Ok((session, _)) => session,
                Err(words) => {
                    let db = self.realm.store().connection();
                    held::block(
                        db,
                        &call.call.runtime,
                        &call.call.key,
                        &words,
                        None,
                    )?;
                    return Ok(Decision::Block(words));
                }
            };
        if let gate::Call::Society { name, args } = &classified
            && matches!(call.tool, "tell" | "ask")
        {
            let paths = society::list(call, "paths");
            if !paths.is_empty() {
                let (name, args) = (name.clone(), args.clone());
                match session.parcel(&paths, None).await? {
                    Ok(sent) => {
                        classified = gate::Call::Parcel {
                            name,
                            args,
                            files: sent.files,
                            bytes: sent.bytes,
                        };
                    }
                    Err(refusal) => {
                        let words = refusal.to_string();
                        let db = self.realm.store().connection();
                        held::block(
                            db,
                            &call.call.runtime,
                            &call.call.key,
                            &words,
                            None,
                        )?;
                        return Ok(Decision::Block(words));
                    }
                }
            }
        }
        if let gate::Call::Edit {
            write: true,
            path,
            old,
            created,
            ..
        } = &mut classified
        {
            match session.preview(path).await {
                Ok(Some(before)) => {
                    *old = before;
                    *created = false;
                }
                Ok(None) => {}
                Err(unread) => {
                    let words = unread.to_string();
                    let db = self.realm.store().connection();
                    held::block(
                        db,
                        &call.call.runtime,
                        &call.call.key,
                        &words,
                        None,
                    )?;
                    return Ok(Decision::Block(words));
                }
            }
        }
        let (change, lodge_edit) = match &classified {
            gate::Call::Edit {
                write,
                path,
                old,
                now,
                created,
            } => {
                let mut change =
                    serde_json::to_value(change_between(
                        path,
                        old,
                        now,
                        *write && *created,
                    ))?;
                self.credentials.scrub(&mut change);
                (
                    Some(change.to_string()),
                    session.is_lodge_edit(path),
                )
            }
            _ => (None, false),
        };
        let about = self
            .credentials
            .scrub_text(&gate::call_text(&classified));
        let allow_net = self.env.settings.allow_net;
        let task = self.task_of(call)?;
        let db = self.realm.store().connection();
        let limits = society::limits_of(db, at.hamlet.as_ref())?;
        let (edit, command) = match chat_session(&call.conversation) {
            Some(session) => gate::trusted(db, session)?,
            None => (false, false),
        };
        let standing = Standing {
            lodge_edit,
            net: allow_net
                || gate::has_grant(
                    db,
                    agent,
                    &hamlet_protocol::client::Permission::Network,
                )?,
            asks_edits: at.manual && !edit,
            asks_commands: at.manual && !command,
        };
        let asked = Asked::of(db, call.conversation.id, call.input)?;
        let verdict = gate::rule(
            &classified,
            &standing,
            at.caller,
            &limits.reserved,
            &asked,
            &about,
            limits.asking.refusals,
        );
        let (runtime, key) =
            (call.call.runtime.clone(), call.call.key.clone());
        let conversation = call.conversation.id;
        let decision = self
            .change(Actor::Realm, |cx, _| {
                let change = change.as_deref();
                Ok(match verdict {
                    Verdict::Run { net } => {
                        held::allow(
                            cx.db, &runtime, &key, net, change,
                        )?;
                        Decision::Run { net }
                    }
                    Verdict::Block(words) => {
                        asked.keep(cx.db, conversation)?;
                        held::block(
                            cx.db, &runtime, &key, &words, change,
                        )?;
                        Decision::Block(words)
                    }
                    Verdict::Ask(danger) => {
                        asked.keep(cx.db, conversation)?;
                        let wait = oversight::raise(
                            cx,
                            Raise {
                                kind: WaitKind::Approval,
                                danger: Some(danger),
                                agent,
                                scholar: None,
                                conversation: Some(conversation),
                                task,
                                about,
                                change: change.map(String::from),
                                answers: danger.answers(),
                                questions: None,
                            },
                        )?;
                        held::hold(
                            cx.db, &runtime, &key, wait, change,
                        )?;
                        Decision::Wait(WAITING)
                    }
                })
            })
            .await?;
        Ok(decision)
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
    async fn ask_chief(
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
        })
        .await?;
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
