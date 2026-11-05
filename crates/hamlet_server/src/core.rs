//! The loop that owns the realm's writes: it takes in what runtimes
//! say and what clients ask, makes each change in one transaction,
//! and wakes what the change needs: the jobs runner and the runtimes.

use std::collections::BTreeMap;

use anyhow::Result;
use hamlet_protocol::client as protocol;
use hamlet_protocol::runtime::{
    self, AdmittedStatus, RuntimeMessage, ServerMessage,
    SettledStatus, ToolSpec,
};
use hamlet_sandbox::{AllLeftovers, Sandbox};
use hamlet_store::ids::{Actor, AgentId, ConversationId};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};

use crate::calls::{self, Calls, Ran};
use crate::change::{Change, Done, Touched};
use crate::clients::Clients;
use crate::clients::check::reply;
use crate::clients::connection::Event;
use crate::clients::fold::Fold;
use crate::clock;
use crate::credentials::Credentials;
use crate::diagnostic::{Secrets, tell};
use crate::knowledge::runner::Runner;
use crate::providers::{FAUX, Providers};
use crate::realm::Realm;
use crate::society::agents;
use crate::society::refusal::Refusal;
use crate::talk::activity::Activity;
use crate::talk::follow::{Followed, follow};
use crate::talk::inputs::{self, Status};
use crate::talk::leases::Authority;
use crate::talk::process::{Pi, Received};
use crate::talk::runtimes::{self, Pending, Runtimes};
use crate::talk::turns::Admission;
use crate::talk::{
    Env, conversations, items, prompt, sync, turns, verdicts,
};
use crate::work::added::{self, Manifest};
use crate::work::catalogue::SOCIETY;
use crate::work::tools::{self, Shells};

pub(crate) struct Core {
    pub realm: Realm,
    pub env: Env,
    pub runtimes: Runtimes,
    pub credentials: Credentials,
    pub clients: Clients,
    /// What each conversation in memory shows: its transcript and
    /// what its agent is doing.
    pub live: BTreeMap<ConversationId, Live>,
    pub sandbox: Sandbox,
    /// This machine's shells, which the agents' commands run in.
    pub shells: Shells,
    /// What the agents' commands leave running, by agent.
    pub leftovers: AllLeftovers,
    /// The tools added by hand (C17).
    pub added: Vec<Manifest>,
    /// Why the tools added by hand that did not load did not.
    pub faults: Vec<String>,
    /// Every tool a model may be offered.
    pub catalogue: Vec<ToolSpec>,
    pub calls: Calls,
    /// The model catalogue and what waits on the providers.
    pub providers: Providers,
    /// Which runtime may read or rotate which provider's credential.
    pub leases: Authority,
    runner: Runner,
}

/// A conversation's transcript and activity, as kept and as running.
pub(crate) struct Live {
    pub agent: AgentId,
    pub fold: Fold,
    pub activity: Activity,
}

/// What the loop is woken by.
pub(crate) struct Inbox {
    lines: mpsc::UnboundedReceiver<Received>,
    worked: mpsc::UnboundedReceiver<()>,
    events: mpsc::Receiver<Event>,
    ran: mpsc::UnboundedReceiver<Ran>,
    listener: Option<TcpListener>,
}

impl Inbox {
    /// Takes clients' connections from `listener` too.
    pub fn listen(&mut self, listener: TcpListener) {
        self.listener = Some(listener);
    }
}

