//! The realm's live state as one view of the world: the resources
//! the handlers of the server's inputs reach. Every change of an
//! input is made in its one transaction, and what it needs woken
//! after, the jobs, the runtimes and the clients, is left for
//! `PostUpdate` and `Last` once it is kept.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{Message, MessageWriter, Messages};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Query, Res, ResMut, SystemParam};
use diesel::Connection;
use hamlet_protocol::client as protocol;
use hamlet_protocol::runtime::{self, ServerMessage, ToolSpec};
use hamlet_store::ids::{Actor, AgentId, ConversationId};

use crate::background::BackgroundWork;
use crate::calls::looks::LookFindings;
use crate::calls::{Calls, ToolRuns};
use crate::change::{Change, Done, Touched};
use crate::clients::connection::UploadOf;
use crate::clients::fold::Fold;
use crate::clients::{Clients, Connections, ConversationWatcher};
use crate::clock;
use crate::commands::Deeds;
use crate::credentials::Credentials;
use crate::files::FileUpload;
use crate::follow_up::{InputEffects, InputEnded, WakeHeldCall};
use crate::images::sent::ImageUpload;
use crate::knowledge::runner::RepositoryWriteLock;
use crate::providers::Providers;
use crate::realm::Realm;
use crate::talk::activity::Activity;
use crate::talk::inputs::Resend;
use crate::talk::leases::Authority;
use crate::talk::runtimes::Runtimes;
use crate::talk::sync::ImageFitting;
use crate::talk::{Env, conversations, prompt, sync};
use crate::work::added::Manifest;
use crate::work::tools::shell::Shells;
use crate::workspace::Transport;
use crate::workspace::serving::RemoteCopy;

/// The live state no other area keeps, empty at the start; the tools
/// added by hand are loaded into it once the server starts.
pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.init_resource::<Lives>()
        .init_resource::<Leftovers>()
        .init_resource::<Added>()
        .init_resource::<ToolFaults>()
        .init_resource::<Catalogue>()
        .init_resource::<Lodged>()
        .add_message::<UnpushedLodge>()
        .init_resource::<Moving>();
}

/// Each conversation in memory with its transcript and its agent's
/// activity.
#[derive(Resource, Deref, DerefMut, Default)]
pub(crate) struct Lives(pub BTreeMap<ConversationId, Live>);

/// The sandbox confining the agents' commands on this machine.
#[derive(Resource, Deref, DerefMut)]
pub(crate) struct Confined(pub hamlet_sandbox::Sandbox);

/// The leftovers of the agents' commands, by agent.
#[derive(Resource, Deref, DerefMut, Default)]
pub(crate) struct Leftovers(pub hamlet_sandbox::AllLeftovers);

/// The tools added by hand (C17), loaded again whenever their folder
/// changes.
#[derive(Resource, Deref, DerefMut, Default)]
pub(crate) struct Added(pub Vec<Manifest>);

/// The reasons the tools added by hand failed to load.
#[derive(Resource, Deref, DerefMut, Default)]
pub(crate) struct ToolFaults(pub Vec<String>);

/// Every tool a model may be offered.
#[derive(Resource, Deref, DerefMut, Default)]
pub(crate) struct Catalogue(pub Vec<ToolSpec>);

impl Catalogue {
    /// Every tool a model may be offered: the built-in ones this
    /// machine's `shells` run, the server's own, and the `added`
    /// ones.
    pub fn of(shells: &Shells, added: &[Manifest]) -> Vec<ToolSpec> {
        let mut catalogue = crate::work::tools::specs(shells);
        catalogue.extend(crate::calls::specs());
        catalogue.extend(
            added.iter().map(|manifest| manifest.spec.clone()),
        );
        catalogue
    }

    /// The names of every tool in it.
    pub fn names(&self) -> Vec<String> {
        self.iter().map(|spec| spec.name.clone()).collect()
    }
}

/// The remote workspace the agents' tools run in, chosen before the
/// server started; none for this machine.
#[derive(Resource, Deref, DerefMut)]
pub(crate) struct Workspace(pub Option<Transport>);

/// Each lodge as both copies held it when last in step, by agent; a
/// lodge not here is pushed before its next remote call.
#[derive(Resource, Deref, DerefMut, Default)]
pub(crate) struct Lodged(pub HashMap<AgentId, RemoteCopy>);

/// A lodge whose remote copy is owed a push, by its agent, which the
/// runner makes before the next input.
#[derive(Message)]
pub(crate) struct UnpushedLodge(pub AgentId);

/// The progress of moving the lodges to the chosen workspace.
#[derive(Resource, Deref, DerefMut, Default)]
pub(crate) struct Moving(
    pub Option<protocol::SettingsViewWorkspaceMove>,
);

/// The tools a model may be offered, and what runs the built-in ones
/// on this machine.
#[derive(SystemParam)]
pub(crate) struct ModelTools<'w> {
    pub sandbox: Res<'w, Confined>,
    /// This machine's shells, which the agents' commands run in.
    pub shells: Res<'w, Shells>,
    pub leftovers: Res<'w, Leftovers>,
    pub added: ResMut<'w, Added>,
    pub faults: ResMut<'w, ToolFaults>,
    pub catalogue: ResMut<'w, Catalogue>,
}

/// The remote workspace the agents' tools run in, and how its copies
/// of the lodges stand.
#[derive(SystemParam)]
pub(crate) struct RemoteWorkspace<'w> {
    pub workspace: Res<'w, Workspace>,
    pub lodged: ResMut<'w, Lodged>,
    pub unpushed: ResMut<'w, Messages<UnpushedLodge>>,
    pub moving: ResMut<'w, Moving>,
}

