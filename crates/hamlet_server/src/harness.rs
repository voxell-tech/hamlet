//! Harness runtimes: started and watched by this server, talking to
//! it over the harness link, with the conversations bound to them.
//!
//! The host owns the [`Store`] and the loop: it waits on
//! [`Harnesses::next`] and passes each event to
//! [`Harnesses::handle`], which answers the runtime and returns what
//! the host may show.

pub(crate) mod answers;
mod bindings;
mod credentials;
// Generated, so formatted as the generator writes it.
#[rustfmt::skip]
pub mod link;
mod process;
mod session;
mod sync;
#[cfg(test)]
mod tests;
pub mod tools;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
pub use bindings::{
    Binding, BindingId, DEFAULT_HARNESS, Input, Standing,
};
use link::{
    Capability, Diagnostic, HelloStore, Item, Mark, ModelRef,
    Partial, RuntimeMessage, Section, ServerMessage, ToolSpec,
};
pub use process::Pi;
use process::{Launch, Process, Received};
use serde_json::Value;
use session::{Pending, Session, id_of, parse};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::credentials::Credentials;
use crate::record::AgentRef;
use crate::{Layout, RuntimeInstance, Store};

/// The link version this server speaks.
pub const LINK_VERSION: i64 = 1;

/// What a runtime must be able to do to be given Hamlet's work (S24).
const NEEDED: &[Capability] = &[
    Capability::Operations,
    Capability::Attempts,
    Capability::Admission,
    Capability::Cancel,
    Capability::Recovery,
    Capability::Updates,
];

/// The first wait before starting a failed runtime again; each
/// failure doubles it, up to [`RESTART_MOST`].
const RESTART_FIRST: Duration = Duration::from_millis(250);
const RESTART_MOST: Duration = Duration::from_secs(30);
/// How long a runtime must have run for its failures to be forgotten.
const RESTART_FORGIVEN: Duration = Duration::from_secs(60);
/// How long a stopping runtime has before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(5);
/// Entries asked for at once when catching up on a conversation.
const SYNC_PAGE: i64 = 200;

/// The kind of an entry written for people alone.
const NOTICE: &str = "hamlet.notice";

/// How this server starts runtimes and what it gives them.
#[derive(Clone, Debug)]
pub struct Config {
    pub pi: Pi,
    /// The model new conversations run, if one is set.
    pub model: Option<ModelRef>,
    pub tools: Vec<ToolSpec>,
}

impl Config {
    /// The configuration the environment gives: the pi harness's,
    /// `HAMLET_MODEL` and `HAMLET_BASE_URL`, with `faux` in place of
    /// a model when given.
    pub fn from_env(faux: Option<PathBuf>) -> Result<Self> {
        let set =
            |name| std::env::var(name).ok().filter(|v| !v.is_empty());
        let pi = Pi::from_env(faux)?;
        let model = if let Some(faux) = pi.faux_model() {
            Some(faux)
        } else if let Some(name) = set("HAMLET_MODEL") {
            let Some((provider, id)) = name
                .split_once("::")
                .filter(|(p, id)| !p.is_empty() && !id.is_empty())
            else {
                bail!("HAMLET_MODEL is `endpoint::id`, not `{name}`");
            };
            Some(ModelRef {
                provider: provider.into(),
                id: id.into(),
                base_url: set("HAMLET_BASE_URL"),
            })
        } else {
            None
        };
        Ok(Self {
            pi,
            model,
            tools: crate::tools::specs()
                .into_iter()
                .chain(crate::chats::specs())
                .chain(tools::specs())
                .chain(crate::society::specs())
                .collect(),
        })
    }
}

