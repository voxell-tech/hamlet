//! What each client message asks, answered by the loop: the hello,
//! a chief's lines and commands, watching conversations, and a chat's
//! deeds.

use anyhow::Result;
use hamlet_protocol::client::{
    self as protocol, ChatRef, ChatRefKind, ClientMessage, RequestId,
    ServerMessage, Transcript,
};
use hamlet_store::ids::{Actor, AgentId, ConversationId};

use super::check::{fault, reply};
use super::connection::{self, Event};
use super::fold::Fold;
use super::{Owed, Watch, changes};
use crate::core::{Core, Live};
use crate::society::agents::{self, AgentKind};
use crate::society::refusal::Refusal;
use crate::society::{authority, hires, workers};
use crate::talk::activity::Activity;
use crate::talk::conversations::{self, Kind};
use crate::talk::{chats, items};

/// The most transcript entries one page holds.
const PAGE: usize = 100;

impl Core {
    /// Takes in what a connection did.
    pub(crate) async fn connection(
        &mut self,
        event: Event,
    ) -> Result<()> {
        match event {
            Event::Message(client, text) => {
                self.message(client, &text).await
            }
            Event::Late(client) => {
                if let Some(found) =
                    self.clients.list.get_mut(&client)
                    && !found.welcomed
                {
                    connection::refuse(
                        found,
                        "No hello came in time.",
                        1008,
                    )?;
                }
                Ok(())
            }
            Event::Gone(client) => {
                self.clients.gone(client);
                self.providers.disconnect(client);
                self.send_providers().await
            }
        }
    }

    async fn message(
        &mut self,
        client: u64,
        text: &str,
    ) -> Result<()> {
        let Some(found) = self.clients.list.get(&client) else {
            return Ok(());
        };
        if found.refused {
            return Ok(());
        }
        let welcomed = found.welcomed;
        let message = match self.clients.check.parse(text) {
            Ok(message) => message,
            Err(failed) => {
                let message = ServerMessage::Fault(failed);
                self.answer(client, message)?;
                if !welcomed
                    && let Some(found) =
                        self.clients.list.get_mut(&client)
                {
                    connection::refuse(
                        found,
                        "The first message is a hello.",
                        1008,
                    )?;
                }
                return Ok(());
            }
        };
        match message {
            ClientMessage::Hello(hello) if !welcomed => {
                let found = self
                    .clients
                    .list
                    .get_mut(&client)
                    .expect("checked above");
                if !self.credentials.is_connection_token(&hello.token)
                {
                    return connection::refuse(
                        found,
                        "That token is not this server's.",
                        4001,
                    );
                }
                let ours = protocol::VERSION;
                if !ours.is_compatible(&hello.v) {
                    let words = format!(
                        "This `hello` message does not fit the \
                         protocol: `v` {} is not compatible with \
                         {ours}.",
                        hello.v
                    );
                    return connection::refuse(found, &words, 4002);
                }
                found.welcomed = true;
                let state = self.compose()?;
                self.clients.sent = Some(state.clone());
                self.answer(client, ServerMessage::State(state))
            }
            _ if !welcomed => {
                let found = self
                    .clients
                    .list
                    .get_mut(&client)
                    .expect("checked above");
                connection::refuse(
                    found,
                    "The first message is a hello.",
                    1008,
                )
            }
            message => {
                let id = message.id().cloned();
                if let Err(error) =
                    self.request(client, message).await
                {
                    crate::diagnostic::tell(
                        crate::diagnostic::Secrets::Of(
                            &self.credentials,
                        ),
                        format!("a request failed: {error:#}"),
                    );
                    self.answer(
                        client,
                        fault(
                            id.as_ref(),
                            format!("The server failed: {error}"),
                        ),
                    )?;
                }
                Ok(())
            }
        }
    }

