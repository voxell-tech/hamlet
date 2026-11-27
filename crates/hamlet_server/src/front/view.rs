//! The realm a client is shown: the realm's own `State` with the
//! door's fields, read again on each change of the realm and the
//! library, and sent to each client as the changes since the last.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Event;
use hamlet_host::{Item, Subscription};
use hamlet_messages::address::{self, Library, Party, Realm};
use hamlet_messages::agent::chat::{
    CheckConnection, Checked, ReadProviders,
};
use hamlet_messages::library::{ReadShelves, Shelved, Skill};
use hamlet_messages::profile::By;
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::client::{ReadState, ReadWorld, World};
use hamlet_messages::realm::world::{
    AgentSeen, Revision, Seen, Spend, Status, WaitSeen,
};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{
    self as protocol, Confinement, Doing, Link, LinkState,
    ServerMessage,
};
use tokio::sync::{mpsc, oneshot, watch};

use super::call::{Calls, server_failed};
use super::host::HAND;
use super::hub::Hub;
use super::{Frames, Keys, words};
use crate::activity::{self, Activity};
use crate::clients::changes;
use crate::credentials::Credentials;
use crate::settings::provider_of;
use crate::society::addresses::Address;

/// What the server knows of itself, shown in every `State`.
pub(crate) struct Own {
    pub(crate) chief: String,
    pub(crate) confinement: Confinement,
    /// The realm's folder.
    pub(crate) root: PathBuf,
    /// When this server started, in milliseconds since the epoch.
    pub(crate) epoch: i64,
    /// The last check of the realm's model.
    pub(crate) link: watch::Sender<Link>,
    /// How far the copy to the chosen remote workspace got.
    pub(crate) moving:
        watch::Sender<Option<protocol::SettingsViewWorkspaceMove>>,
    /// Every agent runs the faux model.
    pub(crate) faux: bool,
}

impl Own {
    /// Shows `checked` as the realm's model link from then on: a
    /// hello as connected, a failure as failed. A refusal leaves the
    /// link as it was.
    pub(crate) fn show_check(&self, checked: &Checked) {
        let (state, models, words) = match checked {
            Checked::Answered { words, models } => {
                (LinkState::Connected, *models, words)
            }
            Checked::Failed { words } => {
                (LinkState::Failed, None, words)
            }
            Checked::Refused { .. } => return,
        };
        self.link.send_replace(Link {
            models: models.map(i64::from),
            state,
            words: Some(words.clone()),
        });
    }
}

/// Checks the realm's model once the agents started, as the realm's
/// endpoints are asked for their models then. A realm served by the
/// faux model, or with no endpoint, is left untested. Once a client
/// has checked, the link shows the client's check instead.
pub(crate) async fn check_at_start(calls: Calls, own: Arc<Own>) {
    if own.faux {
        return;
    }
    let Ok(state) = calls.call(&Realm, &ReadState).await else {
        return;
    };
    if state.settings.endpoints.is_empty() {
        return;
    }
    if let Ok(checked) =
        calls.call(&address::Agent(HAND), &CheckConnection).await
        && own.link.borrow().state == LinkState::Untested
    {
        own.show_check(&checked);
    }
}

/// The realm as the door last read it, with the server's own fields.
pub(crate) struct View {
    pub(crate) own: Arc<Own>,
    pub(crate) world: Seen,
    /// The name of each hire's profile.
    profiles: BTreeMap<AgentNumber, String>,
    /// The waits as a client is shown them.
    waits: Vec<protocol::Wait>,
}

impl View {
    /// The view of `world`, which shows no profile and no wait.
    #[cfg(test)]
    pub(crate) fn of(own: Arc<Own>, world: Seen) -> Self {
        Self {
            own,
            world,
            profiles: BTreeMap::new(),
            waits: Vec::new(),
        }
    }

    /// The agent numbered `number`, ended ones too.
    pub(crate) fn agent(
        &self,
        number: AgentNumber,
    ) -> Option<&AgentSeen> {
        self.world.agents.iter().find(|seen| seen.agent == number)
    }

    /// The address of the agent numbered `number`: `hand`,
    /// `hire#<n>` or `worker#<n>`.
    pub(crate) fn address(&self, number: AgentNumber) -> String {
        address(self.kind(number), number)
    }

