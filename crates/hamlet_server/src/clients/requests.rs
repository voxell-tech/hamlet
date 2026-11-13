//! What each client message asks, answered by the loop: the hello,
//! a chief's lines and commands, watching conversations, and a chat's
//! deeds.

use std::collections::BTreeMap;

use anyhow::Result;
use bevy_ecs::change_detection::Mut;
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::observer::On;
use hamlet_protocol::client::{
    self as protocol, ChatRef, ChatRefKind, ClientMessage, RequestId,
    ServerMessage, Transcript,
};
use hamlet_store::ids::{Actor, AgentId, ConversationId};

use super::check::{fault, reply};
use super::connection::Connection;
use super::fold::Fold;
use super::{Owed, changes};
use crate::change::Done;
use crate::core::{Core, Live};
use crate::society::agents::{self, AgentKind, Effort};
use crate::society::refusal::Refusal;
use crate::society::{authority, hires, workers};
use crate::talk::activity::Activity;
use crate::talk::conversations::{self, Kind};
use crate::talk::{chats, items};

/// The most transcript entries one page holds.
const PAGE: usize = 100;

/// A connection went: the sign-ins its client began end with it. Its
/// uploads go with its entity.
pub(crate) fn left(
    gone: On<Remove<Connection>>,
    mut core: Core,
) -> bevy_ecs::error::Result {
    let Ok(client) = core.connections.get(gone.entity).map(|c| c.id)
    else {
        return Ok(());
    };
    core.providers.disconnect(client);
    core.send_providers()?;
    Ok(())
}

impl Core<'_, '_> {
    /// `client` sent no hello in time.
    pub(crate) fn late(&mut self, client: u64) {
        if self
            .connection(client)
            .is_some_and(|found| !found.welcomed)
        {
            self.clients.refuse(
                client,
                "No hello came in time.",
                1008,
            );
        }
    }