/// Something that happened that the host may show or act on.
// Each is moved to the host once, never kept in bulk.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    Provider(link::ProviderResult),
    ProviderEvent(link::ProviderEvent),
    Started {
        runtime: RuntimeInstance,
        pid: u32,
    },
    /// The runtime passed its checks and carries on its own work.
    Ready {
        runtime: RuntimeInstance,
    },
    /// The runtime was refused, in words for a chief, and is not
    /// started again.
    Refused {
        runtime: RuntimeInstance,
        words: String,
    },
    /// The runtime ended; it starts again after `restart_in` unless
    /// it was refused or the harnesses close.
    Exited {
        runtime: RuntimeInstance,
        status: String,
        restart_in: Option<Duration>,
    },
    Created {
        binding: BindingId,
    },
    /// The runtime kept the input `request`; `standing` is how it
    /// stands now.
    Admitted {
        binding: BindingId,
        request: String,
        standing: Standing,
    },
    /// The input `request` ended.
    Settled {
        binding: BindingId,
        request: String,
        standing: Standing,
    },
    /// An entry the server had not kept before.
    Kept {
        binding: BindingId,
        item: Item,
    },
    /// What the conversation is doing now.
    Live {
        binding: BindingId,
        running: bool,
        pending: i64,
        partial: Option<Partial>,
    },
    /// A model call of the conversation was allowed.
    Attempted {
        binding: BindingId,
    },
    /// The runtime asks what only the host can answer, with
    /// [`Harnesses::answer`].
    Call {
        binding: BindingId,
        re: Re,
        call: Call,
    },
    Cancelled {
        binding: BindingId,
    },
    Page {
        binding: BindingId,
        items: Vec<Item>,
        earlier: bool,
    },
    /// The runtime could not handle a message, or sent one that is
    /// not of the link.
    Fault {
        runtime: RuntimeInstance,
        words: String,
    },
}

/// What a runtime asks of the host for one of its conversations.
// Each is moved to the host once, never kept in bulk.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    /// Whether a tool call may run, asked before it is kept as begun.
    Allow {
        key: String,
        input: Option<String>,
        tool: String,
        arguments: BTreeMap<String, Value>,
    },
    /// A tool call to carry out under its operation key.
    Operation {
        key: String,
        input: Option<String>,
        tool: String,
        arguments: BTreeMap<String, Value>,
    },
    /// The prompt sections for the next model call.
    Prompt { input: Option<String> },
    /// Whether a final answer stands.
    Yield {
        input: Option<String>,
        answer: Item,
        history: bool,
        retried: bool,
        summary_failure: Option<String>,
    },
}

/// The message a [`Call`] came in, to answer it by.
#[derive(Clone, Debug, PartialEq)]
pub struct Re {
    index: usize,
    generation: u64,
    id: String,
    wake: Wake,
}

/// What ends the wait of a call answered [`Answer::Waiting`].
#[derive(Clone, Debug, PartialEq)]
enum Wake {
    /// A chief's answer to the operation `key`.
    Operation(String),
    /// A chief's ruling on what holds the turns of a conversation.
    Turn(BindingId),
}

impl Wake {
    /// The key the runtime is told the wait ends by.
    fn key(&self) -> String {
        match self {
            Self::Operation(key) => key.clone(),
            Self::Turn(binding) => format!("turn:{}", binding.0),
        }
    }
}

/// The host's answer to a [`Call`].
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Allowed,
    /// The call waits for a chief; nothing of it is marked.
    Waiting(String),
    /// The call may not run, in words for the agent.
    Refusal(String),
    /// The marked result of an operation.
    Result {
        text: String,
        error: bool,
        details: Option<BTreeMap<String, Value>>,
        diagnostics: Vec<Diagnostic>,
    },
    Sections(Vec<Section>),
    /// The answer stands, or fails with `words`.
    Verdict {
        words: Option<String>,
        notice: Option<Mark>,
        compact: bool,
    },
}

impl Answer {
    /// A result with no details or diagnostics.
    pub fn result(text: impl Into<String>, error: bool) -> Self {
        Self::Result {
            text: text.into(),
            error,
            details: None,
            diagnostics: Vec::new(),
        }
    }
}

/// One thing for [`Harnesses::handle`] to do.
pub struct Event(Inner);

enum Inner {
    Received(Received),
    Restart(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Down,
    /// Started; its hello is awaited.
    Starting,
    Ready,
    Refused,
}

struct Runtime {
    id: RuntimeInstance,
    process: Option<Process>,
    generation: u64,
    phase: Phase,
    restart_at: Option<Instant>,
    failures: u32,
    ready_since: Option<Instant>,
}

/// The runtimes of one home and their link to this server.
pub struct Harnesses {
    config: Config,
    hand_tools: Vec<ToolSpec>,
    layout: Layout,
    runtimes: Vec<Runtime>,
    sender: UnboundedSender<Received>,
    receiver: UnboundedReceiver<Received>,
    session: Session,
    /// The turns told to wait for a chief's ruling, by runtime.
    held_turns: HashSet<(usize, BindingId)>,
    provider_requests: HashMap<usize, Vec<link::ProviderRequest>>,
    credentials: credentials::Authority,
    /// Set once the harnesses close, so no runtime starts again.
    closing: bool,
}

impl Harnesses {
    /// Starts each runtime that has bound conversations, which
    /// carries on its own interrupted work once it passes its
    /// checks.
    pub fn start(
        layout: &Layout,
        store: &mut Store,
        config: Config,
    ) -> Result<(Self, Vec<Notice>)> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut harnesses = Self {
            config,
            hand_tools: crate::hand::load(layout.root())
                .0
                .into_iter()
                .map(|tool| tool.spec)
                .collect(),
            layout: layout.clone(),
            runtimes: Vec::new(),
            sender,
            receiver,
            session: Session::default(),
            held_turns: HashSet::new(),
            provider_requests: HashMap::new(),
            credentials: credentials::Authority::default(),
            closing: false,
        };
        let mut notices = Vec::new();
        for row in store.runtimes()? {
            let index = harnesses.add(row.id.clone());
            if !store.bindings_of(&row.id)?.is_empty() {
                notices.extend(harnesses.launch(store, index)?);
            }
        }
        Ok((harnesses, notices))
    }