    /// How the agent numbered `number` is shown to a chief: its
    /// address, with beside it a worker's nickname, or a hire's
    /// nickname else the name of its profile.
    pub(crate) fn name(&self, number: AgentNumber) -> String {
        let address = self.address(number);
        let nickname =
            self.agent(number).and_then(|seen| seen.nickname.clone());
        let beside = match self.kind(number) {
            protocol::AgentKind::Hand => None,
            protocol::AgentKind::Worker => nickname,
            protocol::AgentKind::Hire => nickname
                .or_else(|| self.profiles.get(&number).cloned()),
        };
        match beside {
            Some(beside) => format!("{address} ({beside})"),
            None => address,
        }
    }

    /// The wait `seen` as a client is shown it; none once it closed.
    pub(crate) fn wait(
        &self,
        seen: &WaitSeen,
    ) -> Option<&protocol::Wait> {
        let id = seen.wait.0.to_string();
        self.waits.iter().find(|shown| shown.id == id)
    }

    /// What `payer` spent in its period, as the world holds it.
    pub(crate) fn spend(&self, payer: &Payer) -> Option<&Spend> {
        self.world.spend.iter().find(|spend| spend.payer == *payer)
    }

    pub(crate) fn is_living(&self, number: AgentNumber) -> bool {
        self.agent(number).is_some_and(|seen| {
            !matches!(seen.status, Status::Ended { .. })
        })
    }

    pub(crate) fn of_kind(
        &self,
        number: AgentNumber,
        kind: protocol::AgentKind,
    ) -> Option<AgentNumber> {
        self.agent(number)
            .filter(|_| self.kind(number) == kind)
            .map(|seen| seen.agent)
    }

    fn kind(&self, number: AgentNumber) -> protocol::AgentKind {
        if number == HAND {
            protocol::AgentKind::Hand
        } else if self
            .agent(number)
            .is_some_and(|seen| seen.worker.is_some())
        {
            protocol::AgentKind::Worker
        } else {
            protocol::AgentKind::Hire
        }
    }
}

/// Reads the realm's world now.
pub(crate) async fn read(
    calls: &Calls,
    own: &Arc<Own>,
) -> Result<View> {
    let (world, state) = tokio::try_join!(
        calls.call(&Realm, &ReadWorld),
        calls.call(&Realm, &ReadState),
    )?;
    let profiles = state
        .agents
        .iter()
        .filter_map(|agent| {
            let profile = state
                .profiles
                .iter()
                .find(|shown| Some(shown.id) == agent.profile)?;
            Some((
                super::route::number(agent.id)?,
                profile.name.clone(),
            ))
        })
        .collect();
    Ok(View {
        own: own.clone(),
        world,
        profiles,
        waits: state.waits,
    })
}

/// The agent numbered `number` as the protocol names it.
pub(crate) fn wire(number: AgentNumber) -> protocol::AgentId {
    protocol::AgentId(u64::from(number.0))
}

/// The budget report of a payer, `Hamlet` or `Realm` by `whose`,
/// from its spend in the world.
pub(crate) fn report(
    spend: Option<&Spend>,
    whose: &str,
) -> protocol::Report {
    let count = |value: u64| i64::try_from(value).unwrap_or(i64::MAX);
    let spent = protocol::Amount {
        micros: spend.map_or(0, |spend| count(spend.spent_micros)),
        ms: spend.map_or(0, |spend| count(spend.spent_ms)),
        tokens: 0,
    };
    let caps = protocol::ReportCaps {
        micros: spend
            .and_then(|spend| spend.budget_micros)
            .map(count),
        ms: spend.and_then(|spend| spend.budget_ms).map(count),
    };
    protocol::Report {
        summary: words::budget(whose, &spent, &caps),
        caps,
        gone: 0.0,
        left: None,
        lines: Vec::new(),
        period: protocol::Period::Day,
        period_words: words::TODAY.into(),
        remains: None,
        spent,
    }
}

/// The door's means to show its clients the realm, served by
/// [`serve`].
#[derive(Clone)]
pub(crate) struct Shows {
    jobs: mpsc::UnboundedSender<Job>,
}

pub(crate) type Jobs = mpsc::UnboundedReceiver<Job>;

pub(crate) enum Job {
    /// Reads the realm again and shows what changed, then tells.
    Read(oneshot::Sender<()>),
    /// Shows a client that said hello the realm as it is now, then
    /// tells.
    Welcome(u64, oneshot::Sender<()>),
    /// Forgets a client whose connection closed.
    Gone(u64),
    /// Shows what the agents do now.
    Activity,
    /// Reads the providers the Hand can sign in to.
    ReadProviders,
    /// The providers read, or none when they could not be.
    Providers(Option<Vec<protocol::ProviderView>>),
}