    /// The connection of `client`, while it is there.
    pub(crate) fn connection(
        &mut self,
        client: u64,
    ) -> Option<Mut<'_, Connection>> {
        self.connections.iter_mut().find(|found| found.id == client)
    }

    /// The request `text` of `client`, checked; none when it was
    /// answered here: a hello, or a message that is not one.
    pub(crate) fn checked(
        &mut self,
        client: u64,
        text: &str,
    ) -> Result<Option<ClientMessage>> {
        let Some(found) = self.connection(client) else {
            return Ok(None);
        };
        if found.refused {
            return Ok(None);
        }
        let welcomed = found.welcomed;
        let message = match self.clients.check.parse(text) {
            Ok(message) => message,
            Err(failed) => {
                let message = ServerMessage::Fault(failed);
                self.answer(client, message)?;
                if !welcomed {
                    self.clients.refuse(
                        client,
                        "The first message is a hello.",
                        1008,
                    );
                }
                return Ok(None);
            }
        };
        match message {
            ClientMessage::Hello(hello) if !welcomed => {
                if !self.credentials.is_connection_token(&hello.token)
                {
                    self.clients.refuse(
                        client,
                        "That token is not this server's.",
                        4001,
                    );
                    return Ok(None);
                }
                let ours = protocol::VERSION;
                if !ours.is_compatible(&hello.v) {
                    let words = format!(
                        "This `hello` message does not fit the \
                         protocol: `v` {} is not compatible with \
                         {ours}.",
                        hello.v
                    );
                    self.clients.refuse(client, &words, 4002);
                    return Ok(None);
                }
                if let Some(mut found) = self.connection(client) {
                    found.welcomed = true;
                }
                let state = self.compose()?;
                self.clients.sent = Some(state.clone());
                self.answer(client, ServerMessage::State(state))?;
                Ok(None)
            }
            _ if !welcomed => {
                self.clients.refuse(
                    client,
                    "The first message is a hello.",
                    1008,
                );
                Ok(None)
            }
            message => Ok(Some(message)),
        }
    }

    /// Sends `message` to `client`.
    pub(crate) fn answer(
        &mut self,
        client: u64,
        message: ServerMessage,
    ) -> Result<()> {
        self.clients.send(client, message);
        Ok(())
    }

    /// A chief's line to an agent, with the images and files it
    /// carries, kept for the agent; its reply is owed once the agent
    /// is done with it.
    pub(crate) fn say(
        &mut self,
        client: u64,
        say: protocol::Say,
    ) -> Result<()> {
        let chief = self.realm.chief();
        let Some(agent) = self.reached(client, &say.id, say.to.0)?
        else {
            return Ok(());
        };
        // A line no model can answer is refused, so the agent is not
        // left working on it for good.
        if !self.has_model(agent)? {
            return self.answer(
                client,
                reply(&say.id, Err(Refusal::NoModelSetUp)),
            );
        }
        let attached = match self.attach(
            agent,
            &say.text,
            &say.images,
            &say.files,
        )? {
            Ok(attached) => attached,
            Err(refusal) => {
                return self
                    .answer(client, reply(&say.id, Err(refusal)));
            }
        };
        let request = say.id.as_str().to_owned();
        let line = attached.place(&say.text, |words| {
            self.credentials.scrub_text(words)
        });
        let said = self.change(Actor::Chief(chief), |cx, env| {
            chats::say_with(cx, env, agent, chief, &request, &line)
        })?;
        self.clients.owed.push(Owed {
            client,
            id: say.id,
            conversation: said.conversation,
            request: said.request,
        });
        Ok(())
    }

    /// Begins a new session of the chief's chat with `agent`; who
    /// watches it turns to the new one.
    pub(crate) fn new_session(
        &mut self,
        agent: AgentId,
    ) -> Result<String> {
        let chief = self.realm.chief();
        let number = self
            .change(Actor::Chief(chief), |cx, env| {
                chats::new_session(cx, env, agent, chief)
            })?;
        self.rewatch(agent)?;
        Ok(format!("Session {number} began."))
    }

    /// Goes back to session `number` of the chief's chat with
    /// `agent`.
    pub(crate) fn resume_session(
        &mut self,
        agent: AgentId,
        number: i64,
    ) -> Done<String> {
        let chief = self.realm.chief();
        let done = self.attempt(Actor::Chief(chief), |cx, _| {
            chats::resume(cx, agent, chief, number)
        })?;
        self.rewatch(agent)?;
        Ok(done.map(|()| format!("Back in session {number}.")))
    }

    /// Carries on `agent`'s failed or stopped turn.
    pub(crate) fn retry_turn(
        &mut self,
        agent: AgentId,
    ) -> Done<String> {
        let chief = self.realm.chief();
        let done = self.attempt(Actor::Chief(chief), |cx, env| {
            chats::retry(cx, env, agent, chief)
        })?;
        Ok(done.map(|_| "Carrying on.".to_owned()))
    }

    /// Stops the running turns of `agents`, in one change.
    pub(crate) fn stop_turns(
        &mut self,
        agents: &[AgentId],
    ) -> Result<String> {
        let chief = self.realm.chief();
        let stopped = self.change(Actor::Chief(chief), |cx, _| {
            let mut stopped = 0;
            for agent in agents {
                stopped += chats::stop(cx, *agent)?;
            }
            Ok(stopped)
        })?;
        Ok(format!("Stopped {stopped} running turns."))
    }

    /// Takes the chief's chat with `agent` back to before its line
    /// `item`, giving back what the line said and carried.
    pub(crate) fn rewind_chat(
        &mut self,
        agent: AgentId,
        item: &str,
    ) -> Result<
        Result<
            (
                String,
                Vec<protocol::AttachedImage>,
                Vec<protocol::AttachedFile>,
            ),
            Refusal,
        >,
    > {
        let chief = self.realm.chief();
        let done = self.attempt(Actor::Chief(chief), |cx, env| {
            chats::rewind(cx, env, agent, chief, item)
        })?;
        self.rewatch(agent)?;
        Ok(done)
    }

    /// The agent `to` names, or `None` once request `id` was refused
    /// because this server's chief may not reach it.
    pub(crate) fn reached(
        &mut self,
        client: u64,
        id: &RequestId,
        to: u64,
    ) -> Result<Option<AgentId>> {
        let agent = AgentId(i64::try_from(to)?);
        match self.reaches(agent)? {
            Ok(()) => Ok(Some(agent)),
            Err(refusal) => {
                self.answer(client, reply(id, Err(refusal)))?;
                Ok(None)
            }
        }
    }

    /// Whether `agent` runs a model, as it does once the chief set
    /// one up.
    pub(crate) fn has_model(
        &mut self,
        agent: AgentId,
    ) -> Result<bool> {
        let db = self.realm.store().connection();
        crate::talk::models::has_model(
            db,
            &self.env.settings,
            &self.env.overrides,
            agent,
        )
    }

    /// Refuses unless this server's chief may reach `agent` (C7): the
    /// Hand, a hire of one of its hamlets, or a worker started by it
    /// or in its hamlets.
    pub(crate) fn reaches(&mut self, agent: AgentId) -> Done<()> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        let Some(found) = agents::find(db, agent)? else {
            return Ok(Err(Refusal::NoOne {
                address: agent.to_string(),
            }));
        };
        let allowed = match found.kind {
            AgentKind::Hand => true,
            AgentKind::Hire => {
                let hire = hires::get(db, agent)?;
                authority::is_chief_of(db, chief, &hire.hamlet)?
            }
            AgentKind::Worker => {
                let worker = workers::get(db, agent)?;
                worker.chief == Some(chief)
                    || match &worker.hamlet {
                        Some(hamlet) => {
                            authority::is_chief_of(db, chief, hamlet)?
                        }
                        None => true,
                    }
            }
        };
        if !allowed {
            let name = crate::society::addresses::name_of(db, agent)?;
            return Ok(Err(Refusal::NotYours { name }));
        }
        Ok(Ok(()))
    }

    /// Every living agent this server's chief reaches.
    pub(crate) fn reachable(&mut self) -> Result<Vec<AgentId>> {
        let living = {
            use diesel::prelude::*;
            use hamlet_store::schema::agent;
            agent::table
                .filter(agent::ended_at.is_null())
                .select(agent::id)
                .load::<AgentId>(self.realm.store().connection())?
        };
        let mut out = Vec::new();
        for agent in living {
            if self.reaches(agent)?.is_ok() {
                out.push(agent);
            }
        }
        Ok(out)
    }

    /// The conversation `chat` names for this server's chief.
    pub(crate) fn watch_target(
        &mut self,
        chat: &ChatRef,
    ) -> Done<ConversationId> {
        let chief = self.realm.chief();
        let agent = AgentId(i64::try_from(chat.with.0)?);
        if let Err(refusal) = self.reaches(agent)? {
            return Ok(Err(refusal));
        }
        let db = self.realm.store().connection();
        match (&chat.from, &chat.kind) {
            (Some(from), kind) => {
                let peer = AgentId(i64::try_from(from.0)?);
                let kind = match kind {
                    Some(ChatRefKind::Questions) => Kind::Questions,
                    _ => Kind::Letters,
                };
                match conversations::with_peer(db, agent, kind, peer)?
                {
                    Some(conversation) => Ok(Ok(conversation)),
                    None => Ok(Err(Refusal::NoOne {
                        address: format!(
                            "{peer}'s {kind} to {agent}"
                        ),
                    })),
                }
            }
            (None, _) => match chat.past {
                Some(number) => {
                    match chats::session(db, agent, chief, number)? {
                        Some(conversation) => Ok(Ok(conversation)),
                        None => {
                            Ok(Err(Refusal::NoSession { number }))
                        }
                    }
                }
                None => {
                    let conversation = self.change(
                        Actor::Chief(chief),
                        |cx, env| {
                            chats::current(cx, env, agent, chief)
                        },
                    )?;
                    Ok(Ok(conversation))
                }
            },
        }
    }

    /// The fold of `conversation`, read from its kept entries when it
    /// is not in memory yet.
    pub(crate) fn live(
        &mut self,
        conversation: ConversationId,
    ) -> Result<&mut Live> {
        if !self.live.contains_key(&conversation) {
            let db = self.realm.store().connection();
            let entries =
                items::page(db, conversation, None, i64::MAX)?
                    .into_iter()
                    .map(|(_, item)| item)
                    .collect::<Vec<_>>();
            let agent = conversations::get(db, conversation)?.agent;
            self.live.insert(
                conversation,
                Live {
                    agent,
                    fold: Fold::of(&entries)?,
                    activity: Activity::of(&entries),
                },
            );
        }
        Ok(self.live.get_mut(&conversation).expect("inserted above"))
    }

    /// A page of `conversation`'s transcript ending before `before`.
    pub(crate) fn transcript(
        &mut self,
        conversation: ConversationId,
        chat: &ChatRef,
        before: Option<&str>,
        to: Option<&RequestId>,
    ) -> Result<ServerMessage> {
        let usage = crate::spend::usage_view(
            self.realm.store().connection(),
            conversation,
        )?;
        let number = conversations::get(
            self.realm.store().connection(),
            conversation,
        )?
        .session;
        let live = self.live(conversation)?;
        let fold = &live.fold;
        let end = match before {
            None => fold.items.len(),
            Some(before) => match fold
                .items
                .iter()
                .position(|item| item.id == before)
            {
                Some(end) => end,
                None => {
                    return Ok(fault(
                        to,
                        format!("There is no item `{before}` here."),
                    ));
                }
            },
        };
        let start = end.saturating_sub(PAGE);
        Ok(ServerMessage::Transcript(Transcript {
            chat: chat.clone(),
            earlier: (start > 0)
                .then(|| fold.items[start].id.clone()),
            items: fold.items[start..end].to_vec(),
            last: fold.last.clone(),
            number: chat.from.is_none().then_some(number).flatten(),
            pending: fold.pending,
            to: to.cloned(),
            usage,
            working: fold.working,
        }))
    }

    /// Sends the watchers of `agent`'s chats the session they now
    /// stand for.
    pub(crate) fn rewatch(&mut self, agent: AgentId) -> Result<()> {
        let watches = self
            .clients
            .watches
            .iter()
            .filter(|w| {
                w.chat.with.0
                    == u64::try_from(agent.0).unwrap_or_default()
                    && w.chat.from.is_none()
                    && w.chat.past.is_none()
            })
            .cloned()
            .collect::<Vec<_>>();
        for watch in watches {
            if let Ok(conversation) =
                self.watch_target(&watch.chat)?
            {
                for kept in &mut self.clients.watches {
                    if kept.client == watch.client
                        && kept.chat == watch.chat
                    {
                        kept.conversation = conversation;
                    }
                }
                let transcript = self.transcript(
                    conversation,
                    &watch.chat,
                    None,
                    None,
                )?;
                self.answer(watch.client, transcript)?;
            }
        }
        Ok(())
    }

    /// Sends every welcomed client what changed since the state it
    /// was sent last.
    pub(crate) fn send_state(&mut self) -> Result<()> {
        if !self.connections.iter().any(|c| c.welcomed) {
            self.clients.sent = None;
            return Ok(());
        }
        let state = self.compose()?;
        let message = match self
            .clients
            .sent
            .as_ref()
            .and_then(|old| changes(old, &state))
        {
            Some(updates) if updates.is_empty() => return Ok(()),
            Some(updates) => {
                self.clients.revision += 1;
                ServerMessage::Change(protocol::Changes {
                    changes: updates,
                    epoch: self.clients.epoch,
                    revision: self.clients.revision,
                })
            }
            None => {
                self.clients.epoch = crate::clock::now();
                self.clients.revision = 0;
                let mut whole = state.clone();
                whole.epoch = self.clients.epoch;
                whole.revision = 0;
                ServerMessage::State(whole)
            }
        };
        self.clients.sent = Some(state);
        super::broadcast(
            &mut self.connections,
            &self.credentials,
            &message,
        )
    }

    /// The state as it is now.
    pub(crate) fn compose(&mut self) -> Result<protocol::State> {
        // What each agent does: a working conversation's activity,
        // else one whose last turn failed.
        let mut activity = BTreeMap::<AgentId, Activity>::new();
        for live in self.live.values() {
            let shown = &live.activity;
            if !shown.working && !shown.faulted {
                continue;
            }
            let held = activity.get(&live.agent);
            if held.is_none_or(|held| shown.working && !held.working)
            {
                activity.insert(live.agent, shown.clone());
            }
        }
        let chief = self.realm.chief();
        let layout = self.realm.layout().clone();
        let confinement = self.confinement();
        let settings_view = self.settings_view();
        let providers = self.provider_views()?;
        let db = self.realm.store().connection();
        let chief_name = crate::society::chiefs::find(db, chief)?
            .map(|c| c.name)
            .unwrap_or_default();
        let waits = crate::oversight::views(db)?;
        let mut library = Vec::new();
        for (name, description) in
            crate::knowledge::library::skills(&layout)?
        {
            let by =
                match crate::society::library::shelved_by(db, &name)?
                {
                    Some((Some(agent), _)) => {
                        crate::society::addresses::address_of(
                            db, agent,
                        )?
                    }
                    Some((None, Some(chief))) => {
                        crate::society::addresses::actor_name(
                            db,
                            Some(Actor::Chief(chief)),
                        )?
                    }
                    _ => "the starters".into(),
                };
            library.push(protocol::LibrarySkill {
                by,
                description,
                name,
            });
        }
        let sources = super::state::Sources {
            layout: &layout,
            env: &self.env,
            chief: chief_name,
            confinement,
            activity: &activity,
            providers,
            deeds: self.deeds.listed(),
            settings_view,
            waits,
            library,
        };
        super::state::compose(
            db,
            &sources,
            self.clients.epoch,
            self.clients.revision,
            crate::clock::now(),
        )
    }

    fn settings_view(&self) -> protocol::SettingsView {
        let settings = &self.env.settings;
        protocol::SettingsView {
            allow_net: settings.allow_net,
            character: settings.character.clone(),
            effort: settings.effort.map(Effort::wire),
            endpoints: settings
                .endpoints
                .iter()
                .map(|e| protocol::SettingsViewEndpoints {
                    base_url: e.base_url.clone(),
                    key: self.credentials.is_configured(&e.name),
                    name: e.name.clone(),
                })
                .collect(),
            link: settings.link.clone(),
            model: settings.model.clone(),
            newcomers: settings.newcomers,
            reflect_every: i64::try_from(settings.reflect_every)
                .unwrap_or(0),
            unset: !settings.picked,
            workspace: protocol::SettingsViewWorkspace {
                moving: self.remote.moving.clone(),
                running: self
                    .remote
                    .workspace
                    .0
                    .as_ref()
                    .map(|running| running.url().to_owned()),
                url: self
                    .credentials
                    .workspace_url()
                    .map(String::from),
            },
        }
    }
}