    /// Reloads hand-added definitions for the runtimes' next model
    /// request.
    pub async fn refresh_tools(&mut self) {
        let tools = crate::hand::load(self.layout.root())
            .0
            .into_iter()
            .map(|tool| tool.spec)
            .collect::<Vec<_>>();
        if tools == self.hand_tools {
            return;
        }
        self.hand_tools = tools;
        for index in 0..self.runtimes.len() {
            if self.runtimes[index].phase != Phase::Ready {
                continue;
            }
            let tools = self
                .config
                .tools
                .iter()
                .chain(&self.hand_tools)
                .cloned()
                .collect();
            self.send(index, |v, id| {
                ServerMessage::Tools(link::Tools { v, id, tools })
            })
            .await;
        }
    }

    /// Queues provider work in memory, starting the pi runtime if
    /// necessary.
    pub async fn provider(
        &mut self,
        store: &mut Store,
        request: link::ProviderRequest,
    ) -> Result<()> {
        let runtime = store.runtime_for(DEFAULT_HARNESS)?;
        let index =
            self.index(&runtime).unwrap_or_else(|| self.add(runtime));
        self.credentials
            .grant(index, &request.id, &request.provider);
        self.provider_requests
            .entry(index)
            .or_default()
            .push(request);
        if self.runtimes[index].phase == Phase::Down
            && self.runtimes[index].restart_at.is_none()
        {
            self.launch(store, index)?;
        }
        self.flush(store, index, false).await
    }

    pub(crate) async fn provider_control(
        &mut self,
        control: link::ProviderControl,
    ) {
        let indices = (0..self.runtimes.len())
            .filter(|index| {
                self.credentials.owns(*index, &control.scope)
            })
            .collect::<Vec<_>>();
        for index in indices {
            if control.cancel {
                self.credentials.finish(index, &control.scope);
            }
            self.send(index, |_, _| {
                ServerMessage::ProviderControl(control.clone())
            })
            .await;
        }
    }

    /// Services provider credential requests using the host's
    /// credential owner.
    pub async fn credentials(
        &mut self,
        credentials: &mut crate::credentials::Credentials,
    ) {
        let serving = self
            .config
            .model
            .as_ref()
            .map_or("", |m| m.provider.as_str());
        for (index, mut answer) in
            self.credentials.flush(credentials, serving)
        {
            self.send(index, |v, id| {
                answer.v = v;
                answer.id = id;
                ServerMessage::CredentialResult(answer)
            })
            .await;
        }
    }

    /// The next event to handle.
    pub async fn next(&mut self) -> Event {
        let restart = self
            .runtimes
            .iter()
            .enumerate()
            .filter_map(|(index, runtime)| {
                runtime.restart_at.map(|at| (at, index))
            })
            .min();
        tokio::select! {
            Some(received) = self.receiver.recv() => {
                Event(Inner::Received(received))
            }
            _ = sleep_until(restart.map(|(at, _)| at)), if restart.is_some() => {
                Event(Inner::Restart(restart.map_or(0, |(_, index)| index)))
            }
        }
    }