impl Shows {
    pub(crate) fn new() -> (Self, Jobs) {
        let (jobs, taken) = mpsc::unbounded_channel();
        (Self { jobs }, taken)
    }

    /// Shows every client what changed in the realm, and reads the
    /// providers again after.
    pub(crate) async fn read(&self) {
        let (told, tells) = oneshot::channel();
        let _ = self.jobs.send(Job::Read(told));
        let _ = tells.await;
        self.read_providers();
    }

    /// Shows `client`, which said hello, the realm as it is now; from
    /// then on it is sent each change.
    pub(crate) async fn welcome(&self, client: u64) {
        let (told, tells) = oneshot::channel();
        let _ = self.jobs.send(Job::Welcome(client, told));
        let _ = tells.await;
    }

    pub(crate) fn gone(&self, client: u64) {
        let _ = self.jobs.send(Job::Gone(client));
    }

    pub(crate) fn activity(&self) {
        let _ = self.jobs.send(Job::Activity);
    }

    pub(crate) fn read_providers(&self) {
        let _ = self.jobs.send(Job::ReadProviders);
    }
}

/// Shows the clients the realm until the door closes: reads it again
/// on each change the realm and the library tell, and after an event
/// was missed.
pub(crate) async fn serve(
    mut jobs: Jobs,
    shows: Shows,
    calls: Calls,
    own: Arc<Own>,
    hub: Hub,
    keys: Keys,
    frames: Frames,
) {
    let mut world = subscribe(&hub, &calls, Realm, World::NAME).await;
    let mut shelves =
        subscribe(&hub, &calls, Library, Shelved::NAME).await;
    let mut link = own.link.subscribe();
    let mut moving = own.moving.subscribe();
    let epoch = own.epoch;
    let mut shown = Shown {
        shows,
        calls,
        own,
        hub,
        frames,
        realm: None,
        library: Vec::new(),
        keys,
        providers: None,
        sent: None,
        epoch,
        revision: 0,
        clients: BTreeSet::new(),
        read: Revision::default(),
        providers_read: Reading::Idle,
    };
    shown.shelves().await;
    let _ = shown.reread().await;
    loop {
        tokio::select! {
            job = jobs.recv() => match job {
                Some(job) => shown.job(job).await,
                None => return,
            },
            item = next(&mut world) => {
                if let Err(error) = &item {
                    world =
                        shown.follow_again(Realm, World::NAME, error).await;
                }
                if shown.newer(&item) {
                    let _ = shown.reread().await;
                }
            }
            item = next(&mut shelves) => {
                if let Err(error) = &item {
                    shelves = shown
                        .follow_again(Library, Shelved::NAME, error)
                        .await;
                }
                shown.shelves().await;
                shown.publish();
            }
            Ok(()) = link.changed() => shown.publish(),
            Ok(()) = moving.changed() => shown.publish(),
        }
    }
}

/// Follows the event `event` of `party`; none when it cannot be.
async fn subscribe(
    hub: &Hub,
    calls: &Calls,
    party: impl Party,
    event: &str,
) -> Option<Subscription> {
    let followed = async {
        let at = party.address()?;
        calls.subscribe(&at, event).await
    };
    followed
        .await
        .inspect_err(|error| {
            hub.tell(format!("cannot follow {event}: {error:#}"));
        })
        .ok()
}

/// The next item of `followed`, never while there is none.
async fn next(followed: &mut Option<Subscription>) -> Result<Item> {
    match followed {
        Some(followed) => followed.next().await,
        None => std::future::pending().await,
    }
}

/// The address of the agent of `kind` numbered `number`: `hand`,
/// `hire#<n>` or `worker#<n>`.
fn address(kind: protocol::AgentKind, number: AgentNumber) -> String {
    let id = i64::from(number.0);
    match kind {
        protocol::AgentKind::Hand => Address::Hand,
        protocol::AgentKind::Hire => Address::Hire(id),
        protocol::AgentKind::Worker => Address::Worker(id),
    }
    .to_string()
}

