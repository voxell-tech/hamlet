//! Harness runtimes: started and watched by this server, talking to
//! it over the harness link, with the conversations bound to them.
//!
//! The host owns the [`Store`] and the loop: it waits on
//! [`Harnesses::next`] and passes each event to
//! [`Harnesses::handle`], which answers the runtime and returns what
//! the host may show.

pub mod answers;
mod bindings;
// Generated, so formatted as the generator writes it.
#[rustfmt::skip]
pub mod link;
mod process;
#[cfg(test)]
mod tests;
pub mod tools;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub use answers::Decided;
use anyhow::{Context, Result, bail};
pub use bindings::{
    Binding, BindingId, DEFAULT_HARNESS, Input, Standing,
};
use link::{
    Capability, Diagnostic, HelloStore, Item, LinkError, Mark,
    ModelRef, Partial, RuntimeMessage, Section, ServerMessage,
    ToolSpec,
};
use process::{Launch, Process, Received};
use serde_json::Value;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

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

/// How this server starts runtimes and what it gives them.
#[derive(Clone, Debug)]
pub struct Config {
    pub node: PathBuf,
    /// The pi harness's entry, `harnesses/pi/src/main.ts`.
    pub entry: PathBuf,
    /// A faux script in place of real models.
    pub faux: Option<PathBuf>,
    /// The model new conversations run, if one is set.
    pub model: Option<ModelRef>,
    /// The key `HAMLET_API_KEY` gives, held in memory alone.
    pub api_key: Option<String>,
    pub tools: Vec<ToolSpec>,
}

impl Config {
    /// The configuration the environment gives: `HAMLET_NODE`,
    /// `HAMLET_PI_HARNESS`, `HAMLET_MODEL`, `HAMLET_BASE_URL` and
    /// `HAMLET_API_KEY`, with `faux` in place of a model when given.
    pub fn from_env(faux: Option<PathBuf>) -> Result<Self> {
        let set =
            |name| std::env::var(name).ok().filter(|v| !v.is_empty());
        let model = if faux.is_some() {
            Some(ModelRef {
                provider: "faux".into(),
                id: "faux-1".into(),
                base_url: None,
            })
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
            node: set("HAMLET_NODE")
                .unwrap_or_else(|| "node".into())
                .into(),
            entry: set("HAMLET_PI_HARNESS")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../harnesses/pi/src/main.ts")
                }),
            faux,
            model,
            api_key: set("HAMLET_API_KEY"),
            tools: tools::specs(),
        })
    }
}

/// Something that happened that the host may show or act on.
// Each is moved to the host once, never kept in bulk.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    Provider(link::ProviderResult),
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
    Yield { input: Option<String>, answer: Item },
}

/// The message a [`Call`] came in, to answer it by.
#[derive(Clone, Debug, PartialEq)]
pub struct Re {
    index: usize,
    generation: u64,
    id: String,
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

/// What a request to a runtime was for, until its reply.
enum Pending {
    Create(BindingId),
    Submit(BindingId, String),
    Cancel(BindingId),
    History(BindingId),
    /// A page read to catch up on entries kept while the server did
    /// not hear of them.
    Sync(BindingId),
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
    creating: HashSet<BindingId>,
    cancelling: HashSet<BindingId>,
    submitting: HashSet<(BindingId, String)>,
}

/// The runtimes of one home and their link to this server.
pub struct Harnesses {
    config: Config,
    layout: Layout,
    runtimes: Vec<Runtime>,
    sender: UnboundedSender<Received>,
    receiver: UnboundedReceiver<Received>,
    next: u64,
    pending: HashMap<String, (usize, Pending)>,
    provider_requests: HashMap<usize, Vec<link::ProviderRequest>>,
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
            layout: layout.clone(),
            runtimes: Vec::new(),
            sender,
            receiver,
            next: 1,
            pending: HashMap::new(),
            provider_requests: HashMap::new(),
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