    /// Handles `event` with the host's credentials, answering the
    /// runtime that sent it.
    pub async fn handle(
        &mut self,
        store: &mut Store,
        event: Event,
        credentials: &Credentials,
    ) -> Result<Vec<Notice>> {
        match event.0 {
            Inner::Restart(index) => {
                self.runtimes[index].restart_at = None;
                self.launch(store, index)
            }
            Inner::Received(received) => {
                let runtime = &self.runtimes[received.runtime];
                if received.generation != runtime.generation {
                    return Ok(Vec::new());
                }
                match received.line {
                    Some(line) => {
                        match self
                            .received(
                                store,
                                received.runtime,
                                &line,
                                credentials,
                            )
                            .await
                        {
                            Ok(notices) => Ok(notices),
                            Err(error) => {
                                let index = received.runtime;
                                self.runtimes[index].phase =
                                    Phase::Refused;
                                let mut notices =
                                    vec![Notice::Refused {
                                        runtime: self.runtimes[index]
                                            .id
                                            .clone(),
                                        words: format!(
                                            "The runtime could not \
                                             be handled: {error:#}"
                                        ),
                                    }];
                                notices
                                    .extend(self.closed(index).await);
                                Ok(notices)
                            }
                        }
                    }
                    None => Ok(self.closed(received.runtime).await),
                }
            }
        }
    }

    /// Binds a new conversation for `agent` to a runtime of its
    /// harness, which makes it.
    pub async fn bind(
        &mut self,
        store: &mut Store,
        agent: &AgentRef,
        errand: bool,
    ) -> Result<(BindingId, Vec<Notice>)> {
        let Some(model) = self.config.model.clone() else {
            bail!("No model is set for new conversations.");
        };
        let harness = store.harness_of(agent)?;
        if harness != DEFAULT_HARNESS {
            bail!("There is no harness named {harness}.");
        }
        let runtime = store.runtime_for(&harness)?;
        let index = match self.index(&runtime) {
            Some(index) => index,
            None => self.add(runtime.clone()),
        };
        if self.runtimes[index].phase == Phase::Refused {
            bail!(
                "The runtime {} was refused; no conversation was \
                 made.",
                runtime.as_str()
            );
        }
        let binding =
            store.add_binding(agent, errand, &model, &runtime)?;
        let mut notices = Vec::new();
        if self.runtimes[index].phase == Phase::Down
            && self.runtimes[index].restart_at.is_none()
        {
            notices.extend(self.launch(store, index)?);
        }
        self.flush(store, index, false).await?;
        Ok((binding, notices))
    }

    /// Keeps an input or a write for `binding` and submits it in its
    /// order. A request id kept before adds nothing new.
    pub async fn submit(
        &mut self,
        store: &mut Store,
        binding: BindingId,
        input: &Input<'_>,
    ) -> Result<()> {
        let bound = self.bound(store, binding)?;
        store.keep_input(binding, input)?;
        if let Some(index) = self.index(&bound.runtime) {
            self.flush(store, index, false).await?;
        }
        Ok(())
    }

    /// Binds a copy of `parent` up to its entry `at`, which its
    /// runtime makes.
    pub async fn fork(
        &mut self,
        store: &mut Store,
        parent: BindingId,
        at: &str,
    ) -> Result<BindingId> {
        let bound = self.bound(store, parent)?;
        if bound.conversation.is_none() {
            bail!("The conversation is not made yet.");
        }
        let fork = store.add_fork(&bound, at)?;
        if let Some(index) = self.index(&bound.runtime) {
            self.flush(store, index, false).await?;
        }
        Ok(fork)
    }

    /// Answers the call `re` came in. A runtime that started again
    /// since asks again, so its answer is dropped.
    pub async fn answer(&mut self, re: Re, answer: Answer) {
        let Some(runtime) = self.runtimes.get(re.index) else {
            return;
        };
        if runtime.generation != re.generation
            || runtime.phase != Phase::Ready
        {
            return;
        }
        if let (Answer::Waiting(_), Wake::Turn(binding)) =
            (&answer, &re.wake)
        {
            self.held_turns.insert((re.index, *binding));
        }
        let key = re.wake.key();
        let re_id = re.id;
        self.send(re.index, |v, id| {
            let re = re_id;
            match answer {
                Answer::Allowed => {
                    ServerMessage::Allowed(link::Allowed {
                        id,
                        re,
                        v,
                    })
                }
                Answer::Waiting(words) => {
                    ServerMessage::Waiting(link::Waiting {
                        id,
                        key,
                        re,
                        v,
                        words,
                    })
                }
                Answer::Refusal(words) => {
                    ServerMessage::Refusal(link::Refusal {
                        id,
                        re,
                        v,
                        words,
                    })
                }
                Answer::Result {
                    text,
                    error,
                    details,
                    diagnostics,
                } => ServerMessage::Result(link::Result {
                    details,
                    diagnostics: (!diagnostics.is_empty())
                        .then_some(diagnostics),
                    error,
                    id,
                    re,
                    text,
                    v,
                }),
                Answer::Sections(sections) => {
                    ServerMessage::Sections(link::Sections {
                        id,
                        re,
                        sections,
                        v,
                    })
                }
                Answer::Verdict {
                    words,
                    notice,
                    compact,
                } => ServerMessage::Verdict(link::Verdict {
                    id,
                    notice,
                    compact: compact.then_some(true),
                    re,
                    v,
                    words,
                }),
            }
        })
        .await;
    }

