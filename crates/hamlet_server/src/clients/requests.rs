//! The loop's answers to client messages.

use std::collections::BTreeMap;

use anyhow::Result;
use bevy_ecs::change_detection::Mut;
use bevy_ecs::lifecycle::Remove;
use bevy_ecs::observer::On;
use hamlet_protocol::client::{
    self as protocol, ChatRef, ClientMessage, ImportProject,
    RequestId, ServerMessage, Transcript,
};
use hamlet_store::ids::{Actor, AgentId, ConversationId};

use super::check::{fault, reply};
use super::connection::Connection;
use super::fold::Fold;
use super::{OwedReply, changes};
use crate::attachments::files::Files;
use crate::conversations::activity::Activity;
use crate::conversations::{
    self, QUEUED, chats, imports, items, models,
};
use crate::core::{Core, Live};
use crate::realm::change::{Done, done};
use crate::society::agents::Effort;
use crate::society::authority;
use crate::society::rejection::Rejection;

/// The answer to a compaction asked of a conversation.
pub(crate) const COMPACTING: &str =
    "Summarizing the earlier part of the conversation.";

/// The most transcript entries one page holds.
const PAGE: usize = 100;

/// Ends the sign-ins a client began once its connection goes.
pub(crate) fn left(
    gone: On<Remove<Connection>>,
    mut core: Core,
) -> bevy_ecs::error::Result {
    let Some(client) = core.connections.client(gone.entity) else {
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
            self.connections.refuse(
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
        self.connections.get_mut(client)
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
        let message = match self.clients.protocol_schema.parse(text) {
            Ok(message) => message,
            Err(failed) => {
                let message = ServerMessage::Fault(failed);
                self.answer(client, message)?;
                if !welcomed {
                    self.connections.refuse(
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
                    self.connections.refuse(
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
                    self.connections.refuse(client, &words, 4002);
                    return Ok(None);
                }
                if let Some(mut found) = self.connection(client) {
                    found.welcomed = true;
                }
                let state = self.compose()?;
                self.clients.last_sent_state = Some(state.clone());
                self.answer(client, ServerMessage::State(state))?;
                Ok(None)
            }
            _ if !welcomed => {
                self.connections.refuse(
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
        self.connections.send(client, message);
        Ok(())
    }

    /// Keeps a chief's line to an agent.
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
        // A line no model can answer is rejected, so the agent is not
        // left working on it for good.
        if !self.has_model(agent)? {
            return self.answer(
                client,
                reply(&say.id, Err(Rejection::NoModelSetUp)),
            );
        }
        let attached = match self.attach(
            agent,
            &say.text,
            &say.images,
            &say.files,
        )? {
            Ok(attached) => attached,
            Err(rejection) => {
                return self
                    .answer(client, reply(&say.id, Err(rejection)));
            }
        };
        let request = say.id.as_str().to_owned();
        let line = attached.place(&say.text, |words| {
            self.credentials.scrub_text(words)
        });
        let said = self.change(Actor::Chief(chief), |cx, env| {
            chats::say_with(cx, env, agent, chief, &request, &line)
        })?;
        self.connections.owe(
            client,
            OwedReply {
                id: say.id,
                conversation: said.conversation,
                request: said.request,
            },
        );
        Ok(())
    }

    /// Asks `agent`'s conversation to summarize its earlier part now.
    pub(crate) fn compact_chat(
        &mut self,
        agent: AgentId,
    ) -> Result<String> {
        let chief = self.realm.chief();
        self.change(Actor::Chief(chief), |cx, env| {
            chats::compact(cx, env, agent)
        })?;
        Ok(COMPACTING.into())
    }

    /// Takes the chief's queued line `item` back out of `agent`'s
    /// queue.
    pub(crate) fn withdraw_line(
        &mut self,
        agent: AgentId,
        item: &str,
    ) -> Done<String> {
        let chief = self.realm.chief();
        let done = self.attempt(Actor::Chief(chief), |cx, _| {
            chats::withdraw(cx, agent, chief, item)
        })?;
        if done.is_ok()
            && let Some(conversation) =
                chats::of(self.realm.store().connection(), agent)?
            && let Some(request) = item.strip_prefix(QUEUED)
        {
            for (client, id) in
                self.connections.settle(conversation, request)
            {
                let answer =
                    reply(&id, Ok::<_, Rejection>(String::new()));
                self.connections.send(client, answer);
            }
        }
        Ok(done.map(|()| String::new()))
    }

    /// Ends the hold a stop put on each of `agents`, in one change.
    pub(crate) fn resume_agents(
        &mut self,
        agents: &[AgentId],
    ) -> Result<String> {
        let chief = self.realm.chief();
        let resumed = self.change(Actor::Chief(chief), |cx, _| {
            let mut resumed = 0;
            for agent in agents {
                if chats::resume(cx, *agent)? {
                    resumed += 1;
                }
            }
            Ok(resumed)
        })?;
        Ok(match resumed {
            0 => "Nothing was stopped.".to_owned(),
            1 => "Resumed.".to_owned(),
            n => format!("Resumed {n} agents."),
        })
    }

    /// Brings in the chief's `project` of another program, its
    /// conversations sent first as files (C35).
    pub(crate) fn import_project(
        &mut self,
        project: &ImportProject,
    ) -> Done<String> {
        let chief = self.realm.chief();
        let files = Files::new(self.realm.layout().files());
        // Every file is held read at once.
        let together = project
            .files
            .iter()
            .filter_map(|file| files.size(file))
            .sum::<u64>();
        if together > ImportProject::MOST_BYTES {
            return Ok(Err(Rejection::ProjectTooBig {
                most: ImportProject::MOST_BYTES >> 20,
            }));
        }
        let mut conversations = Vec::new();
        for file in &project.files {
            conversations.push(done!(imports::read(&files, file)));
        }
        let wanted = imports::Project {
            name: &project.name,
            folder: project.folder.as_deref(),
            conversations: &conversations,
        };
        let done = self.attempt(Actor::Chief(chief), |cx, env| {
            imports::bring_in_project(cx, env, chief, &wanted)
        })?;
        Ok(done.map(|brought| brought.words()))
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
        let working = self.working()?;
        let stopped = self.change(Actor::Chief(chief), |cx, _| {
            let mut stopped = 0;
            for agent in agents {
                stopped += chats::stop(cx, *agent, &working)?;
            }
            Ok(stopped)
        })?;
        let turns = if stopped == 1 { "turn" } else { "turns" };
        Ok(format!("Stopped {stopped} running {turns}."))
    }

    /// Rewinds `agent`'s conversation to before the chief's line
    /// `item`.
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
            Rejection,
        >,
    > {
        let chief = self.realm.chief();
        let done = self.attempt(Actor::Chief(chief), |cx, env| {
            chats::rewind(cx, env, agent, chief, item)
        })?;
        self.rewatch(agent)?;
        Ok(done)
    }

    /// The agent `to` names, or `None` once request `id` was rejected
    /// because this server's chief may not reach it.
    pub(crate) fn reached(
        &mut self,
        client: u64,
        id: &RequestId,
        to: u64,
    ) -> Result<Option<AgentId>> {
        let agent = AgentId(i64::try_from(to)?);
        let chief = Actor::Chief(self.realm.chief());
        let db = self.realm.store().connection();
        match authority::reaches(db, chief, agent)? {
            Ok(()) => Ok(Some(agent)),
            Err(rejection) => {
                self.answer(client, reply(id, Err(rejection)))?;
                Ok(None)
            }
        }
    }

    pub(crate) fn has_model(
        &mut self,
        agent: AgentId,
    ) -> Result<bool> {
        let db = self.realm.store().connection();
        models::has_model(
            db,
            &self.env.settings,
            &self.env.overrides,
            agent,
        )
    }

    /// Every living agent this server's chief reaches.
    pub(crate) fn reachable(&mut self) -> Result<Vec<AgentId>> {
        let chief = Actor::Chief(self.realm.chief());
        let db = self.realm.store().connection();
        let living = {
            use diesel::prelude::*;
            use hamlet_store::schema::agent;
            agent::table
                .filter(agent::ended_at.is_null())
                .select(agent::id)
                .load::<AgentId>(db)?
        };
        let mut out = Vec::new();
        for agent in living {
            if authority::reaches(db, chief, agent)?.is_ok() {
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
        let db = self.realm.store().connection();
        if let Err(rejection) =
            authority::reaches(db, Actor::Chief(chief), agent)?
        {
            return Ok(Err(rejection));
        }
        // An agent that left keeps its conversation, read as it was.
        if let Some(conversation) = chats::of(db, agent)? {
            return Ok(Ok(conversation));
        }
        let conversation = self
            .change(Actor::Chief(chief), |cx, env| {
                chats::current(cx, env, agent)
            })?;
        Ok(Ok(conversation))
    }

    /// The fold of `conversation`, read from its kept entries when it
    /// is not in memory yet.
    pub(crate) fn live(
        &mut self,
        conversation: ConversationId,
    ) -> Result<&mut Live> {
        if !self.live.contains_key(&conversation) {
            let db = self.realm.store().connection();
            let mut entries =
                items::page(db, conversation, None, i64::MAX)?
                    .into_iter()
                    .map(|(_, item)| item)
                    .collect::<Vec<_>>();
            // A conversation brought in shows what it begins with
            // before its runtime makes it.
            if entries.is_empty() {
                entries = imports::held(db, conversation)?;
            }
            let agent = conversations::get(db, conversation)?.agent;
            let mut fold = Fold::of(&entries)?;
            fold.queue(crate::clients::fold::waiting(
                db,
                conversation,
            )?);
            fold.context = self.measured.remove(&conversation);
            fold.take();
            self.live.insert(
                conversation,
                Live {
                    agent,
                    fold,
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
        let usage = crate::society::spend::usage_view(
            self.realm.store().connection(),
            conversation,
        )?;
        // Only watched conversations follow their queue as it moves.
        let waiting = crate::clients::fold::waiting(
            self.realm.store().connection(),
            conversation,
        )?;
        self.live(conversation)?.fold.queue(waiting);
        self.stream(conversation)?;
        let fold = &self.live(conversation)?.fold;
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
            compacting: fold.compacting,
            context: fold.context,
            earlier: (start > 0)
                .then(|| fold.items[start].id.clone()),
            held: fold.held,
            items: fold.items[start..end].to_vec(),
            last: fold.last.clone(),
            pending: fold.pending,
            to: to.cloned(),
            usage,
            working: fold.working,
        }))
    }

    /// Sends every watcher of `conversation` its items again, under
    /// the ids they have now.
    pub(crate) fn retell(
        &mut self,
        conversation: ConversationId,
    ) -> Result<()> {
        for watcher in self.connections.watching(conversation) {
            let transcript = self.transcript(
                conversation,
                &watcher.chat,
                None,
                None,
            )?;
            self.answer(watcher.client, transcript)?;
        }
        Ok(())
    }

    /// Sends the watchers of `agent`'s chat the conversation it now
    /// stands for, as after a rewind.
    pub(crate) fn rewatch(&mut self, agent: AgentId) -> Result<()> {
        let with = u64::try_from(agent.0).unwrap_or_default();
        let watchers =
            self.connections.watchers(|w| w.chat.with.0 == with);
        for watcher in watchers {
            if let Ok(conversation) =
                self.watch_target(&watcher.chat)?
            {
                self.connections.rewatch(watcher.watch, conversation);
                let transcript = self.transcript(
                    conversation,
                    &watcher.chat,
                    None,
                    None,
                )?;
                self.answer(watcher.client, transcript)?;
            }
        }
        Ok(())
    }

    /// Sends every welcomed client what changed since the state it
    /// was sent last.
    pub(crate) fn send_state(&mut self) -> Result<()> {
        if !self.connections.is_any_welcomed() {
            self.clients.last_sent_state = None;
            return Ok(());
        }
        let state = self.compose()?;
        let message = match self
            .clients
            .last_sent_state
            .as_ref()
            .and_then(|old| changes(old, &state))
        {
            Some(updates) if updates.is_empty() => return Ok(()),
            Some(updates) => {
                self.clients.state_revision += 1;
                ServerMessage::Change(protocol::Changes {
                    changes: updates,
                    epoch: self.clients.state_epoch,
                    revision: self.clients.state_revision,
                })
            }
            None => {
                self.clients.state_epoch = crate::app::clock::now();
                self.clients.state_revision = 0;
                let mut whole = state.clone();
                whole.epoch = self.clients.state_epoch;
                whole.revision = 0;
                ServerMessage::State(whole)
            }
        };
        self.clients.last_sent_state = Some(state);
        self.connections.broadcast(&self.credentials, &message)
    }

    /// The state as it is now.
    pub(crate) fn compose(&mut self) -> Result<protocol::State> {
        // What each agent does: a working conversation's activity,
        // else one whose last turn failed.
        let mut activity = BTreeMap::<AgentId, Activity>::new();
        for (&conversation, live) in self.live.iter() {
            let shown = &live.activity;
            if !shown.working && !shown.faulted {
                continue;
            }
            // A failed chat is shown only while it is the agent's
            // chat: a rewind moves the chat to a fork and leaves the
            // failure behind.
            if !shown.working {
                let db = self.realm.store().connection();
                let kept = conversations::get(db, conversation)?;
                if kept.kind == conversations::Kind::Chat
                    && chats::of(db, live.agent)?
                        != Some(conversation)
                {
                    continue;
                }
            }
            let held = activity.get(&live.agent);
            if held.is_none_or(|held| shown.working && !held.working)
            {
                activity.insert(live.agent, shown.clone());
            }
        }
        self.prune_terminals()?;
        let terminals = self.terminals.views();
        let chief = self.realm.chief();
        let layout = self.realm.layout().clone();
        let confinement = self.confinement();
        let settings_view = self.settings_view();
        let providers = self.provider_views()?;
        let db = self.realm.store().connection();
        let chief_name = crate::society::chiefs::find(db, chief)?
            .map(|c| c.name)
            .unwrap_or_default();
        let waits = crate::society::oversight::views(db)?;
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
            actions: self.actions.listed(),
            settings_view,
            waits,
            library,
            terminals,
            learned: &self.learned.0,
        };
        super::state::compose(
            db,
            &sources,
            self.clients.state_epoch,
            self.clients.state_revision,
            crate::app::clock::now(),
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
            mcp: self.tools.mcp.view(),
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