/// The images and files being sent, one entity each.
#[derive(SystemParam)]
pub(crate) struct Uploads<'w, 's> {
    pub images: Query<
        'w,
        's,
        (Entity, &'static UploadOf, &'static mut ImageUpload),
    >,
    pub files: Query<
        'w,
        's,
        (Entity, &'static UploadOf, &'static mut FileUpload),
    >,
}

/// Every resource of the realm's live state, as the handlers reach
/// it.
#[derive(SystemParam)]
pub(crate) struct Core<'w, 's> {
    pub realm: ResMut<'w, Realm>,
    /// The clients' connections, one entity each.
    pub connections: Connections<'w, 's>,
    pub env: ResMut<'w, Env>,
    /// The harness's runtimes, one entity each.
    pub runtimes: Runtimes<'w, 's>,
    pub credentials: ResMut<'w, Credentials>,
    pub clients: ResMut<'w, Clients>,
    pub live: ResMut<'w, Lives>,
    pub tools: ModelTools<'w>,
    /// The changes the input left for the runner to follow up.
    pub changed: ResMut<'w, InputEffects>,
    /// The held calls to wake once the input's changes are sent.
    pub wakes: MessageWriter<'w, WakeHeldCall>,
    /// The runtimes' inputs that ended, followed once kept.
    pub ended: MessageWriter<'w, InputEnded>,
    /// The chief's commands.
    pub deeds: Res<'w, Deeds>,
    /// The input's spawns and despawns, applied after it.
    pub commands: Commands<'w, 's>,
    pub uploads: Uploads<'w, 's>,
    pub calls: ResMut<'w, Calls>,
    /// The tool runs in progress, one entity each.
    pub runs: ToolRuns<'w, 's>,
    /// The images of inputs being fitted to their models.
    pub fitting: ImageFitting<'w, 's>,
    /// The model catalogue and what waits on the providers.
    pub providers: ResMut<'w, Providers>,
    /// The runtimes' leases on the providers' credentials.
    pub leases: ResMut<'w, Authority>,
    pub remote: RemoteWorkspace<'w>,
    /// The tasks off the loop and their results.
    pub later: ResMut<'w, BackgroundWork>,
    /// The findings of looks off the loop, for the calls that wait
    /// on them.
    pub looks: LookFindings<'w, 's>,
    /// Held while anything writes the realm's repositories and
    /// lodges.
    pub writing: Res<'w, RepositoryWriteLock>,
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
    ) -> Done<T> {
        let at = clock::now();
        let env = self.env.clone();
        let db = self.realm.store().connection();
        hamlet_store::begin(db)?;
        let (made, touched) =
            crate::change::run(db, at, actor, |cx| make(cx, &env))?;
        self.after(&touched);
        Ok(made)
    }

    /// Leaves what a change touched for `PostUpdate` to follow up
    /// once the input is kept.
    fn after(&mut self, touched: &Touched) {
        self.changed.change_kept = true;
        self.changed.jobs_kept |= touched.jobs;
        self.changed.agents_or_hamlets_changed |=
            !touched.agents.is_empty() || !touched.hamlets.is_empty();
    }

    /// Owes the clients the realm's state, sent once the input is
    /// kept: for what changed outside a change, such as a setting.
    pub(crate) fn publish(&mut self) {
        self.changed.state_changed_outside_change = true;
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
            faults: &self.tools.faults,
        };
        let db = self.realm.store().connection();
        let mut sections = prompt::sections(db, &cx, conversation)?;
        for section in &mut sections {
            section.text = self.credentials.scrub_text(&section.text);
        }
        Ok(sections)
    }

    /// The confinement of the agents' commands, as a client is told.
    pub(crate) fn confinement(&self) -> protocol::Confinement {
        let sandbox = &self.tools.sandbox;
        if self.remote.workspace.is_some() {
            protocol::Confinement::Remote
        } else if sandbox.is_insecure() {
            protocol::Confinement::Insecure
        } else {
            protocol::Confinement::Sandbox {
                name: sandbox.name().into(),
            }
        }
    }

    /// Sends the watchers of `conversation` what changed in its
    /// transcript.
    pub(crate) fn stream(
        &mut self,
        conversation: ConversationId,
    ) -> Result<()> {
        let watching = self.connections.watching(conversation);
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
        watching: Vec<ConversationWatcher>,
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
            self.connections.send(watch.client, turn);
        }
        Ok(())
    }

    /// Sends the watchers of `conversation` its usage totals, after a
    /// model call of it was charged.
    pub(crate) fn stream_usage(
        &mut self,
        conversation: ConversationId,
    ) -> Result<()> {
        let watching = self.connections.watching(conversation);
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

    /// Sends `runtime` what it is owed.
    pub(crate) fn flush_one(
        &mut self,
        runtime: Entity,
        resend: Resend,
    ) -> Result<()> {
        let images = self.realm.layout().images();
        let mut owed = sync::RuntimeDues {
            db: self.realm.store().connection(),
            env: &self.env,
            runtimes: &mut self.runtimes,
            providers: &self.providers,
            fitting: &mut self.fitting,
            images: &images,
        };
        sync::flush(&mut owed, runtime, resend)
    }

    /// Answers the credential requests the leases let through.
    pub(crate) fn answer_credentials(&mut self) {
        let serving = self
            .env
            .settings
            .provider()
            .unwrap_or_default()
            .to_owned();
        for (runtime, mut answer) in
            self.leases.flush(&mut self.credentials, &serving)
        {
            self.runtimes.send(runtime, |v, id| {
                answer.v = v;
                answer.id = id;
                ServerMessage::CredentialResult(answer)
            });
        }
    }
}