    /// Sets the tools the conversation of `binding` offers from its
    /// next request, and tells its runtime when they changed (H35).
    pub async fn configure(
        &mut self,
        store: &mut Store,
        binding: BindingId,
        tools: &link::ConfigureTools,
    ) -> Result<()> {
        if !store.set_tools(binding, tools)? {
            return Ok(());
        }
        let bound = self.bound(store, binding)?;
        if let Some(index) = self.index(&bound.runtime) {
            self.flush(store, index, false).await?;
        }
        Ok(())
    }

    /// Tells `runtime` that the wait of its call `key` was answered,
    /// so it asks again at once.
    pub async fn wake(
        &mut self,
        runtime: &RuntimeInstance,
        key: &str,
    ) {
        if let Some(index) = self.index(runtime)
            && self.runtimes[index].phase == Phase::Ready
        {
            let key = key.to_owned();
            self.send(index, |v, id| {
                ServerMessage::Answered(link::Answered { id, key, v })
            })
            .await;
        }
    }

    /// Tells every turn waiting for a chief's ruling to ask again,
    /// once one was given.
    pub async fn wake_turns(&mut self) {
        for (index, binding) in std::mem::take(&mut self.held_turns) {
            if self.runtimes[index].phase == Phase::Ready {
                let key = Wake::Turn(binding).key();
                self.send(index, |v, id| {
                    ServerMessage::Answered(link::Answered {
                        id,
                        key,
                        v,
                    })
                })
                .await;
            }
        }
    }

    /// Stops the running turn of `binding` and drops its queue:
    /// inputs not yet admitted are dropped here first.
    pub async fn cancel(
        &mut self,
        store: &mut Store,
        binding: BindingId,
    ) -> Result<()> {
        let bound = self.bound(store, binding)?;
        store.begin_cancel(binding)?;
        let Some(index) = self.index(&bound.runtime) else {
            return Ok(());
        };
        if self.runtimes[index].phase != Phase::Ready {
            return Ok(());
        }
        // A runtime that is down is told when it is ready again.
        if let Some(conversation) = bound.conversation {
            self.send_cancel(index, binding, conversation).await;
        }
        Ok(())
    }

    /// Asks for a page of the transcript of `binding` before the
    /// entry `before`, answered with [`Notice::Page`].
    pub async fn history(
        &mut self,
        store: &Store,
        binding: BindingId,
        before: Option<String>,
        limit: i64,
    ) -> Result<()> {
        let bound = self.bound(store, binding)?;
        let (Some(index), Some(conversation)) =
            (self.index(&bound.runtime), bound.conversation)
        else {
            bail!("The conversation is not made yet.");
        };
        self.request(index, Pending::History(binding), |v, id| {
            ServerMessage::History(link::History {
                before,
                conversation,
                id,
                limit,
                v,
            })
        })
        .await;
        Ok(())
    }

    /// The process id of `runtime`, while it runs.
    pub fn pid(&self, runtime: &RuntimeInstance) -> Option<u32> {
        let index = self.index(runtime)?;
        self.runtimes[index].process.as_ref()?.pid()
    }

    /// Asks every runtime to stop in order: its running work is
    /// signalled and waited on, then it exits. None starts again.
    pub async fn begin_close(&mut self) {
        self.closing = true;
        for index in 0..self.runtimes.len() {
            self.runtimes[index].restart_at = None;
            if self.runtimes[index].phase == Phase::Ready {
                self.send(index, |v, id| {
                    ServerMessage::Close(link::Close { id, v })
                })
                .await;
            }
        }
    }

    /// Whether every runtime has exited.
    pub fn down(&self) -> bool {
        self.runtimes
            .iter()
            .all(|runtime| runtime.process.is_none())
    }

    /// Closes every link, so each runtime exits, and waits for them;
    /// one still there after its grace is killed.
    pub async fn close(mut self) {
        for runtime in &mut self.runtimes {
            if let Some(process) = runtime.process.take() {
                process.stop(STOP_GRACE).await;
            }
        }
    }

    fn add(&mut self, id: RuntimeInstance) -> usize {
        self.runtimes.push(Runtime {
            id,
            process: None,
            generation: 0,
            phase: Phase::Down,
            restart_at: None,
            failures: 0,
            ready_since: None,
        });
        self.runtimes.len() - 1
    }