impl Core {
    /// Takes `realm` on: its jobs runner and its harness's runtime.
    pub fn start(
        mut realm: Realm,
        env: Env,
        pi: Pi,
        sandbox: Sandbox,
        closing: watch::Receiver<bool>,
    ) -> Result<(Self, Inbox)> {
        let credentials = Credentials::open(realm.layout().root())?;
        let (lines_to, lines) = mpsc::unbounded_channel();
        let (worked_to, worked) = mpsc::unbounded_channel();
        let (events_to, events) = mpsc::channel(128);
        let (ran_to, ran) = mpsc::unbounded_channel();
        let runner = Runner::start(realm.layout(), worked_to)?;
        let layout = realm.layout().clone();
        let shells = Shells::local();
        let (added, faults) = added::load(&layout.tools());
        for fault in &faults {
            tell(
                Secrets::Fixed,
                format!("a tool added by hand did not load: {fault}"),
            );
        }
        let mut catalogue = tools::specs(&shells);
        catalogue.extend(calls::specs());
        catalogue.extend(
            added.iter().map(|manifest| manifest.spec.clone()),
        );
        let providers = Providers::open(&layout, env.overrides.faux)?;
        let db = realm.store().connection();
        runtimes::of_harness(db, agents::HARNESS)?;
        let mut runtimes = Runtimes::new(pi, lines_to);
        runtimes.start(db, &layout)?;
        let clients = Clients::new(events_to, closing, clock::now())?;
        let mut core = Self {
            realm,
            env,
            runtimes,
            credentials,
            clients,
            live: BTreeMap::new(),
            sandbox,
            shells,
            leftovers: AllLeftovers::default(),
            added,
            faults,
            catalogue,
            calls: Calls::new(ran_to),
            providers,
            leases: Authority::default(),
            runner,
        };
        core.retool()?;
        core.start_providers()?;
        Ok((
            core,
            Inbox {
                lines,
                worked,
                events,
                ran,
                listener: None,
            },
        ))
    }