    /// Sends `message` to `client`.
    pub(crate) fn answer(
        &mut self,
        client: u64,
        message: ServerMessage,
    ) -> Result<()> {
        self.clients.send(client, &self.credentials, message)
    }

    async fn request(
        &mut self,
        client: u64,
        message: ClientMessage,
    ) -> Result<()> {
        let chief = self.realm.chief();
        if self.model_request(client, &message).await? {
            return Ok(());
        }
        match message {
            ClientMessage::Hello(_) => Ok(()),
            ClientMessage::Say(say) => {
                let agent = AgentId(i64::try_from(say.to.0)?);
                if let Err(refusal) = self.reaches(agent)? {
                    return self.answer(
                        client,
                        reply(&say.id, Err(refusal)),
                    );
                }
                let text = say.text.trim();
                if text.starts_with('/') {
                    let answer = self.command(agent, text).await?;
                    return self
                        .answer(client, reply(&say.id, answer));
                }
                let request = say.id.as_str().to_owned();
                let line = self.credentials.scrub_text(&say.text);
                let said = self
                    .change(Actor::Chief(chief), |cx, env| {
                        chats::say(
                            cx, env, agent, chief, &request, &line,
                        )
                    })
                    .await?;
                self.clients.owed.push(Owed {
                    client,
                    id: say.id,
                    conversation: said.conversation,
                    request: said.request,
                });
                Ok(())
            }
            ClientMessage::Open(open) => {
                let found = self.watch_target(&open.chat).await?;
                let conversation = match found {
                    Ok(conversation) => conversation,
                    Err(refusal) => {
                        return self.answer(
                            client,
                            reply(&open.id, Err(refusal)),
                        );
                    }
                };
                let transcript = self.transcript(
                    conversation,
                    &open.chat,
                    open.before.as_deref(),
                    Some(&open.id),
                )?;
                self.clients.watches.retain(|w| {
                    !(w.client == client && w.chat == open.chat)
                });
                self.clients.watches.push(Watch {
                    client,
                    chat: open.chat,
                    conversation,
                });
                self.answer(client, transcript)
            }
            ClientMessage::Close(close) => {
                self.clients.watches.retain(|w| {
                    !(w.client == client && w.chat == close.chat)
                });
                Ok(())
            }
            ClientMessage::New(new) => {
                let agent = AgentId(i64::try_from(new.to.0)?);
                if let Err(refusal) = self.reaches(agent)? {
                    return self.answer(
                        client,
                        reply(&new.id, Err(refusal)),
                    );
                }
                let number = self
                    .change(Actor::Chief(chief), |cx, env| {
                        chats::new_session(cx, env, agent, chief)
                    })
                    .await?;
                self.rewatch(agent).await?;
                self.answer(
                    client,
                    reply(
                        &new.id,
                        Ok(format!("Session {number} began.")),
                    ),
                )
            }
            ClientMessage::Resume(resume) => {
                let agent = AgentId(i64::try_from(resume.to.0)?);
                if let Err(refusal) = self.reaches(agent)? {
                    return self.answer(
                        client,
                        reply(&resume.id, Err(refusal)),
                    );
                }
                let done = self
                    .attempt(Actor::Chief(chief), |cx, _| {
                        chats::resume(cx, agent, chief, resume.number)
                    })
                    .await?;
                self.rewatch(agent).await?;
                let words = done.map(|()| {
                    format!("Back in session {}.", resume.number)
                });
                self.answer(client, reply(&resume.id, words))
            }
            ClientMessage::Retry(retry) => {
                let agent = AgentId(i64::try_from(retry.to.0)?);
                if let Err(refusal) = self.reaches(agent)? {
                    return self.answer(
                        client,
                        reply(&retry.id, Err(refusal)),
                    );
                }
                let done = self
                    .attempt(Actor::Chief(chief), |cx, env| {
                        chats::retry(cx, env, agent, chief)
                    })
                    .await?;
                let words = done.map(|_| "Carrying on.".to_owned());
                self.answer(client, reply(&retry.id, words))
            }
            ClientMessage::Stop(stop) => {
                let targets = match stop.agent {
                    Some(agent) => {
                        let agent = AgentId(i64::try_from(agent.0)?);
                        if let Err(refusal) = self.reaches(agent)? {
                            return self.answer(
                                client,
                                reply(&stop.id, Err(refusal)),
                            );
                        }
                        vec![agent]
                    }
                    None => self.reachable()?,
                };
                let mut stopped = 0;
                for agent in targets {
                    stopped += self
                        .change(Actor::Chief(chief), |cx, _| {
                            chats::stop(cx, agent)
                        })
                        .await?;
                }
                let words = if stopped == 0 {
                    "Nothing was running.".to_owned()
                } else {
                    format!("Stopped {stopped} running turns.")
                };
                self.answer(client, reply(&stop.id, Ok(words)))
            }
            ClientMessage::Edit(edit) => {
                self.edit(client, edit).await
            }
            ClientMessage::Read(read) => {
                self.read(client, read).await
            }
            ClientMessage::Answer(answer) => {
                let words = self
                    .answer_wait(
                        &answer.wait,
                        &answer.choice,
                        answer.text.as_deref(),
                        answer.picks.as_deref(),
                    )
                    .await?;
                self.answer(client, reply(&answer.id, words))
            }
            message => {
                let what = match &message {
                    ClientMessage::Give(_)
                    | ClientMessage::GivePart(_) => "gifts",
                    ClientMessage::Rewind(_) => {
                        "taking a conversation back"
                    }
                    ClientMessage::MoveLodges(_) => "moving lodges",
                    _ => "that",
                };
                match message.id() {
                    Some(id) => {
                        let id = id.clone();
                        self.answer(
                            client,
                            reply(
                                &id,
                                Err(Refusal::Unsupported {
                                    what: what.into(),
                                }),
                            ),
                        )
                    }
                    None => Ok(()),
                }
            }
        }
    }