    fn index(&self, id: &RuntimeInstance) -> Option<usize> {
        self.runtimes.iter().position(|runtime| runtime.id == *id)
    }

    fn bound(
        &self,
        store: &Store,
        binding: BindingId,
    ) -> Result<Binding> {
        store
            .binding(binding)?
            .with_context(|| format!("no conversation {}", binding.0))
    }

    fn launch(
        &mut self,
        store: &Store,
        index: usize,
    ) -> Result<Vec<Notice>> {
        let row = store.runtime(&self.runtimes[index].id)?;
        let folder = self.layout.harness(&row.id);
        std::fs::create_dir_all(&folder).with_context(|| {
            format!("creating {}", folder.display())
        })?;
        let runtime = &mut self.runtimes[index];
        runtime.generation += 1;
        let process = Process::start(
            &Launch {
                pi: &self.config.pi,
                id: &runtime.id,
                store: &Pi::store(&folder),
                fresh: !row.begun,
            },
            index,
            runtime.generation,
            self.sender.clone(),
        );
        let process = match process {
            Ok(process) => process,
            Err(error) => {
                runtime.phase = Phase::Down;
                let restart_in = schedule(runtime);
                return Ok(vec![Notice::Exited {
                    runtime: runtime.id.clone(),
                    status: format!("{error:#}"),
                    restart_in: Some(restart_in),
                }]);
            }
        };
        let pid = process.pid().unwrap_or_default();
        runtime.process = Some(process);
        runtime.phase = Phase::Starting;
        Ok(vec![Notice::Started {
            runtime: runtime.id.clone(),
            pid,
        }])
    }

    async fn closed(&mut self, index: usize) -> Vec<Notice> {
        self.credentials.gone(index);
        let runtime = &mut self.runtimes[index];
        let status = match runtime.process.take() {
            Some(process) => process.stop(STOP_GRACE).await,
            None => String::from("gone"),
        };
        self.session.forget(index);
        self.held_turns.retain(|(at, _)| *at != index);
        let closing = self.closing;
        let runtime = &mut self.runtimes[index];
        let restart_in = if runtime.phase == Phase::Refused {
            None
        } else if closing {
            runtime.phase = Phase::Down;
            None
        } else {
            if runtime.ready_since.is_some_and(|since| {
                since.elapsed() >= RESTART_FORGIVEN
            }) {
                runtime.failures = 0;
            }
            runtime.phase = Phase::Down;
            Some(schedule(runtime))
        };
        runtime.ready_since = None;
        vec![Notice::Exited {
            runtime: runtime.id.clone(),
            status,
            restart_in,
        }]
    }

