//! The realm's live state as one view of the world: the resources
//! the handlers of the server's inputs reach. Each change is made in
//! one transaction, and wakes what it needs after: the jobs runner,
//! the runtimes and the clients.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::panic::AssertUnwindSafe;

use anyhow::Result;
use bevy_ecs::system::{Res, ResMut, SystemParam};
use futures::FutureExt;
use hamlet_protocol::client as protocol;
use hamlet_protocol::runtime::{
    self, AdmittedStatus, ErrorCode, ProtocolError, RuntimeMessage,
    ServerMessage, SettledStatus, ToolSpec,
};
use hamlet_store::ids::{Actor, AgentId, ConversationId};

use crate::apply::Changed;
use crate::calls::Calls;
use crate::change::{Change, Done, Touched};
use crate::clients::Clients;
use crate::clients::check::reply;
use crate::clients::fold::Fold;
use crate::clock;
use crate::credentials::Credentials;
use crate::diagnostic::{self, Secrets, tell};
use crate::files::FileUploads;
use crate::images::Uploads;
use crate::knowledge::runner::{Runner, Writing, run_due};
use crate::providers::{FAUX, Providers};
use crate::realm::Realm;
use crate::society::refusal::Refusal;
use crate::talk::activity::Activity;
use crate::talk::follow::{Followed, follow};
use crate::talk::inputs::{self, Status};
use crate::talk::leases::Authority;
use crate::talk::process::Received;
use crate::talk::runtimes::{Pending, Runtimes};
use crate::talk::sync::Fitting;
use crate::talk::turns::Admission;
use crate::talk::{
    Env, conversations, items, prompt, sync, turns, verdicts,
};
use crate::work::added::Manifest;
use crate::work::tools::Shells;
use crate::workspace::Transport;
use crate::workspace::lodges::Synced;

/// A resource that holds one value of a type made elsewhere, reached
/// through it.
macro_rules! holder {
    ($(#[$meta:meta])* $name:ident($inner:ty)) => {
        $(#[$meta])*
        #[derive(bevy_ecs::resource::Resource)]
        pub(crate) struct $name(pub $inner);

        impl std::ops::Deref for $name {
            type Target = $inner;

            fn deref(&self) -> &$inner {
                &self.0
            }
        }

        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut $inner {
                &mut self.0
            }
        }
    };
}

holder! {
    /// What each conversation in memory shows: its transcript and
    /// what its agent is doing.
    Lives(BTreeMap<ConversationId, Live>)
}
holder! {
    /// How the agents' commands are confined on this machine.
    Confined(hamlet_sandbox::Sandbox)
}
holder! {
    /// What the agents' commands leave running, by agent.
    Leftovers(hamlet_sandbox::AllLeftovers)
}
holder! {
    /// The tools added by hand (C17).
    Added(Vec<Manifest>)
}
holder! {
    /// Why the tools added by hand that did not load did not.
    ToolFaults(Vec<String>)
}
holder! {
    /// Every tool a model may be offered.
    Catalogue(Vec<ToolSpec>)
}

impl Catalogue {
    /// The names of every tool in it.
    pub fn names(&self) -> Vec<String> {
        self.iter().map(|spec| spec.name.clone()).collect()
    }
}
holder! {
    /// The remote workspace the agents' tools run in, chosen before
    /// the server started; none for this machine.
    Workspace(Option<Transport>)
}
holder! {
    /// Each lodge as both copies held it when last in step, by agent;
    /// a lodge not here is pushed before its next remote call.
    Lodged(HashMap<AgentId, Synced>)
}
holder! {
    /// The lodges whose remote copy is owed a push, which the runner
    /// makes before the next input.
    Unpushed(BTreeSet<AgentId>)
}
holder! {
    /// How moving the lodges to the chosen workspace goes.
    Moving(Option<protocol::SettingsViewWorkspaceMove>)
}

/// Every resource of the realm's live state, as the handlers reach
/// it.
#[derive(SystemParam)]
pub(crate) struct Core<'w> {
    pub realm: ResMut<'w, Realm>,
    pub env: ResMut<'w, Env>,
    pub runtimes: ResMut<'w, Runtimes>,
    pub credentials: ResMut<'w, Credentials>,
    pub clients: ResMut<'w, Clients>,
    pub live: ResMut<'w, Lives>,
    pub sandbox: Res<'w, Confined>,
    /// This machine's shells, which the agents' commands run in.
    pub shells: Res<'w, Shells>,
    pub leftovers: Res<'w, Leftovers>,
    pub added: Res<'w, Added>,
    pub faults: Res<'w, ToolFaults>,
    pub catalogue: ResMut<'w, Catalogue>,
    /// What the input's changes leave for the runner to follow up.
    pub changed: ResMut<'w, Changed>,
    pub calls: ResMut<'w, Calls>,
    /// The images of inputs being fitted to their models.
    pub fitting: ResMut<'w, Fitting>,
    /// The model catalogue and what waits on the providers.
    pub providers: ResMut<'w, Providers>,
    /// Which runtime may read or rotate which provider's credential.
    pub leases: ResMut<'w, Authority>,
    /// The images being sent.
    pub images: ResMut<'w, Uploads>,
    /// The files being sent.
    pub files: ResMut<'w, FileUploads>,
    pub workspace: ResMut<'w, Workspace>,
    pub synced: ResMut<'w, Lodged>,
    pub unpushed: ResMut<'w, Unpushed>,
    pub moving: ResMut<'w, Moving>,
    pub runner: Res<'w, Runner>,
    /// Held while anything writes the realm's repositories and
    /// lodges.
    pub writing: Res<'w, Writing>,
}