/// Who shelved a skill: an agent by its address among `agents`, a
/// chief by `chief`'s name, and none as the starters.
fn shelver(
    by: Option<&By>,
    agents: &[protocol::Agent],
    chief: &str,
) -> String {
    match by {
        Some(By::Agent { agent }) => {
            let kind = agents
                .iter()
                .find(|shown| shown.id == wire(*agent))
                .map_or(protocol::AgentKind::Hire, |shown| {
                    shown.kind
                });
            address(kind, *agent)
        }
        Some(By::Chief { .. }) => chief.to_owned(),
        None => "the starters".to_owned(),
    }
}

/// The state [`serve`] keeps: the realm as read last, the door's own
/// parts of it, and the `State` the clients were sent with the
/// changes since.
struct Shown {
    shows: Shows,
    calls: Calls,
    own: Arc<Own>,
    hub: Hub,
    frames: Frames,
    realm: Option<protocol::State>,
    library: Vec<Skill>,
    /// The credentials, whose keys say which endpoint is signed in
    /// to.
    keys: Keys,
    /// The providers the Hand listed; none before it answers.
    providers: Option<Vec<protocol::ProviderView>>,
    sent: Option<protocol::State>,
    epoch: i64,
    revision: i64,
    /// The clients sent a `State`, which each change goes to.
    clients: BTreeSet<u64>,
    /// The revision of the last change the realm told, read since.
    read: Revision,
    providers_read: Reading,
}

/// Whether the providers are being read, and again after.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reading {
    Idle,
    Once,
    Again,
}

impl Shown {
    async fn job(&mut self, job: Job) {
        match job {
            Job::Read(told) => {
                let _ = self.reread().await;
                let _ = told.send(());
            }
            Job::Welcome(client, told) => {
                self.welcome(client).await;
                let _ = told.send(());
            }
            Job::Gone(client) => {
                self.clients.remove(&client);
            }
            Job::Activity => self.publish(),
            Job::ReadProviders => match self.providers_read {
                Reading::Idle => self.read_providers(),
                Reading::Once | Reading::Again => {
                    self.providers_read = Reading::Again;
                }
            },
            Job::Providers(read) => {
                if let Some(providers) = read {
                    self.providers = Some(providers);
                    self.publish();
                }
                if std::mem::replace(
                    &mut self.providers_read,
                    Reading::Idle,
                ) == Reading::Again
                {
                    self.read_providers();
                }
            }
        }
    }

    /// Whether `item` of the realm's changes tells one not read yet.
    fn newer(&mut self, item: &Result<Item>) -> bool {
        let Ok(Item::Event(args)) = item else {
            return true;
        };
        match ciborium::from_reader::<(World,), _>(args.as_slice()) {
            Ok((event,)) if event.revision <= self.read => false,
            Ok((event,)) => {
                self.read = event.revision;
                true
            }
            Err(_) => true,
        }
    }

    /// Follows `event` of `party` again, after it ended with `error`.
    async fn follow_again(
        &self,
        party: impl Party,
        event: &str,
        error: &anyhow::Error,
    ) -> Option<Subscription> {
        self.hub.tell(format!("{event} ended: {error:#}"));
        subscribe(&self.hub, &self.calls, party, event).await
    }

    /// Reads the providers from the Hand outside the jobs, which a
    /// Hand not yet up would hold.
    fn read_providers(&mut self) {
        self.providers_read = Reading::Once;
        let (calls, shows) = (self.calls.clone(), self.shows.clone());
        tokio::spawn(async move {
            let read = calls
                .call(&address::Agent(HAND), &ReadProviders)
                .await
                .ok()
                .map(|list| list.providers);
            let _ = shows.jobs.send(Job::Providers(read));
        });
    }

    async fn shelves(&mut self) {
        if let Ok(shelves) =
            self.calls.call(&Library, &ReadShelves).await
        {
            self.library = shelves.skills;
        }
    }

    /// Reads the realm, follows the agents it shows at work, and
    /// shows the clients what changed.
    async fn reread(&mut self) -> Result<()> {
        let state = self.calls.call(&Realm, &ReadState).await?;
        let busy = state
            .agents
            .iter()
            .filter(|agent| agent.busy)
            .filter_map(|agent| super::route::number(agent.id))
            .collect();
        self.hub.busy(&busy);
        self.realm = Some(state);
        self.publish();
        Ok(())
    }

