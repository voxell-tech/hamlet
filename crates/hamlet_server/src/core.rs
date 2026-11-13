//! The realm's live state as one view of the world: the resources
//! the handlers of the server's inputs reach. Every change of an
//! input is made in its one transaction, and what it needs woken
//! after, the jobs, the runtimes and the clients, is left for
//! [`crate::apply::Apply`] once it is kept.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::Result;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::system::{Commands, Query, Res, ResMut, SystemParam};
use diesel::Connection;
use hamlet_protocol::client as protocol;
use hamlet_protocol::runtime::{self, ServerMessage, ToolSpec};
use hamlet_store::ids::{Actor, AgentId, ConversationId};

use crate::apply::{Changed, InputEnded, Wake};
use crate::calls::looks::Looks;
use crate::calls::{Calls, Runs};
use crate::change::{Change, Done, Touched};
use crate::clients::connection::UploadOf;
use crate::clients::fold::Fold;
use crate::clients::{Clients, Connections};
use crate::clock;
use crate::commands::Deeds;
use crate::credentials::Credentials;
use crate::diagnostic::{Secrets, tell};
use crate::files::FileUpload;
use crate::images::ImageUpload;
use crate::knowledge::runner::{Runner, Writing, run_due};
use crate::later::Later;
use crate::providers::Providers;
use crate::realm::Realm;
use crate::society::refusal::Refusal;
use crate::talk::activity::Activity;
use crate::talk::leases::Authority;
use crate::talk::runtimes::Runtimes;
use crate::talk::sync::Fitting;
use crate::talk::{Env, conversations, prompt, sync};
use crate::work::added::Manifest;
use crate::work::tools::Shells;
use crate::workspace::Transport;
use crate::workspace::serving::RemoteCopy;

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
    Lodged(HashMap<AgentId, RemoteCopy>)
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
    /// The held calls to wake once the input's changes are sent.
    pub wakes: MessageWriter<'w, Wake>,
    /// The runtimes' inputs that ended, followed once kept.
    pub ended: MessageWriter<'w, InputEnded>,
    /// The chief's commands.
    pub deeds: Res<'w, Deeds>,
    /// What the input spawns and despawns, applied after it.
    pub commands: Commands<'w, 's>,
    /// The images being sent, one entity each.
    pub image_uploads: Query<
        'w,
        's,
        (Entity, &'static UploadOf, &'static mut ImageUpload),
    >,
    /// The files being sent, one entity each.
    pub file_uploads: Query<
        'w,
        's,
        (Entity, &'static UploadOf, &'static mut FileUpload),
    >,
    pub calls: ResMut<'w, Calls>,
    /// The tool runs in progress, one entity each.
    pub runs: Runs<'w, 's>,
    /// The images of inputs being fitted to their models.
    pub fitting: ResMut<'w, Fitting>,
    /// The model catalogue and what waits on the providers.
    pub providers: ResMut<'w, Providers>,
    /// Which runtime may read or rotate which provider's credential.
    pub leases: ResMut<'w, Authority>,
    pub workspace: ResMut<'w, Workspace>,
    pub synced: ResMut<'w, Lodged>,
    pub unpushed: ResMut<'w, Unpushed>,
    pub moving: ResMut<'w, Moving>,
    /// Where tasks send what they finish with.
    pub later: Res<'w, Later>,
    /// What looks off the loop found for the calls that wait on
    /// them.
    pub looks: ResMut<'w, Looks>,
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
    /// Makes a change by `actor` in the input's transaction, kept
    /// with the rest of the input at its end. A change that fails
    /// leaves nothing of it.
    pub fn change<T>(
        &mut self,
        actor: Actor,
        make: impl FnOnce(&mut Change<'_>, &Env) -> Result<T>,
    ) -> Result<T> {
        let at = clock::now();
        let env = self.env.clone();
        let db = self.realm.store().connection();
        hamlet_store::begin(db)?;
        let (made, touched) = db.transaction(|db| {
            let mut cx = Change::new(db, at, actor);
            let made = make(&mut cx, &env)?;
            anyhow::Ok((made, std::mem::take(&mut cx.touched)))
        })?;
        self.after(&touched);
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
        let db = self.realm.store().connection();
        hamlet_store::begin(db)?;
        let (made, touched) =
            crate::change::run(db, at, actor, |cx| make(cx, &env))?;
        self.after(&touched);
        Ok(made)
    }

    /// Leaves what a change touched for [`crate::apply::Apply`] to
    /// follow up once the input is kept.
    fn after(&mut self, touched: &Touched) {
        self.changed.committed = true;
        self.changed.jobs |= touched.jobs;
        self.changed.reshaped |=
            !touched.agents.is_empty() || !touched.hamlets.is_empty();
    }

    /// Owes the clients the realm's state, sent once the input is
    /// kept: for what changed outside a change, such as a setting.
    pub(crate) fn publish(&mut self) {
        self.changed.republish = true;
    }

    /// Runs the jobs the kept changes left, and wakes the runner for
    /// what they leave in turn. Their effects come before anything
    /// that reads them; one that fails is run again by the runner.
    pub(crate) fn run_jobs(&mut self) {
        crate::fault::point("jobs");
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        match run_due(db, &layout, &self.writing) {
            Ok(done) => self.owe_pushes(&done),
            Err(error) => {
                tell(
                    Secrets::Fixed,
                    format!("running jobs: {error:#}"),
                );
            }
        }
        self.runner.wake();
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

    /// Input `request` of `conversation` ended: once what said so is
    /// kept, it is followed.
    pub(crate) fn input_ended(
        &mut self,
        conversation: ConversationId,
        request: &str,
    ) {
        self.ended.write(InputEnded {
            conversation,
            request: request.to_owned(),
        });
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