/// A conversation's transcript and activity, as kept and as running.
pub(crate) struct Live {
    pub agent: AgentId,
    pub fold: Fold,
    pub activity: Activity,
}

impl Core<'_> {
    /// Makes a change by `actor` in one transaction, then wakes what
    /// it needs.
    pub fn change<T>(
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
        self.after(touched)?;
        Ok(made)
    }

    /// Makes a change by `actor` that may be refused, leaving nothing
    /// of it when it is.
    pub fn attempt<T>(
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
        self.after(touched)?;
        Ok(made)
    }

    fn after(&mut self, touched: Touched) -> Result<()> {
        if touched.jobs {
            crate::fault::point("jobs");
            // The change's effects come before anything that reads
            // them; one that fails is run again by the runner.
            let layout = self.realm.layout().clone();
            let db = self.realm.store().connection();
            match run_due(db, &layout, &self.writing) {
                Ok(done) => self.owe_pushes(&done),
                Err(error) => tell(
                    Secrets::Fixed,
                    format!("running jobs: {error:#}"),
                ),
            }
            self.runner.wake();
        }
        // What follows in memory, the runtimes and the clients is the
        // runner's to do once the input is handled.
        self.changed.committed = true;
        self.changed.reshaped |=
            !touched.agents.is_empty() || !touched.hamlets.is_empty();
        Ok(())
    }

    /// Narrows the tools of every living agent's conversations to its
    /// role now; a runtime is told before the next input.
    pub(crate) fn retool(&mut self) -> Result<()> {
        crate::apply::retool_all(
            self.realm.store().connection(),
            &self.catalogue,
        )
    }

    /// The prompt of `conversation`'s next call, with no secret in
    /// it.
    pub(crate) fn prompt(
        &mut self,
        conversation: &conversations::Conversation,
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
        let mut sections = prompt::sections(db, &cx, conversation)?;
        for section in &mut sections {
            section.text = self.credentials.scrub_text(&section.text);
        }
        Ok(sections)
    }

    /// Wakes the calls a follow says waited, once its change is kept.
    fn woken(&mut self, followed: Followed) {
        for (runtime, key) in followed.wake {
            self.wake_call(&runtime, &key);
        }
    }

    /// How the agents' commands are confined, as a client is told.
    pub(crate) fn confinement(&self) -> protocol::Confinement {
        if self.workspace.is_some() {
            protocol::Confinement::Remote
        } else if self.sandbox.is_insecure() {
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
        self.send_turn(watching, operations)
    }

    /// Sends `operations` to each of `watching`.
    fn send_turn(
        &mut self,
        watching: Vec<crate::clients::Watch>,
        operations: Vec<protocol::TranscriptOperation>,
    ) -> Result<()> {
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
            self.clients.send(watch.client, turn);
        }
        Ok(())
    }

    /// Sends the watchers of `conversation` its usage totals, after a
    /// model call of it was charged.
    fn stream_usage(
        &mut self,
        conversation: ConversationId,
    ) -> Result<()> {
        let watching = self.clients.watching(conversation);
        if watching.is_empty() {
            return Ok(());
        }
        let usage = crate::spend::usage_view(
            self.realm.store().connection(),
            conversation,
        )?;
        self.send_turn(
            watching,
            vec![protocol::TranscriptOperation::Usage { usage }],
        )
    }

    /// Follows the end of input `request` of `conversation`, wakes
    /// what its follow freed and answers the replies owed for it.
    pub(crate) fn input_ended(
        &mut self,
        conversation: ConversationId,
        request: &str,
    ) -> Result<()> {
        crate::fault::point("followed");
        let followed = self.change(Actor::Realm, |cx, env| {
            follow(cx, env, conversation, request)
        })?;
        self.woken(followed);
        self.settle_owed(conversation, request)
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
            self.clients.send(owed.client, message);
        }
        Ok(())
    }

    /// Sends every ready runtime what it is owed.
    pub(crate) fn flush(&mut self, again: bool) -> Result<()> {
        for index in 0..self.runtimes.list.len() {
            self.flush_one(index, again)?;
        }
        Ok(())
    }

    /// Sends runtime `index` what it is owed.
    fn flush_one(&mut self, index: usize, again: bool) -> Result<()> {
        let images = self.realm.layout().images();
        let mut owed = sync::Owed {
            db: self.realm.store().connection(),
            env: &self.env,
            runtimes: &mut self.runtimes,
            providers: &self.providers,
            fitting: &mut self.fitting,
            images: &images,
        };
        sync::flush(&mut owed, index, again)
    }

    pub(crate) async fn received(
        &mut self,
        received: Received,
    ) -> Result<()> {
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
                let re = awaited(&message);
                // A line whose handling fails leaves its transaction
                // rolled back; the runtime is told and goes on.
                let heard =
                    AssertUnwindSafe(self.heard(index, message))
                        .catch_unwind()
                        .await;
                let failure = match heard {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(format!("{error:#}")),
                    Err(panic) => Some(diagnostic::panicked(&*panic)),
                };
                if let Some(failure) = failure {
                    hamlet_store::unwind(
                        self.realm.store().connection(),
                    )?;
                    tell(
                        Secrets::Of(&self.credentials),
                        format!("a runtime's line failed: {failure}"),
                    );
                    if let Some(re) = re {
                        let message = self.credentials.scrub_text(
                            &format!("The server failed: {failure}"),
                        );
                        self.runtimes.send(index, |v, id| {
                            ServerMessage::Error(ProtocolError {
                                code: ErrorCode::Failed,
                                id,
                                message,
                                re: Some(re),
                                v,
                            })
                        });
                    }
                }
                self.answer_credentials();
                Ok(())
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
    fn answer_credentials(&mut self) {
        let serving = self
            .env
            .settings
            .provider()
            .unwrap_or_default()
            .to_owned();
        for (index, mut answer) in
            self.leases.flush(&mut self.credentials, &serving)
        {
            self.runtimes.send(index, |v, id| {
                answer.v = v;
                answer.id = id;
                ServerMessage::CredentialResult(answer)
            });
        }
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
                        self.runtimes.send(index, |v, id| {
                            ServerMessage::Refused(runtime::Refused {
                                id,
                                re,
                                v,
                                words,
                            })
                        });
                    }
                    None => {
                        let tools = self.catalogue.0.clone();
                        self.runtimes.send(index, |v, id| {
                            ServerMessage::Welcome(runtime::Welcome {
                                id,
                                re,
                                tools,
                                v,
                                windows: BTreeMap::new(),
                            })
                        });
                        self.flush_one(index, true)?;
                        self.send_providers()?;
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
                self.flush(false)?;
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
                        self.input_ended(conversation, &request)?;
                    }
                }
                self.flush(false)?;
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
                    self.input_ended(
                        conversation.id,
                        &settled.request,
                    )?;
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
                self.publish()?;
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
                self.flush(false)?;
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
                self.flush(false)?;
            }
            RuntimeMessage::Prompt(asked) => {
                let db = self.realm.store().connection();
                let conversation = turns::conversation(
                    db,
                    &runtime,
                    &asked.conversation,
                )?;
                let re = asked.id.clone();
                if let Some((key, words)) = self.before_prompt(
                    &conversation,
                    asked.input.as_deref(),
                )? {
                    self.runtimes.send(index, |v, id| {
                        ServerMessage::Waiting(runtime::Waiting {
                            id,
                            key,
                            re,
                            v,
                            words,
                        })
                    });
                    return Ok(());
                }
                let sections = self.prompt(&conversation)?;
                self.runtimes.send(index, |v, id| {
                    ServerMessage::Sections(runtime::Sections {
                        id,
                        re,
                        sections,
                        v,
                    })
                });
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
                        let db = self.realm.store().connection();
                        let conversation = turns::conversation(
                            db,
                            &runtime,
                            &asked.conversation,
                        )?;
                        self.live(conversation.id)?
                            .activity
                            .attempted();
                        self.publish()?;
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
                        self.runtimes.send(index, |v, id| {
                            ServerMessage::Allowance(
                                runtime::Allowance {
                                    credential,
                                    id,
                                    re,
                                    selection,
                                    v,
                                },
                            )
                        });
                    }
                    Admission::Stopped(words) => {
                        let notice = Some(
                            crate::talk::marks::Mark::Notice {
                                text: words.clone(),
                                at,
                            }
                            .wire(),
                        );
                        self.runtimes.send(index, |v, id| {
                            ServerMessage::Stopped(runtime::Stopped {
                                id,
                                notice,
                                re,
                                v,
                                words,
                            })
                        });
                    }
                }
            }
            RuntimeMessage::Usage(report) => {
                self.leases.finish(index, &report.key);
                let charged = turns::charge(
                    self.realm.store().connection(),
                    &runtime,
                    &report.key,
                    &report.usage,
                    at,
                )?;
                if let Some(conversation) = charged {
                    self.stream_usage(conversation)?;
                    self.publish()?;
                }
                let re = report.id;
                self.runtimes.send(index, |v, id| {
                    ServerMessage::Charged(runtime::Charged {
                        id,
                        re,
                        v,
                    })
                });
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
                let ruled = self.change(Actor::Realm, |cx, _| {
                    verdicts::rule(cx, &conversation, &answer)
                })?;
                let re = answer.id;
                self.runtimes.send(index, |v, id| {
                    ServerMessage::Verdict(runtime::Verdict {
                        compact: ruled.compact.then_some(true),
                        id,
                        notice: ruled.notice,
                        re,
                        v,
                        words: ruled.words,
                    })
                });
            }
            RuntimeMessage::Scrub(asked) => {
                let mut value = asked.value;
                self.credentials.scrub(&mut value);
                let re = asked.id;
                self.runtimes.send(index, |v, id| {
                    ServerMessage::Scrubbed(runtime::Scrubbed {
                        id,
                        re,
                        v,
                        value,
                    })
                });
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
                self.provider_result(Some(index), result)?;
            }
            RuntimeMessage::ProviderEvent(event) => {
                self.provider_event(index, event)?;
            }
            RuntimeMessage::Page(_) | RuntimeMessage::Statuses(_) => {
            }
        }
        Ok(())
    }
}

/// The id of `message` when the runtime waits for its answer.
fn awaited(message: &RuntimeMessage) -> Option<String> {
    let id = match message {
        RuntimeMessage::Prompt(asked) => &asked.id,
        RuntimeMessage::Attempt(asked) => &asked.id,
        RuntimeMessage::Usage(report) => &report.id,
        RuntimeMessage::Allow(asked) => &asked.id,
        RuntimeMessage::Operation(asked) => &asked.id,
        RuntimeMessage::Yield(answer) => &answer.id,
        RuntimeMessage::Scrub(asked) => &asked.id,
        RuntimeMessage::CredentialRequest(request) => &request.id,
        _ => return None,
    };
    Some(id.clone())
}