    /// Takes in what woke the loop next, or nothing after `wait`.
    /// Returns whether anything came.
    pub async fn step(
        &mut self,
        inbox: &mut Inbox,
        wait: std::time::Duration,
    ) -> Result<bool> {
        let restart = self
            .runtimes
            .next_start()
            .map(tokio::time::Instant::from_std);
        let expiry = self
            .providers
            .next_expiry()
            .map(tokio::time::Instant::from_std);
        let listener = inbox.listener.as_ref();
        tokio::select! {
            Some(received) = inbox.lines.recv() => {
                self.received(received).await?;
                Ok(true)
            }
            Some(()) = inbox.worked.recv() => {
                self.publish().await?;
                Ok(true)
            }
            Some(event) = inbox.events.recv() => {
                self.connection(event).await?;
                Ok(true)
            }
            Some(ran) = inbox.ran.recv() => {
                self.ran(ran).await?;
                Ok(true)
            }
            Ok((stream, _)) = async {
                match listener {
                    Some(listener) => listener.accept().await,
                    None => std::future::pending().await,
                }
            } => {
                self.clients.accept(stream);
                Ok(true)
            }
            () = async {
                match restart {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => {
                let layout = self.realm.layout().clone();
                self.runtimes.start(self.realm.store().connection(), &layout)?;
                Ok(true)
            }
            () = async {
                match expiry {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => {
                self.expire_providers().await?;
                Ok(true)
            }
            () = tokio::time::sleep(wait) => Ok(false),
        }
    }

    /// Makes a change by `actor` in one transaction, then wakes what
    /// it needs.
    pub async fn change<T>(
        &mut self,
        actor: Actor,
        make: impl FnOnce(&mut Change<'_>, &Env) -> Result<T>,
    ) -> Result<T> {
        let at = clock::now();
        let env = self.env.clone();
        let mut tx = self.realm.store().immediate()?;
        let mut cx = Change::new(&mut tx, at, actor);
        let made = make(&mut cx, &env)?;
        let touched = std::mem::take(&mut cx.touched);
        tx.commit()?;
        self.after(touched).await?;
        Ok(made)
    }

    /// Makes a change by `actor` that may be refused, leaving nothing
    /// of it when it is.
    pub async fn attempt<T>(
        &mut self,
        actor: Actor,
        make: impl FnOnce(&mut Change<'_>, &Env) -> Done<T>,
    ) -> Result<Result<T, Refusal>> {
        let at = clock::now();
        let env = self.env.clone();
        let mut tx = self.realm.store().immediate()?;
        let (made, touched) =
            crate::change::run(&mut tx, at, actor, |cx| {
                make(cx, &env)
            })?;
        tx.commit()?;
        self.after(touched).await?;
        Ok(made)
    }

    async fn after(&mut self, touched: Touched) -> Result<()> {
        if touched.jobs {
            self.runner.wake();
        }
        if !touched.agents.is_empty() || !touched.hamlets.is_empty() {
            self.retool()?;
        }
        self.stop_cancelled()?;
        self.flush(false).await?;
        self.publish().await
    }

    /// Narrows the tools of every living agent's conversations to its
    /// role now; a runtime is told before the next input.
    fn retool(&mut self) -> Result<()> {
        let names = self.catalogue_names();
        let db = self.realm.store().connection();
        for agent in agents::living(db)? {
            for kept in conversations::of_agent(db, agent)? {
                let tools = calls::place::tools(
                    db, agent, kept.kind, &names, &SOCIETY,
                )?;
                if kept.tools()?.as_ref() != Some(&tools) {
                    conversations::set_tools(db, kept.id, &tools)?;
                }
            }
        }
        Ok(())
    }

    /// Stops the tool runs of every conversation being stopped.
    fn stop_cancelled(&mut self) -> Result<()> {
        let db = self.realm.store().connection();
        for conversation in self.calls.conversations() {
            if conversations::get(db, conversation)?.cancelling {
                self.calls.stop(conversation);
            }
        }
        Ok(())
    }

    /// The prompt of `conversation`'s next call in the turn of
    /// `submission`, with no secret in it.
    pub(crate) fn prompt(
        &mut self,
        conversation: &conversations::Conversation,
        submission: Option<&str>,
    ) -> Result<Vec<runtime::Section>> {
        let layout = self.realm.layout().clone();
        let confinement = self.confinement();
        let cx = prompt::Context {
            layout: &layout,
            settings: &self.env.settings,
            confinement: &confinement,
            faults: &self.faults,
        };
        let db = self.realm.store().connection();
        let mut sections =
            prompt::sections(db, &cx, conversation, submission)?;
        for section in &mut sections {
            section.text = self.credentials.scrub_text(&section.text);
        }
        Ok(sections)
    }

    /// Wakes the calls a follow says waited, once its change is kept.
    async fn woken(&mut self, followed: Followed) {
        for (runtime, key) in followed.wake {
            self.wake_call(&runtime, &key).await;
        }
    }

    /// How the agents' commands are confined, as a client is told.
    pub(crate) fn confinement(&self) -> protocol::Confinement {
        if self.sandbox.is_insecure() {
            protocol::Confinement::Insecure
        } else {
            protocol::Confinement::Sandbox {
                name: self.sandbox.name().into(),
            }
        }
    }

    /// Sends the watchers of `conversation` what changed in its
    /// transcript.
    fn stream(&mut self, conversation: ConversationId) -> Result<()> {
        let watching = self.clients.watching(conversation);
        let Some(live) = self.live.get_mut(&conversation) else {
            return Ok(());
        };
        let operations = live.fold.take();
        if operations.is_empty() {
            return Ok(());
        }
        for watch in watching {
            let number = watch
                .chat
                .from
                .is_none()
                .then_some(watch.chat.past)
                .flatten();
            let turn =
                protocol::ServerMessage::Turn(protocol::Turn {
                    chat: watch.chat.clone(),
                    number,
                    operations: operations.clone(),
                });
            self.clients.send(
                watch.client,
                &self.credentials,
                turn,
            )?;
        }
        Ok(())
    }

    /// Answers the replies owed for input `request` of
    /// `conversation`, which ended.
    fn settle_owed(
        &mut self,
        conversation: ConversationId,
        request: &str,
    ) -> Result<()> {
        let owed = self
            .clients
            .owed
            .iter()
            .filter(|o| {
                o.conversation == conversation && o.request == request
            })
            .cloned()
            .collect::<Vec<_>>();
        self.clients.owed.retain(|o| {
            !(o.conversation == conversation && o.request == request)
        });
        for owed in owed {
            let message =
                reply(&owed.id, Ok::<_, Refusal>(String::new()));
            self.clients.send(
                owed.client,
                &self.credentials,
                message,
            )?;
        }
        Ok(())
    }

    /// Sends every ready runtime what it is owed.
    pub(crate) async fn flush(&mut self, again: bool) -> Result<()> {
        for index in 0..self.runtimes.list.len() {
            sync::flush(
                self.realm.store().connection(),
                &mut self.runtimes,
                index,
                again,
            )
            .await?;
        }
        Ok(())
    }

    async fn received(&mut self, received: Received) -> Result<()> {
        if !self.runtimes.is_current(&received) {
            return Ok(());
        }
        let index = received.runtime;
        let Some(line) = received.line else {
            self.runtimes.failed(index);
            self.leases.gone(index);
            return Ok(());
        };
        match serde_json::from_str::<RuntimeMessage>(&line) {
            Ok(message) => {
                self.heard(index, message).await?;
                self.answer_credentials().await
            }
            Err(error) => {
                tell(
                    Secrets::Of(&self.credentials),
                    format!(
                        "a runtime's line is not a message: {error}"
                    ),
                );
                Ok(())
            }
        }
    }

    /// Answers the credential requests the leases let through.
    async fn answer_credentials(&mut self) -> Result<()> {
        let serving = self
            .env
            .settings
            .provider()
            .unwrap_or_default()
            .to_owned();
        for (index, mut answer) in
            self.leases.flush(&mut self.credentials, &serving)
        {
            self.runtimes
                .send(index, |v, id| {
                    answer.v = v;
                    answer.id = id;
                    ServerMessage::CredentialResult(answer)
                })
                .await;
        }
        Ok(())
    }

    async fn heard(
        &mut self,
        index: usize,
        message: RuntimeMessage,
    ) -> Result<()> {
        let runtime = self.runtimes.list[index].id.clone();
        let at = clock::now();
        match message {
            RuntimeMessage::Hello(hello) => {
                let refusal = self.runtimes.hello(
                    self.realm.store().connection(),
                    index,
                    &hello,
                )?;
                let re = hello.id;
                match refusal {
                    Some(words) => {
                        tell(Secrets::Fixed, &words);
                        self.runtimes
                            .send(index, |v, id| {
                                ServerMessage::Refused(
                                    runtime::Refused {
                                        id,
                                        re,
                                        v,
                                        words,
                                    },
                                )
                            })
                            .await;
                    }
                    None => {
                        let tools = self.catalogue.clone();
                        self.runtimes
                            .send(index, |v, id| {
                                ServerMessage::Welcome(
                                    runtime::Welcome {
                                        id,
                                        re,
                                        tools,
                                        v,
                                        windows: BTreeMap::new(),
                                    },
                                )
                            })
                            .await;
                        sync::flush(
                            self.realm.store().connection(),
                            &mut self.runtimes,
                            index,
                            true,
                        )
                        .await?;
                        self.send_providers().await?;
                    }
                }
            }
            RuntimeMessage::Created(created) => {
                if let Some((_, Pending::Create(conversation))) =
                    self.runtimes.reply(&created.re)
                {
                    let db = self.realm.store().connection();
                    let (adapter, version) =
                        self.runtimes.versions(db, index)?;
                    conversations::bound(
                        db,
                        conversation,
                        &created.conversation,
                        &adapter,
                        &version,
                    )?;
                }
                self.flush(false).await?;
            }
            RuntimeMessage::Admitted(admitted) => {
                if let Some((
                    _,
                    Pending::Submit(conversation, request),
                )) = self.runtimes.reply(&admitted.re)
                {
                    let status = match admitted.status {
                        AdmittedStatus::Queued => Status::Queued,
                        AdmittedStatus::Placed => Status::Placed,
                        AdmittedStatus::Done => Status::Done,
                        AdmittedStatus::Unanswered => {
                            Status::Unanswered
                        }
                    };
                    let detail =
                        admitted.detail.map(|d| d.to_string());
                    let ended = inputs::admitted(
                        self.realm.store().connection(),
                        conversation,
                        &request,
                        &admitted.submission,
                        status,
                        admitted.reason.as_deref(),
                        detail.as_deref(),
                    )?;
                    if ended {
                        let followed = self
                            .change(Actor::Realm, |cx, env| {
                                follow(
                                    cx,
                                    env,
                                    conversation,
                                    &request,
                                )
                            })
                            .await?;
                        self.woken(followed).await;
                        self.settle_owed(conversation, &request)?;
                    }
                }
                self.flush(false).await?;
            }
            RuntimeMessage::Settled(settled) => {
                let db = self.realm.store().connection();
                let conversation = turns::conversation(
                    db,
                    &runtime,
                    &settled.conversation,
                )?;
                let status = match settled.status {
                    SettledStatus::Done => Status::Done,
                    SettledStatus::Unanswered => Status::Unanswered,
                };
                let detail = settled.detail.map(|d| d.to_string());
                if inputs::settled(
                    db,
                    conversation.id,
                    &settled.request,
                    status,
                    settled.reason.as_deref(),
                    detail.as_deref(),
                )? {
                    let request = settled.request;
                    let followed = self
                        .change(Actor::Realm, |cx, env| {
                            follow(cx, env, conversation.id, &request)
                        })
                        .await?;
                    self.woken(followed).await;
                    self.settle_owed(conversation.id, &request)?;
                }
            }
            RuntimeMessage::Kept(kept) => {
                let db = self.realm.store().connection();
                let conversation = turns::conversation(
                    db,
                    &runtime,
                    &kept.conversation,
                )?;
                let new =
                    items::keep(db, conversation.id, &kept.item)?;
                if new {
                    let loaded =
                        self.live.contains_key(&conversation.id);
                    let live = self.live(conversation.id)?;
                    // A fold read now already holds the entry.
                    if loaded {
                        live.fold.entry(&kept.item)?;
                        live.activity.kept(&kept.item);
                    }
                }
                if new
                    && !self
                        .clients
                        .watching(conversation.id)
                        .is_empty()
                {
                    self.stream(conversation.id)?;
                }
            }
            RuntimeMessage::Live(running) => {
                let db = self.realm.store().connection();
                let conversation = turns::conversation(
                    db,
                    &runtime,
                    &running.conversation,
                )?;
                let secrets = self.credentials.secrets();
                let live = self.live(conversation.id)?;
                live.fold.live(
                    running.running,
                    running.pending,
                    running.partial.as_ref(),
                    &secrets,
                );
                live.activity.live(
                    running.running,
                    running.pending,
                    running.partial.as_ref(),
                );
                self.stream(conversation.id)?;
                self.publish().await?;
            }
            RuntimeMessage::Cancelled(cancelled) => {
                if let Some((_, Pending::Cancel(conversation))) =
                    self.runtimes.reply(&cancelled.re)
                {
                    conversations::set_cancelling(
                        self.realm.store().connection(),
                        conversation,
                        false,
                    )?;
                }
                self.flush(false).await?;
            }
            RuntimeMessage::Configured(configured) => {
                if let Some((
                    _,
                    Pending::Configure(conversation, told),
                )) = self.runtimes.reply(&configured.re)
                {
                    conversations::told_tools(
                        self.realm.store().connection(),
                        conversation,
                        &told,
                    )?;
                }
                self.flush(false).await?;
            }
            RuntimeMessage::Prompt(asked) => {
                let db = self.realm.store().connection();
                let conversation = turns::conversation(
                    db,
                    &runtime,
                    &asked.conversation,
                )?;
                let sections = self
                    .prompt(&conversation, asked.input.as_deref())?;
                let re = asked.id;
                self.runtimes
                    .send(index, |v, id| {
                        ServerMessage::Sections(runtime::Sections {
                            id,
                            re,
                            sections,
                            v,
                        })
                    })
                    .await;
            }
            RuntimeMessage::Attempt(asked) => {
                let admission = turns::allow(
                    self.realm.store().connection(),
                    &self.env,
                    &self.providers,
                    &runtime,
                    &asked,
                    at,
                )?;
                let re = asked.id;
                match admission {
                    Admission::Allowed(selection) => {
                        let provider =
                            selection.model.provider.clone();
                        self.leases
                            .grant(index, &asked.key, &provider);
                        let serving = self
                            .env
                            .settings
                            .provider()
                            .unwrap_or_default()
                            .to_owned();
                        let credential = (provider != FAUX)
                            .then(|| {
                                self.credentials
                                    .model(&provider, &serving)
                            })
                            .flatten()
                            .map(crate::credentials::pack)
                            .transpose()?;
                        self.runtimes
                            .send(index, |v, id| {
                                ServerMessage::Allowance(
                                    runtime::Allowance {
                                        credential,
                                        id,
                                        re,
                                        selection,
                                        v,
                                    },
                                )
                            })
                            .await;
                    }
                    Admission::Stopped(words) => {
                        let notice = Some(
                            crate::talk::marks::Mark::Notice {
                                text: words.clone(),
                                at,
                            }
                            .wire(),
                        );
                        self.runtimes
                            .send(index, |v, id| {
                                ServerMessage::Stopped(
                                    runtime::Stopped {
                                        id,
                                        notice,
                                        re,
                                        v,
                                        words,
                                    },
                                )
                            })
                            .await;
                    }
                }
            }
            RuntimeMessage::Usage(report) => {
                self.leases.finish(index, &report.key);
                turns::charge(
                    self.realm.store().connection(),
                    &runtime,
                    &report.key,
                    &report.usage,
                    at,
                )?;
                let re = report.id;
                self.runtimes
                    .send(index, |v, id| {
                        ServerMessage::Charged(runtime::Charged {
                            id,
                            re,
                            v,
                        })
                    })
                    .await;
            }
            RuntimeMessage::Allow(asked) => {
                self.allow_call(index, asked).await?;
            }
            RuntimeMessage::Operation(asked) => {
                self.operation(index, asked).await?;
            }
            RuntimeMessage::Yield(answer) => {
                let db = self.realm.store().connection();
                let conversation = turns::conversation(
                    db,
                    &runtime,
                    &answer.conversation,
                )?;
                let ruled = self
                    .change(Actor::Realm, |cx, _| {
                        verdicts::rule(cx, &conversation, &answer)
                    })
                    .await?;
                let re = answer.id;
                self.runtimes
                    .send(index, |v, id| {
                        ServerMessage::Verdict(runtime::Verdict {
                            compact: ruled.compact.then_some(true),
                            id,
                            notice: ruled.notice,
                            re,
                            v,
                            words: ruled.words,
                        })
                    })
                    .await;
            }
            RuntimeMessage::Scrub(asked) => {
                let mut value = asked.value;
                self.credentials.scrub(&mut value);
                let re = asked.id;
                self.runtimes
                    .send(index, |v, id| {
                        ServerMessage::Scrubbed(runtime::Scrubbed {
                            id,
                            re,
                            v,
                            value,
                        })
                    })
                    .await;
            }
            RuntimeMessage::Error(error) => {
                tell(
                    Secrets::Of(&self.credentials),
                    format!("{runtime}: {}", error.message),
                );
            }
            RuntimeMessage::CredentialRequest(request) => {
                self.leases.request(index, request);
            }
            RuntimeMessage::ProviderResult(result) => {
                self.provider_result(Some(index), result).await?;
            }
            RuntimeMessage::ProviderEvent(event) => {
                self.provider_event(index, event)?;
            }
            RuntimeMessage::Page(_) | RuntimeMessage::Statuses(_) => {
            }
        }
        Ok(())
    }

    /// Stops the runtimes, giving each `grace` to finish.
    pub async fn stop(
        mut self,
        grace: std::time::Duration,
    ) -> Result<()> {
        self.calls.stop_all();
        self.runtimes.stop(grace).await;
        self.leftovers.end(|_| true);
        drop(self.runner);
        Ok(())
    }
}

/// The realm's chief, for a change a chief makes.
pub(crate) fn chief(core: &Core) -> Actor {
    Actor::Chief(core.realm.chief())
}

/// What a test reads to wait on a condition of the realm.
#[cfg(test)]
pub(crate) async fn until(
    core: &mut Core,
    inbox: &mut Inbox,
    what: &str,
    mut done: impl FnMut(&mut Core) -> Result<bool>,
) -> Result<()> {
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_secs(20);
    while !done(core)? {
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        core.step(inbox, std::time::Duration::from_millis(100))
            .await?;
    }
    Ok(())
}

#[cfg(all(test, feature = "harness"))]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::settings::{Overrides, Settings};
    use crate::society::hand;
    use crate::talk::chats;

    /// A realm on `dir` served with the faux `script`, and a chief's
    /// line `text` to the Hand, answered.
    async fn told(
        dir: &Scratch,
        script: &str,
        text: &str,
    ) -> (Core, Inbox, chats::Said) {
        let (mut core, mut inbox, said) =
            say(dir, script, text).await;
        ended(&mut core, &mut inbox, &said).await;
        (core, inbox, said)
    }

    async fn ended(
        core: &mut Core,
        inbox: &mut Inbox,
        said: &chats::Said,
    ) {
        until(core, inbox, "the Hand's answer", |core| {
            let db = core.realm.store().connection();
            let input =
                inputs::find(db, said.conversation, &said.request)?;
            Ok(input.is_some_and(|i| i.is_ended() && i.followed))
        })
        .await
        .unwrap();
    }

    /// A realm on `dir` served with the faux `script`, and a chief's
    /// line `text` to the Hand, kept.
    async fn say(
        dir: &Scratch,
        script: &str,
        text: &str,
    ) -> (Core, Inbox, chats::Said) {
        let faux = dir.join("faux.json");
        std::fs::write(&faux, script).unwrap();
        let realm =
            Realm::open(&dir.join("realm"), "nixon", clock::now())
                .unwrap();
        let chief_id = realm.chief();
        let env = Env {
            settings: Settings::default(),
            overrides: Overrides {
                faux: true,
                ..Overrides::default()
            },
        };
        let sandbox =
            Sandbox::open(hamlet_sandbox::Isolation::Sandboxed)
                .unwrap();
        let (_closing, closed) = watch::channel(false);
        let (mut core, inbox) = Core::start(
            realm,
            env,
            Pi::from_env(Some(faux)).unwrap(),
            sandbox,
            closed,
        )
        .unwrap();
        let hand = hand::id(core.realm.store().connection()).unwrap();
        let said = core
            .change(Actor::Chief(chief_id), |cx, env| {
                chats::say(cx, env, hand, chief_id, "r1", text)
            })
            .await
            .unwrap();
        (core, inbox, said)
    }

    #[tokio::test]
    async fn a_networked_command_waits_for_the_chief_and_runs_once_allowed()
     {
        let dir = Scratch::new("server-core-gate");
        let script = r#"{"responses": [
            {"calls": [{"name": "bash", "arguments": {"command": "echo curl reached"}}]},
            "Done."
        ]}"#;
        let (mut core, mut inbox, said) =
            say(&dir, script, "Fetch it").await;
        until(&mut core, &mut inbox, "the approval", |core| {
            let db = core.realm.store().connection();
            Ok(!crate::oversight::open(db)?.is_empty())
        })
        .await
        .unwrap();
        let db = core.realm.store().connection();
        let wait = crate::oversight::open(db).unwrap().remove(0);
        assert_eq!(
            wait.danger,
            Some(crate::oversight::Danger::Network)
        );
        let answered = core
            .answer_wait(
                &wait.id.to_string(),
                &protocol::Choice::Allow,
                None,
                None,
            )
            .await
            .unwrap();
        assert!(answered.is_ok(), "{answered:?}");
        ended(&mut core, &mut inbox, &said).await;
        let db = core.realm.store().connection();
        let page =
            items::page(db, said.conversation, None, 20).unwrap();
        assert!(
            page.iter().any(|(_, item)| {
                item.tool.as_deref() == Some("bash")
                    && item.text.contains("curl reached")
            }),
            "{page:#?}"
        );
        assert!(crate::oversight::open(db).unwrap().is_empty());
        core.stop(std::time::Duration::from_secs(5)).await.unwrap();
    }

    #[tokio::test]
    async fn the_hand_writes_its_lodge_and_reads_it_back_with_a_command()
     {
        let dir = Scratch::new("server-core-tools");
        let script = r#"{"responses": [
            {"calls": [{"name": "write", "arguments": {"path": "notes/seen.md", "content": "seen it"}}]},
            {"calls": [{"name": "bash", "arguments": {"command": "cat notes/seen.md"}}]},
            "Done."
        ]}"#;
        let (mut core, _inbox, said) =
            told(&dir, script, "Note it").await;
        let lodge = core.realm.layout().hand();
        assert_eq!(
            std::fs::read_to_string(lodge.join("notes/seen.md"))
                .unwrap(),
            "seen it"
        );
        let db = core.realm.store().connection();
        let page =
            items::page(db, said.conversation, None, 20).unwrap();
        assert!(
            page.iter().any(|(_, item)| {
                item.tool.as_deref() == Some("bash")
                    && item.text.contains("seen it")
            }),
            "{page:#?}"
        );
        core.stop(std::time::Duration::from_secs(5)).await.unwrap();
    }

    #[tokio::test]
    async fn the_hand_answers_a_chief_through_pi() {
        let dir = Scratch::new("server-core");
        let (mut core, _inbox, said) =
            told(&dir, r#"{"responses": ["Hello, chief."]}"#, "Hi")
                .await;
        let db = core.realm.store().connection();
        let page =
            items::page(db, said.conversation, None, 20).unwrap();
        assert!(
            page.iter()
                .any(|(_, item)| item.text.contains("Hello, chief.")),
            "{page:#?}"
        );
        let task = crate::talk::tasks::get(db, said.task).unwrap();
        assert!(
            task.done_at.is_some(),
            "the instruction is answered"
        );
        core.stop(std::time::Duration::from_secs(5)).await.unwrap();
    }
}