    async fn received(
        &mut self,
        store: &mut Store,
        index: usize,
        line: &str,
        credentials: &Credentials,
    ) -> Result<Vec<Notice>> {
        let message = match parse(line) {
            Ok(message) => message,
            Err((words, re)) => {
                self.error(
                    index,
                    re,
                    link::ErrorCode::Malformed,
                    &words,
                )
                .await;
                return Ok(vec![self.fault(index, words)]);
            }
        };
        let phase = self.runtimes[index].phase;
        match message {
            RuntimeMessage::Hello(hello) => {
                if phase != Phase::Starting {
                    self.error(
                        index,
                        Some(hello.id),
                        link::ErrorCode::Malformed,
                        "A runtime says hello once.",
                    )
                    .await;
                    return Ok(Vec::new());
                }
                self.hello(store, index, hello).await
            }
            message if phase != Phase::Ready => {
                let re = id_of(&message);
                self.error(
                    index,
                    Some(re),
                    link::ErrorCode::Malformed,
                    "A runtime says hello first.",
                )
                .await;
                Ok(Vec::new())
            }
            RuntimeMessage::Created(created) => {
                self.created(store, index, created).await
            }
            RuntimeMessage::Admitted(admitted) => {
                self.admitted(store, admitted)
            }
            RuntimeMessage::Settled(settled) => {
                self.settled(store, index, settled)
            }
            RuntimeMessage::Cancelled(cancelled) => {
                self.cancelled(store, index, cancelled).await
            }
            RuntimeMessage::Configured(configured) => {
                self.configured(store, index, configured).await
            }
            RuntimeMessage::Page(page) => {
                self.page(store, index, page).await
            }
            RuntimeMessage::Kept(kept) => {
                self.kept(store, index, kept)
            }
            RuntimeMessage::Live(live) => {
                self.live(store, index, live)
            }
            RuntimeMessage::Statuses(_) => Ok(Vec::new()),
            RuntimeMessage::ProviderResult(result) => {
                self.credentials.finish(index, &result.re);
                Ok(vec![Notice::Provider(result)])
            }
            RuntimeMessage::ProviderEvent(event) => {
                Ok(if self.credentials.owns(index, &event.scope) {
                    vec![Notice::ProviderEvent(event)]
                } else {
                    Vec::new()
                })
            }
            RuntimeMessage::CredentialRequest(request) => {
                self.credentials.request(index, request);
                Ok(Vec::new())
            }
            RuntimeMessage::Allow(allow) => {
                let call = Call::Allow {
                    key: allow.key,
                    input: allow.input,
                    tool: allow.tool,
                    arguments: allow.arguments,
                };
                self.call(
                    store,
                    index,
                    &allow.conversation,
                    allow.id,
                    call,
                )
                .await
            }
            RuntimeMessage::Operation(operation) => {
                let call = Call::Operation {
                    key: operation.key,
                    input: operation.input,
                    tool: operation.tool,
                    arguments: operation.arguments,
                };
                self.call(
                    store,
                    index,
                    &operation.conversation,
                    operation.id,
                    call,
                )
                .await
            }
            RuntimeMessage::Prompt(prompt) => {
                let call = Call::Prompt {
                    input: prompt.input,
                };
                self.call(
                    store,
                    index,
                    &prompt.conversation,
                    prompt.id,
                    call,
                )
                .await
            }
            RuntimeMessage::Yield(answer) => {
                let call = Call::Yield {
                    input: answer.input,
                    answer: answer.answer,
                    history: answer.history.unwrap_or(false),
                    retried: answer.retried.unwrap_or(false),
                    summary_failure: answer.summary_failure,
                };
                self.call(
                    store,
                    index,
                    &answer.conversation,
                    answer.id,
                    call,
                )
                .await
            }
            RuntimeMessage::Attempt(attempt) => {
                self.allow_attempt(store, index, attempt, credentials)
                    .await
            }
            RuntimeMessage::Usage(report) => {
                self.charge_usage(store, index, report).await
            }
            RuntimeMessage::Scrub(mut request) => {
                crate::scrub::scrub_value(
                    &credentials.secrets(),
                    &mut request.value,
                );
                self.send(index, |v, id| {
                    ServerMessage::Scrubbed(link::Scrubbed {
                        v,
                        id,
                        re: request.id,
                        value: request.value,
                    })
                })
                .await;
                Ok(Vec::new())
            }
            RuntimeMessage::Error(error) => {
                // What the request was for is no longer in flight.
                if let Some(re) = &error.re {
                    self.session.reply(re);
                }
                Ok(vec![self.fault(index, error.message)])
            }
        }
    }

    async fn hello(
        &mut self,
        store: &mut Store,
        index: usize,
        hello: link::Hello,
    ) -> Result<Vec<Notice>> {
        let id = self.runtimes[index].id.clone();
        let row = store.runtime(&id)?;
        let refusal = refusal(
            &hello,
            &row,
            &Pi::store(&self.layout.harness(&id)),
        );
        let re = hello.id;
        if let Some(words) = refusal {
            self.runtimes[index].phase = Phase::Refused;
            let said = words.clone();
            self.send(index, |v, id| {
                ServerMessage::Refused(link::Refused {
                    id,
                    re,
                    v,
                    words: said,
                })
            })
            .await;
            return Ok(vec![Notice::Refused { runtime: id, words }]);
        }
        store.runtime_began(&id, &hello.adapter, &hello.harness)?;
        let tools = self
            .config
            .tools
            .iter()
            .chain(&self.hand_tools)
            .cloned()
            .collect();
        let choices = self.choices(store)?;
        let mut windows = BTreeMap::new();
        for binding in store.bindings_of(&id)? {
            if let Some(window) =
                choices.select(store, &binding)?.context_window
            {
                windows.insert(binding.id.0.to_string(), window);
            }
        }
        self.send(index, |v, id| {
            ServerMessage::Welcome(link::Welcome {
                id,
                re,
                tools,
                windows: Some(windows),
                v,
                version: LINK_VERSION,
            })
        })
        .await;
        let runtime = &mut self.runtimes[index];
        runtime.phase = Phase::Ready;
        runtime.ready_since = Some(Instant::now());
        // What the runtime kept or ended while this server did not
        // hear of it is asked for again.
        self.flush(store, index, true).await?;
        for binding in store.bindings_of(&id)? {
            if let Some(conversation) = binding.conversation {
                self.sync(index, binding.id, conversation, None)
                    .await;
            }
        }
        Ok(vec![Notice::Ready { runtime: id }])
    }