    /// Handles `event`, answering the runtime that sent it.
    pub async fn handle(
        &mut self,
        store: &mut Store,
        event: Event,
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
                        self.received(store, received.runtime, &line)
                            .await
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
        let binding =
            store.add_binding(agent, errand, &model, &runtime)?;
        let index = match self.index(&runtime) {
            Some(index) => index,
            None => self.add(runtime),
        };
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
                Answer::Verdict { words, notice } => {
                    ServerMessage::Verdict(link::Verdict {
                        id,
                        notice,
                        re,
                        v,
                        words,
                    })
                }
            }
        })
        .await;
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

    /// Stops the running turn of `binding` and drops its queue:
    /// inputs not yet admitted are dropped here first, and its
    /// waits lapse.
    pub async fn cancel(
        &mut self,
        store: &mut Store,
        binding: BindingId,
    ) -> Result<()> {
        let bound = self.bound(store, binding)?;
        let lapsed = store.begin_cancel(binding)?;
        let Some(index) = self.index(&bound.runtime) else {
            return Ok(());
        };
        if self.runtimes[index].phase != Phase::Ready {
            return Ok(());
        }
        for key in lapsed {
            self.send(index, |v, id| {
                ServerMessage::Answered(link::Answered { id, key, v })
            })
            .await;
        }
        // A runtime that is down is told when it is ready again.
        self.runtimes[index].cancelling.remove(&binding);
        if let Some(conversation) = bound.conversation {
            self.send_cancel(index, binding, conversation).await;
        }
        Ok(())
    }

    async fn send_cancel(
        &mut self,
        index: usize,
        binding: BindingId,
        conversation: String,
    ) {
        if !self.runtimes[index].cancelling.insert(binding) {
            return;
        }
        let id = self
            .send(index, |v, id| {
                ServerMessage::Cancel(link::Cancel {
                    conversation,
                    id,
                    v,
                })
            })
            .await;
        self.pending.insert(id, (index, Pending::Cancel(binding)));
    }

    /// A chief's answer to the wait of the call `key` of `runtime`.
    pub async fn decide(
        &mut self,
        store: &mut Store,
        runtime: &RuntimeInstance,
        key: &str,
        allow: bool,
    ) -> Result<Decided> {
        let decided = store.decide(runtime, key, allow)?;
        if let Some(index) = self.index(runtime)
            && self.runtimes[index].phase == Phase::Ready
        {
            let key = key.to_owned();
            self.send(index, |v, id| {
                ServerMessage::Answered(link::Answered { id, key, v })
            })
            .await;
        }
        Ok(decided)
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
        let id = self
            .send(index, |v, id| {
                ServerMessage::History(link::History {
                    before,
                    conversation,
                    id,
                    limit,
                    v,
                })
            })
            .await;
        self.pending.insert(id, (index, Pending::History(binding)));
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
            creating: HashSet::new(),
            cancelling: HashSet::new(),
            submitting: HashSet::new(),
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
        let row = store
            .runtimes()?
            .into_iter()
            .find(|row| row.id == self.runtimes[index].id)
            .context("a runtime the store does not keep")?;
        let folder = self.layout.harness(&row.id);
        std::fs::create_dir_all(&folder).with_context(|| {
            format!("creating {}", folder.display())
        })?;
        let store_path = folder.join("pi.sqlite");
        let runtime = &mut self.runtimes[index];
        runtime.generation += 1;
        let process = Process::start(
            &Launch {
                node: &self.config.node,
                entry: &self.config.entry,
                id: &runtime.id,
                store: &store_path,
                fresh: !row.begun,
                faux: self.config.faux.as_deref(),
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
        let runtime = &mut self.runtimes[index];
        let status = match runtime.process.take() {
            Some(process) => process.stop(STOP_GRACE).await,
            None => String::from("gone"),
        };
        runtime.creating.clear();
        runtime.cancelling.clear();
        runtime.submitting.clear();
        self.pending.retain(|_, (at, _)| *at != index);
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
                let Some((_, Pending::Create(binding))) =
                    self.pending.remove(&created.re)
                else {
                    return Ok(Vec::new());
                };
                self.runtimes[index].creating.remove(&binding);
                let row = store
                    .runtimes()?
                    .into_iter()
                    .find(|row| row.id == self.runtimes[index].id);
                let (adapter, version) = row
                    .map(|row| {
                        (
                            row.adapter.unwrap_or_default(),
                            row.version.unwrap_or_default(),
                        )
                    })
                    .unwrap_or_default();
                store.conversation_made(
                    binding,
                    &created.conversation,
                    &adapter,
                    &version,
                )?;
                self.flush(store, index, false).await?;
                Ok(vec![Notice::Created { binding }])
            }
            RuntimeMessage::Admitted(admitted) => {
                let Some((_, Pending::Submit(binding, request))) =
                    self.pending.remove(&admitted.re)
                else {
                    return Ok(Vec::new());
                };
                self.runtimes[index]
                    .submitting
                    .remove(&(binding, request.clone()));
                store.input_admitted(binding, &request)?;
                let status = match admitted.status {
                    link::AdmittedStatus::Queued => "queued",
                    link::AdmittedStatus::Placed => "placed",
                    link::AdmittedStatus::Done => "done",
                    link::AdmittedStatus::Unanswered => "unanswered",
                };
                store.input_stands(
                    binding,
                    &request,
                    Some(&admitted.submission),
                    status,
                    admitted.reason.as_deref(),
                    admitted.detail.as_ref(),
                )?;
                let standing = store
                    .standing(binding, &request)?
                    .context("an admitted input")?;
                Ok(vec![Notice::Admitted {
                    binding,
                    request,
                    standing,
                }])
            }
            RuntimeMessage::Settled(settled) => {
                let runtime = &self.runtimes[index].id;
                let Some(binding) =
                    store.bound(runtime, &settled.conversation)?
                else {
                    return Ok(Vec::new());
                };
                let status = match settled.status {
                    link::SettledStatus::Done => "done",
                    link::SettledStatus::Unanswered => "unanswered",
                };
                store.input_stands(
                    binding.id,
                    &settled.request,
                    None,
                    status,
                    settled.reason.as_deref(),
                    settled.detail.as_ref(),
                )?;
                // An input this server never kept, such as one the
                // runtime made itself, ends unheard of.
                let Some(standing) =
                    store.standing(binding.id, &settled.request)?
                else {
                    return Ok(Vec::new());
                };
                Ok(vec![Notice::Settled {
                    binding: binding.id,
                    request: settled.request,
                    standing,
                }])
            }
            RuntimeMessage::Cancelled(cancelled) => {
                let Some((_, Pending::Cancel(binding))) =
                    self.pending.remove(&cancelled.re)
                else {
                    return Ok(Vec::new());
                };
                self.runtimes[index].cancelling.remove(&binding);
                store.cancelled(binding)?;
                Ok(vec![Notice::Cancelled { binding }])
            }
            RuntimeMessage::Page(page) => {
                match self.pending.remove(&page.re) {
                    Some((_, Pending::History(binding))) => {
                        Ok(vec![Notice::Page {
                            binding,
                            items: page.items,
                            earlier: page.earlier,
                        }])
                    }
                    Some((_, Pending::Sync(binding))) => {
                        self.synced(store, index, binding, page).await
                    }
                    _ => Ok(Vec::new()),
                }
            }
            RuntimeMessage::Statuses(_) => Ok(Vec::new()),
            RuntimeMessage::ProviderResult(result) => {
                Ok(vec![Notice::Provider(result)])
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
                let Some(binding) = self
                    .conversation(
                        store,
                        index,
                        &attempt.conversation,
                        &attempt.id,
                    )
                    .await?
                else {
                    return Ok(Vec::new());
                };
                let task = attempt.task.as_ref().map(|t| {
                    format!("{}:{t}", binding.runtime.as_str())
                });
                let selected = crate::models::choice(
                    store,
                    self.layout.root(),
                    &binding,
                    self.config.faux.is_some(),
                )?;
                let selected_ref = ModelRef {
                    provider: selected["provider"]
                        .as_str()
                        .unwrap_or(&attempt.model.provider)
                        .into(),
                    id: selected["id"]
                        .as_str()
                        .unwrap_or(&attempt.model.id)
                        .into(),
                    base_url: selected["baseUrl"]
                        .as_str()
                        .map(String::from),
                };
                if let Some(stop) = answers::attempt(
                    store,
                    &binding,
                    &attempt.key,
                    &selected_ref,
                    task.as_deref(),
                    selected.get("cost"),
                    now(),
                )? {
                    let re = attempt.id;
                    // A cap of spend ends the turn as stopped, with
                    // its words kept for the chief.
                    let notice =
                        stop.spend.then(|| notice(&stop.words));
                    let words = stop.words;
                    self.send(index, |v, id| {
                        ServerMessage::Stopped(link::Stopped {
                            v,
                            id,
                            re,
                            words,
                            notice,
                        })
                    })
                    .await;
                    return Ok(Vec::new());
                }
                let credential = self
                    .credential(&selected_ref.provider)
                    .map(|key| link::Credential {
                        key,
                        type_: link::CredentialType::ApiKey,
                    });
                let model = selected
                    .as_object()
                    .map(|m| m.clone().into_iter().collect());
                let re = attempt.id;
                self.send(index, |v, id| {
                    ServerMessage::Allowance(link::Allowance {
                        credential,
                        model,
                        id,
                        re,
                        v,
                    })
                })
                .await;
                Ok(vec![Notice::Attempted {
                    binding: binding.id,
                }])
            }
            RuntimeMessage::Usage(report) => {
                let runtime = self.runtimes[index].id.clone();
                let charged = answers::charge(
                    store,
                    &runtime,
                    &report.key,
                    &report.usage,
                    now(),
                )?;
                let re = report.id;
                match charged {
                    Some(()) => {
                        self.send(index, |v, id| {
                            ServerMessage::Charged(link::Charged {
                                id,
                                re,
                                v,
                            })
                        })
                        .await;
                    }
                    None => {
                        self.error(
                            index,
                            Some(re),
                            link::ErrorCode::Failed,
                            "No model call was allowed under this \
                             key.",
                        )
                        .await;
                    }
                }
                Ok(Vec::new())
            }
            RuntimeMessage::Kept(kept) => {
                let runtime = &self.runtimes[index].id;
                let Some(binding) =
                    store.bound(runtime, &kept.conversation)?
                else {
                    return Ok(Vec::new());
                };
                Ok(if store.keep_item(binding.id, &kept.item)? {
                    vec![Notice::Kept {
                        binding: binding.id,
                        item: kept.item,
                    }]
                } else {
                    Vec::new()
                })
            }
            RuntimeMessage::Live(live) => {
                let runtime = &self.runtimes[index].id;
                Ok(
                    match store.bound(runtime, &live.conversation)? {
                        Some(binding) => vec![Notice::Live {
                            binding: binding.id,
                            running: live.running,
                            pending: live.pending,
                            partial: live.partial,
                        }],
                        None => Vec::new(),
                    },
                )
            }
            RuntimeMessage::Error(error) => {
                if let Some(re) = &error.re
                    && let Some((_, pending)) =
                        self.pending.remove(re)
                {
                    let runtime = &mut self.runtimes[index];
                    match pending {
                        Pending::Create(binding) => {
                            runtime.creating.remove(&binding);
                        }
                        Pending::Submit(binding, request) => {
                            runtime
                                .submitting
                                .remove(&(binding, request));
                        }
                        Pending::Cancel(binding) => {
                            runtime.cancelling.remove(&binding);
                        }
                        Pending::History(_) | Pending::Sync(_) => {}
                    }
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
        let row = store
            .runtimes()?
            .into_iter()
            .find(|row| row.id == id)
            .context("a runtime the store does not keep")?;
        let store_path = self.layout.harness(&id).join("pi.sqlite");
        let refusal = refusal(&hello, &row, &store_path);
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
        let tools = self.config.tools.clone();
        self.send(index, |v, id| {
            ServerMessage::Welcome(link::Welcome {
                id,
                re,
                tools,
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

    /// Sends what `index` is owed: conversations to make, then each
    /// conversation's kept inputs in order, and when `resync` also
    /// those admitted whose end was not heard, which answer again.
    async fn flush(
        &mut self,
        store: &mut Store,
        index: usize,
        resync: bool,
    ) -> Result<()> {
        if self.runtimes[index].phase != Phase::Ready {
            return Ok(());
        }
        for request in
            self.provider_requests.remove(&index).unwrap_or_default()
        {
            if let Some(process) =
                self.runtimes[index].process.as_mut()
            {
                process
                    .write(&serde_json::to_string(
                        &ServerMessage::ProviderRequest(request),
                    )?)
                    .await;
            }
        }
        let runtime_id = self.runtimes[index].id.clone();
        for binding in store.bindings_of(&runtime_id)? {
            let Some(conversation) = binding.conversation.clone()
            else {
                if self.runtimes[index].creating.insert(binding.id) {
                    self.make(store, index, &binding).await?;
                }
                continue;
            };
            // Before the inputs kept since, which it must not
            // withdraw.
            if store.cancelling(binding.id)? {
                self.send_cancel(
                    index,
                    binding.id,
                    conversation.clone(),
                )
                .await;
            }
            for owed in store.owed(binding.id, resync)? {
                let request = owed.request.clone();
                if !self.runtimes[index]
                    .submitting
                    .insert((binding.id, request.clone()))
                {
                    continue;
                }
                let conversation = conversation.clone();
                let id = self
                    .send(index, |v, id| {
                        if owed.write {
                            ServerMessage::Write(link::Write {
                                boundary: owed
                                    .boundary
                                    .then_some(true),
                                conversation,
                                id,
                                mark: owed.mark.unwrap_or(Mark {
                                    data: BTreeMap::new(),
                                    kind: "hamlet.notice".into(),
                                }),
                                request: owed.request,
                                v,
                            })
                        } else {
                            ServerMessage::Submit(link::Submit {
                                content: owed.content,
                                conversation,
                                id,
                                mark: owed.mark,
                                request: owed.request,
                                steer: owed.steer,
                                v,
                            })
                        }
                    })
                    .await;
                self.pending.insert(
                    id,
                    (index, Pending::Submit(binding.id, request)),
                );
            }
        }
        Ok(())
    }

    /// Asks the runtime to make the conversation of `binding`: a new
    /// one, or a copy of the one it forks.
    async fn make(
        &mut self,
        store: &Store,
        index: usize,
        binding: &Binding,
    ) -> Result<()> {
        let key = binding.id.0.to_string();
        let fork = match (binding.fork_of, &binding.fork_at) {
            (Some(parent), Some(at)) => Some((
                self.bound(store, parent)?
                    .conversation
                    .context("a fork of a conversation not made")?,
                at.clone(),
            )),
            _ => None,
        };
        let agent = format!(
            "{}:{}",
            binding.agent.hamlet, binding.agent.number
        );
        let errand = binding.errand;
        let model = binding.model.clone();
        let message = move |v, id| match fork {
            Some((conversation, at)) => {
                ServerMessage::Fork(link::Fork {
                    at,
                    conversation,
                    id,
                    key,
                    v,
                })
            }
            None => ServerMessage::Create(link::Create {
                agent,
                errand,
                id,
                key,
                model,
                v,
            }),
        };
        let id = self.send(index, message).await;
        self.pending
            .insert(id, (index, Pending::Create(binding.id)));
        Ok(())
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
        let re = Re {
            index,
            generation: self.runtimes[index].generation,
            id,
        };
        Ok(vec![Notice::Call {
            binding: binding.id,
            re,
            call,
        }])
    }

    /// Asks for the entries of `conversation` before `before`, to
    /// keep those the server lacks.
    async fn sync(
        &mut self,
        index: usize,
        binding: BindingId,
        conversation: String,
        before: Option<String>,
    ) {
        let id = self
            .send(index, |v, id| {
                ServerMessage::History(link::History {
                    before,
                    conversation,
                    id,
                    limit: SYNC_PAGE,
                    v,
                })
            })
            .await;
        self.pending.insert(id, (index, Pending::Sync(binding)));
    }

    /// Keeps the entries of a page read to catch up, asking for the
    /// page before it until one holds an entry already kept.
    async fn synced(
        &mut self,
        store: &mut Store,
        index: usize,
        binding: BindingId,
        page: link::Page,
    ) -> Result<Vec<Notice>> {
        let mut known = false;
        let mut notices = Vec::new();
        for item in page.items.iter() {
            if store.has_item(binding, &item.entry)? {
                known = true;
            } else if store.keep_item(binding, item)? {
                notices.push(Notice::Kept {
                    binding,
                    item: item.clone(),
                });
            }
        }
        if page.earlier
            && !known
            && let (Some(first), Some(conversation)) = (
                page.items.first(),
                self.bound(store, binding)?.conversation,
            )
        {
            let before = first.entry.clone();
            self.sync(index, binding, conversation, Some(before))
                .await;
        }
        Ok(notices)
    }

    async fn error(
        &mut self,
        index: usize,
        re: Option<String>,
        code: link::ErrorCode,
        words: &str,
    ) {
        let message = words.to_owned();
        self.send(index, |v, id| {
            ServerMessage::Error(LinkError {
                code,
                id,
                message,
                re,
                v,
            })
        })
        .await;
    }

    fn fault(&self, index: usize, words: String) -> Notice {
        Notice::Fault {
            runtime: self.runtimes[index].id.clone(),
            words,
        }
    }

    /// Sends the message `make` builds from the link version and a
    /// new id; returns the id.
    async fn send(
        &mut self,
        index: usize,
        make: impl FnOnce(i64, String) -> ServerMessage,
    ) -> String {
        let id = format!("s{}", self.next);
        self.next += 1;
        let message = make(LINK_VERSION, id.clone());
        if let (Some(process), Ok(line)) = (
            self.runtimes[index].process.as_mut(),
            serde_json::to_string(&message),
        ) {
            process.write(&line).await;
        }
        id
    }

    /// The key a model call of `provider` is made with:
    /// `HAMLET_API_KEY` for an endpoint, else the stored
    /// credential.
    fn credential(&self, provider: &str) -> Option<String> {
        if provider == "faux" {
            return None;
        }
        if let Some(key) = &self.config.api_key
            && self
                .config
                .model
                .as_ref()
                .is_some_and(|m| m.provider == provider)
        {
            return Some(key.clone());
        }
        let text = std::fs::read_to_string(self.layout.credentials())
            .ok()?;
        let stored = serde_json::from_str::<Value>(&text).ok()?;
        let credential = &stored["models"][provider];
        (credential["type"] == "api_key")
            .then(|| credential["key"].as_str().map(str::to_owned))
            .flatten()
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

/// A runtime's line as a link message, or the words of why it is none
/// with the id it carried.
fn parse(
    line: &str,
) -> Result<RuntimeMessage, (String, Option<String>)> {
    let value = serde_json::from_str::<Value>(line)
        .map_err(|error| (format!("Not JSON: {error}."), None))?;
    let re = value["id"].as_str().map(str::to_owned);
    if value["v"] != LINK_VERSION {
        return Err((
            format!("Link messages here are version {LINK_VERSION}."),
            re,
        ));
    }
    let message =
        serde_json::from_value::<RuntimeMessage>(value.clone())
            .map_err(|error| {
                (format!("Not a link message: {error}."), re.clone())
            })?;
    // Written back, a field the link does not name is missing.
    if !serde_json::to_value(&message)
        .is_ok_and(|back| same_fields(&back, &value))
    {
        return Err((
            "The message has fields the link does not name.".into(),
            re,
        ));
    }
    Ok(message)
}

/// Whether `a` and `b` have the same fields at every depth; numbers
/// may be written differently.
fn same_fields(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, a)| {
                    b.get(key).is_some_and(|b| same_fields(a, b))
                })
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len()
                && a.iter().zip(b).all(|(a, b)| same_fields(a, b))
        }
        (Value::Number(_), Value::Number(_)) => true,
        (a, b) => a == b,
    }
}

fn id_of(message: &RuntimeMessage) -> String {
    match message {
        RuntimeMessage::ProviderResult(m) => m.id.clone(),
        RuntimeMessage::Hello(m) => m.id.clone(),
        RuntimeMessage::Created(m) => m.id.clone(),
        RuntimeMessage::Admitted(m) => m.id.clone(),
        RuntimeMessage::Cancelled(m) => m.id.clone(),
        RuntimeMessage::Page(m) => m.id.clone(),
        RuntimeMessage::Statuses(m) => m.id.clone(),
        RuntimeMessage::Allow(m) => m.id.clone(),
        RuntimeMessage::Operation(m) => m.id.clone(),
        RuntimeMessage::Attempt(m) => m.id.clone(),
        RuntimeMessage::Usage(m) => m.id.clone(),
        RuntimeMessage::Prompt(m) => m.id.clone(),
        RuntimeMessage::Kept(m) => m.id.clone(),
        RuntimeMessage::Live(m) => m.id.clone(),
        RuntimeMessage::Settled(m) => m.id.clone(),
        RuntimeMessage::Yield(m) => m.id.clone(),
        RuntimeMessage::Error(m) => m.id.clone(),
    }
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
        kind: "hamlet.notice".into(),
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