    async fn welcome(&mut self, client: u64) {
        if let Err(error) = self.reread().await {
            let _ = self
                .frames
                .send((client, server_failed(None, &error)));
            return;
        }
        let Some(sent) = self.sent.clone().or_else(|| self.compose())
        else {
            return;
        };
        if self.sent.is_none() {
            self.revision = 0;
            self.sent = Some(sent.clone());
        }
        let whole = protocol::State {
            epoch: self.epoch,
            revision: self.revision,
            ..sent
        };
        let _ =
            self.frames.send((client, ServerMessage::State(whole)));
        self.clients.insert(client);
    }

    /// Sends each client what changed since the `State` it holds.
    fn publish(&mut self) {
        if self.clients.is_empty() {
            self.sent = None;
            return;
        }
        let Some(state) = self.compose() else {
            return;
        };
        let message = match self
            .sent
            .as_ref()
            .and_then(|old| changes(old, &state))
        {
            Some(updates) if updates.is_empty() => return,
            Some(updates) => {
                self.revision += 1;
                ServerMessage::Change(protocol::Changes {
                    changes: updates,
                    epoch: self.epoch,
                    revision: self.revision,
                })
            }
            None => {
                self.epoch = crate::clock::now();
                self.revision = 0;
                ServerMessage::State(protocol::State {
                    epoch: self.epoch,
                    revision: 0,
                    ..state.clone()
                })
            }
        };
        self.sent = Some(state);
        for &client in &self.clients {
            let _ = self.frames.send((client, message.clone()));
        }
    }

    /// The realm as read last with the door's own fields, and what
    /// each agent does now.
    fn compose(&self) -> Option<protocol::State> {
        let own = &self.own;
        let realm = self.realm.clone()?;
        let providers = match &self.providers {
            Some(listed) => listed.clone(),
            None if own.faux => Vec::new(),
            None => endpoints(&realm.settings, &self.keys.read()),
        };
        let mut state = protocol::State {
            chief: own.chief.clone(),
            confinement: own.confinement.clone(),
            deeds: crate::commands::deeds(),
            hand: wire(HAND),
            library: self
                .library
                .iter()
                .map(|skill| protocol::LibrarySkill {
                    by: shelver(
                        skill.shelved_by.as_ref(),
                        &realm.agents,
                        &own.chief,
                    ),
                    description: skill.description.clone(),
                    name: skill.name.clone(),
                })
                .collect(),
            providers,
            realm: hamlet_os::fs::path::text(&own.root),
            ..realm
        };
        state.settings.link = own.link.borrow().clone();
        state.settings.workspace.moving = own.moving.borrow().clone();
        for agent in &mut state.agents {
            match super::route::number(agent.id)
                .and_then(|number| self.hub.activity(number))
            {
                Some(shown) => show(agent, &shown),
                None => {
                    agent.activity.doing_words =
                        activity::words(&agent.activity.doing).into();
                }
            }
        }
        Some(state)
    }
}

/// The providers of a realm not served by the faux model before the
/// Hand lists its own: each endpoint of `settings`, signed in to by a
/// key, configured when `keys` holds one.
fn endpoints(
    settings: &protocol::SettingsView,
    keys: &Credentials,
) -> Vec<protocol::ProviderView> {
    let serving = provider_of(&settings.model).unwrap_or_default();
    let mut providers: Vec<_> = settings
        .endpoints
        .iter()
        .map(|endpoint| protocol::ProviderView {
            configured: keys.model(&endpoint.name, serving).is_some(),
            id: endpoint.name.clone(),
            label: endpoint.name.clone(),
            methods: vec![protocol::LoginMethod::Key],
            name: endpoint.name.clone(),
        })
        .collect();
    providers.sort_by(|a, b| a.id.cmp(&b.id));
    providers.dedup_by(|a, b| a.id == b.id);
    providers
}

/// Shows on `agent` what `shown` says it does and says.
fn show(agent: &mut protocol::Agent, shown: &Activity) {
    let doing = if agent.busy && shown.working {
        shown.doing.clone()
    } else {
        Doing::Rest
    };
    agent.activity = protocol::AgentActivity {
        doing_words: activity::words(&doing).into(),
        doing,
        failures: u64::try_from(shown.failures).unwrap_or(0),
        step: u64::try_from(shown.step).unwrap_or(0),
    };
    agent.saying = protocol::AgentSaying {
        text: shown.saying.clone(),
        thought: shown.thought,
    };
    agent.faulted = shown.faulted;
}

#[cfg(test)]
mod tests;
