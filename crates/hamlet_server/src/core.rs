//! The realm's live state as one view of the world: the resources
//! the handlers of the server's inputs reach. Each change is made in
//! one transaction, and wakes what it needs after: the jobs runner,
//! the runtimes and the clients.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::Result;
use bevy_ecs::system::{Res, ResMut, SystemParam};
use hamlet_protocol::client as protocol;
use hamlet_protocol::runtime::{self, ServerMessage, ToolSpec};
use hamlet_store::ids::{Actor, AgentId, ConversationId};

use crate::apply::Changed;
use crate::calls::Calls;
use crate::change::{Change, Done, Touched};
use crate::clients::check::reply;
use crate::clients::fold::Fold;
use crate::clients::{Clients, Connections};
use crate::clock;
use crate::commands::Commands;
use crate::credentials::Credentials;
use crate::diagnostic::{Secrets, tell};
use crate::files::FileUploads;
use crate::images::Uploads;
use crate::knowledge::runner::{Runner, Writing, run_due};
use crate::providers::Providers;
use crate::realm::Realm;
use crate::society::refusal::Refusal;
use crate::talk::activity::Activity;
use crate::talk::follow::{Followed, follow};
use crate::talk::leases::Authority;
use crate::talk::runtimes::Runtimes;
use crate::talk::sync::Fitting;
use crate::talk::{Env, conversations, prompt, sync};
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
pub(crate) struct Core<'w, 's> {
    pub realm: ResMut<'w, Realm>,
    /// The clients' connections, one entity each.
    pub connections: Connections<'w, 's>,
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
    /// The chief's commands.
    pub commands: Res<'w, Commands>,
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

impl Core<'_, '_> {
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
    pub(crate) fn stream(
        &mut self,
        conversation: ConversationId,
    ) -> Result<()> {
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
    pub(crate) fn stream_usage(
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
    pub(crate) fn flush_one(
        &mut self,
        index: usize,
        again: bool,
    ) -> Result<()> {
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

    /// Answers the credential requests the leases let through.
    pub(crate) fn answer_credentials(&mut self) {
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
}