    /// The binding of the conversation a runtime names, or an error
    /// sent back to it.
    async fn conversation(
        &mut self,
        store: &Store,
        index: usize,
        conversation: &str,
        re: &str,
    ) -> Result<Option<Binding>> {
        let runtime = &self.runtimes[index].id;
        let bound = store.bound(runtime, conversation)?;
        if bound.is_none() {
            self.error(
                index,
                Some(re.into()),
                link::ErrorCode::UnknownConversation,
                &format!(
                    "No conversation {conversation} is bound here."
                ),
            )
            .await;
        }
        Ok(bound)
    }

    /// A call of the conversation `conversation`, for the host to
    /// answer.
    async fn call(
        &mut self,
        store: &Store,
        index: usize,
        conversation: &str,
        id: String,
        call: Call,
    ) -> Result<Vec<Notice>> {
        let Some(binding) = self
            .conversation(store, index, conversation, &id)
            .await?
        else {
            return Ok(Vec::new());
        };
        let wake = match &call {
            Call::Allow { key, .. } | Call::Operation { key, .. } => {
                Wake::Operation(key.clone())
            }
            Call::Prompt { .. } | Call::Yield { .. } => {
                Wake::Turn(binding.id)
            }
        };
        let re = Re {
            index,
            generation: self.runtimes[index].generation,
            id,
            wake,
        };
        Ok(vec![Notice::Call {
            binding: binding.id,
            re,
            call,
        }])
    }

    fn fault(&self, index: usize, words: String) -> Notice {
        Notice::Fault {
            runtime: self.runtimes[index].id.clone(),
            words,
        }
    }
}

/// Why the runtime that said `hello` is not taken on, in words for a
/// chief.
fn refusal(
    hello: &link::Hello,
    row: &bindings::RuntimeRow,
    store: &Path,
) -> Option<String> {
    let id = row.id.as_str();
    if !hello.versions.contains(&LINK_VERSION) {
        return Some(format!(
            "The runtime {id} speaks link versions {:?}; this \
             server speaks {LINK_VERSION}.",
            hello.versions
        ));
    }
    if hello.store == HelloStore::Missing {
        return Some(format!(
            "The store of the runtime {id} is missing from {}. Its \
             conversations are not started with an empty one.",
            store.display()
        ));
    }
    match &hello.runtime {
        Some(recorded) if recorded == id => {}
        Some(recorded) => {
            return Some(format!(
                "The store at {} belongs to the runtime {recorded}, \
                 not {id}.",
                store.display()
            ));
        }
        None => {
            return Some(format!(
                "The store at {} records no runtime, so it is not \
                 the store of {id}.",
                store.display()
            ));
        }
    }
    if let (Some(adapter), Some(version)) =
        (&row.adapter, &row.version)
        && (adapter != &hello.adapter || version != &hello.harness)
    {
        return Some(format!(
            "The runtime {id} began as {adapter} on {version}, and \
             now reports {} on {}. A runtime's version never \
             changes; its store needs a check that the two are \
             compatible first.",
            hello.adapter, hello.harness
        ));
    }
    let lacking = NEEDED
        .iter()
        .filter(|needed| !hello.capabilities.contains(needed))
        .map(|needed| format!("{needed:?}").to_lowercase())
        .collect::<Vec<_>>();
    if !lacking.is_empty() {
        return Some(format!(
            "The runtime {id} cannot do what Hamlet needs of every \
             harness: {}.",
            lacking.join(", ")
        ));
    }
    None
}

/// Puts `runtime` down to start again after its backoff; returns the
/// wait.
fn schedule(runtime: &mut Runtime) -> Duration {
    let wait = RESTART_FIRST
        .saturating_mul(1 << runtime.failures.min(16))
        .min(RESTART_MOST);
    runtime.failures += 1;
    runtime.restart_at = Some(Instant::now() + wait);
    wait
}

async fn sleep_until(at: Option<Instant>) {
    if let Some(at) = at {
        tokio::time::sleep_until(at.into()).await;
    }
}

/// A notice for people alone, made now.
fn notice(text: &str) -> Mark {
    Mark {
        data: BTreeMap::from([
            ("text".into(), Value::from(text)),
            ("at".into(), Value::from(now())),
        ]),
        kind: NOTICE.into(),
    }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(
        0,
        |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        },
    )
}