    /// Refuses unless this server's chief may reach `agent` (C7): the
    /// Hand, a hire of one of its hamlets, or a worker started by it
    /// or in its hamlets.
    pub(crate) fn reaches(
        &mut self,
        agent: AgentId,
    ) -> Result<Result<(), Refusal>> {
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
    fn reachable(&mut self) -> Result<Vec<AgentId>> {
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
    async fn watch_target(
        &mut self,
        chat: &ChatRef,
    ) -> Result<Result<ConversationId, Refusal>> {
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
                    let conversation = self
                        .change(Actor::Chief(chief), |cx, env| {
                            chats::current(cx, env, agent, chief)
                        })
                        .await?;
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
    fn transcript(
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
    async fn rewatch(&mut self, agent: AgentId) -> Result<()> {
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
                self.watch_target(&watch.chat).await?
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
    pub(crate) async fn publish(&mut self) -> Result<()> {
        if !self.clients.list.values().any(|c| c.welcomed) {
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
        self.clients.broadcast(&self.credentials, &message)
    }

    /// The state as it is now.
    pub(crate) fn compose(&mut self) -> Result<protocol::State> {
        let activity = self
            .live
            .values()
            .filter(|live| live.activity.working)
            .map(|live| (live.agent, live.activity.clone()))
            .collect();
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
        let sources = super::state::Sources {
            layout: &layout,
            env: &self.env,
            chief: chief_name,
            confinement,
            activity: &activity,
            providers,
            deeds: crate::commands::deeds(),
            settings_view,
            waits,
            library: Vec::new(),
            starters: Vec::new(),
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
            effort: settings.effort.map(hires::Effort::wire),
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
            reflect_every: i64::try_from(settings.reflect_every)
                .unwrap_or(0),
            unset: !settings.saved,
            workspace: protocol::SettingsViewWorkspace {
                moving: None,
                running: None,
                url: None,
            },
        }
    }
}
