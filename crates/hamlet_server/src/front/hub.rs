//! The agents' chats the door follows for its clients: one `chat`
//! subscription per agent, held while a client watches one of its
//! chats or is owed the reply of a line to it, and dropped after, so
//! the door keeps no agent awake for nothing.
//!
//! A line's reply is owed until the agent's work on it ends: the
//! first idle status of the chief's chat after the realm said the
//! agent took the line and a read of that chat showed it busy, or
//! that read itself when it shows the agent idle.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use anyhow::Result;
use hamlet_actor::Action;
use hamlet_actor::action::encode_positional;
use hamlet_host::{Address, Item};
use hamlet_messages::address::{self, Party};
use hamlet_messages::agent::chat::{
    ChatEvent, ChatOp, ChatPage, ReadChat,
};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::{
    self as protocol, AgentId, Open, RequestId, ServerMessage,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

use super::call::{Calls, failed, refused_words, server_failed};
use super::route::number;
use super::{Frames, Keys, To, frames};
use crate::clients::check::{fault, reply};
use crate::diagnostic::{self, Secrets};
use crate::society::refusal::Refusal;

/// The chats the door follows, shared by the door and each agent's
/// follow.
#[derive(Clone)]
pub(crate) struct Hub {
    shared: Arc<Shared>,
}

struct Shared {
    calls: Calls,
    frames: Frames,
    credentials: Keys,
    followed: Mutex<BTreeMap<AgentNumber, Followed>>,
}

/// An agent whose `chat` a task of its own follows, and what needs
/// it. The task alone removes it, once nothing does.
struct Followed {
    jobs: mpsc::UnboundedSender<Job>,
    task: AbortHandle,
    watches: Vec<Watch>,
    owed: Vec<Owed>,
    /// The last status of the chief's current chat: the change that
    /// told it, and whether the agent was idle.
    status: Option<(Mark, bool)>,
}

/// A chat a client watches.
struct Watch {
    client: u64,
    chat: protocol::ChatRef,
    /// The last change its page holds; none until the page is sent.
    seen: Option<Mark>,
}

/// Where a change stands in the agent's changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mark {
    epoch: u32,
    number: u32,
}

impl Mark {
    fn of(page: &ChatPage) -> Self {
        Self {
            epoch: page.epoch,
            number: page.number,
        }
    }

    /// Whether the change at `self` came after the one at `then`. The
    /// changes of a later wake count again from 0, and come after.
    fn after(self, then: Self) -> bool {
        self.epoch != then.epoch || self.number > then.number
    }
}

/// The reply of a line a client is owed.
struct Owed {
    client: u64,
    id: RequestId,
    taken: Taken,
}

enum Taken {
    /// The realm has not said the agent took the line.
    Not,
    /// It took it: the first idle status after this change ends its
    /// work, or any idle status when the change is not known.
    After(Option<Mark>),
}

/// What a follow does for the door, in the order it is asked.
enum Job {
    /// Reads the page `open` asks for and sends it to `client`, who
    /// then follows that chat.
    Page { client: u64, open: Open },
    /// Tells once the subscription is held.
    Held(oneshot::Sender<()>),
    /// Reads the current session again for its watchers, then tells.
    Reread(oneshot::Sender<()>),
    /// Something that needed the follow went away.
    Look,
}

impl Hub {
    pub(crate) fn new(
        calls: Calls,
        frames: Frames,
        credentials: Keys,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                calls,
                frames,
                credentials,
                followed: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// Sends `client` the page `open` asks for, and from then on the
    /// changes of its chat.
    pub(crate) fn watch(&self, client: u64, open: Open) {
        let Some(agent) = number(open.chat.with) else {
            let refusal = Refusal::NoOne {
                address: open.chat.with.0.to_string(),
            };
            self.shared.send(client, reply(&open.id, Err(refusal)));
            return;
        };
        let shared = &self.shared;
        let mut followed = shared.followed();
        let found = shared.follow(&mut followed, agent);
        if !found.watches.iter().any(|watch| {
            watch.client == client && watch.chat == open.chat
        }) {
            found.watches.push(Watch {
                client,
                chat: open.chat.clone(),
                seen: None,
            });
        }
        let _ = found.jobs.send(Job::Page { client, open });
    }

    /// Stops sending `client` the changes of `chat`.
    pub(crate) fn close(
        &self,
        client: u64,
        chat: &protocol::ChatRef,
    ) {
        let mut followed = self.shared.followed();
        for found in followed.values_mut() {
            found.watches.retain(|watch| {
                !(watch.client == client && watch.chat == *chat)
            });
            let _ = found.jobs.send(Job::Look);
        }
    }

    /// Drops what `client`, whose connection closed, watched and was
    /// owed. Its lines go on.
    pub(crate) fn forget(&self, client: u64) {
        let mut followed = self.shared.followed();
        for found in followed.values_mut() {
            found.watches.retain(|watch| watch.client != client);
            found.owed.retain(|owed| owed.client != client);
            let _ = found.jobs.send(Job::Look);
        }
    }

    /// Owes `client` the reply of its line `id` to `agent`, once
    /// `agent`'s chat is followed, so no change after it is missed.
    /// An `Err` means the hub answered the line already.
    pub(crate) async fn owe(
        &self,
        client: u64,
        agent: AgentNumber,
        id: &RequestId,
    ) -> Result<(), oneshot::error::RecvError> {
        let (held, holds) = oneshot::channel();
        {
            let shared = &self.shared;
            let mut followed = shared.followed();
            let found = shared.follow(&mut followed, agent);
            found.owed.push(Owed {
                client,
                id: id.clone(),
                taken: Taken::Not,
            });
            let _ = found.jobs.send(Job::Held(held));
        }
        holds.await
    }

    /// Takes in that `agent` took the line `id` of `client`: reads
    /// the chief's chat to learn where the agent's work on it stands.
    pub(crate) async fn taken(
        &self,
        client: u64,
        agent: AgentNumber,
        id: &RequestId,
    ) {
        let read = self.shared.current(agent).await.ok();
        let shared = &self.shared;
        let mut followed = shared.followed();
        let Some(found) = followed.get_mut(&agent) else {
            return;
        };
        let Some(at) = found
            .owed
            .iter()
            .position(|owed| owed.client == client && owed.id == *id)
        else {
            return;
        };
        let ended = match (read, found.status) {
            (Some((_, true)), _) => true,
            (Some((mark, false)), Some((then, idle))) => {
                idle && then.after(mark)
            }
            _ => false,
        };
        if ended {
            let owed = found.owed.remove(at);
            shared.settle(&owed);
            let _ = found.jobs.send(Job::Look);
        } else {
            found.owed[at].taken =
                Taken::After(read.map(|(mark, _)| mark));
        }
    }

    /// Answers the line `id` of `client` to `agent` with `message`,
    /// unless it was answered.
    pub(crate) fn settle(
        &self,
        agent: AgentNumber,
        client: u64,
        id: &RequestId,
        message: ServerMessage,
    ) {
        let mut followed = self.shared.followed();
        let Some(found) = followed.get_mut(&agent) else {
            return;
        };
        let before = found.owed.len();
        found.owed.retain(|owed| {
            !(owed.client == client && owed.id == *id)
        });
        if found.owed.len() < before {
            self.shared.send(client, message);
        }
        let _ = found.jobs.send(Job::Look);
    }

    /// Sends the watchers of `agent`'s current session that session
    /// read again, as after it began again.
    pub(crate) async fn reread(&self, agent: AgentNumber) {
        let (told, tells) = oneshot::channel();
        {
            let followed = self.shared.followed();
            let Some(found) = followed.get(&agent) else {
                return;
            };
            let _ = found.jobs.send(Job::Reread(told));
        }
        let _ = tells.await;
    }

    /// Stops every follow, as the server closes.
    pub(crate) fn close_all(&self) {
        let mut followed = self.shared.followed();
        for found in followed.values() {
            found.task.abort();
        }
        followed.clear();
    }
}

impl Shared {
    fn followed(
        &self,
    ) -> MutexGuard<'_, BTreeMap<AgentNumber, Followed>> {
        self.followed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The follow of `agent`, started now if there is none.
    fn follow<'a>(
        self: &Arc<Self>,
        followed: &'a mut BTreeMap<AgentNumber, Followed>,
        agent: AgentNumber,
    ) -> &'a mut Followed {
        followed.entry(agent).or_insert_with(|| {
            let (jobs, taken) = mpsc::unbounded_channel();
            let task = tokio::spawn(self.clone().run(agent, taken))
                .abort_handle();
            Followed {
                jobs,
                task,
                watches: Vec::new(),
                owed: Vec::new(),
                status: None,
            }
        })
    }

    fn send(&self, client: u64, message: ServerMessage) {
        let _ = self.frames.send((To::One(client), message));
    }

    fn tell(&self, words: String) {
        diagnostic::tell(
            Secrets::Of(&self.credentials.read()),
            words,
        );
    }

    /// Answers `owed` with the empty notice.
    fn settle(&self, owed: &Owed) {
        self.send(owed.client, reply(&owed.id, Ok(String::new())));
    }

    /// Follows `agent`'s chat while something needs it.
    async fn run(
        self: Arc<Self>,
        agent: AgentNumber,
        mut jobs: mpsc::UnboundedReceiver<Job>,
    ) {
        let subscribed = async {
            let at = address::Agent(agent).address()?;
            self.calls.subscribe(&at, "chat").await
        };
        let mut subscription = match subscribed.await {
            Ok(subscription) => subscription,
            Err(error) => {
                self.tell(format!(
                    "cannot follow agent {}'s chat: {error:#}",
                    agent.0
                ));
                return self.end(agent, &mut jobs, &error);
            }
        };
        loop {
            tokio::select! {
                job = jobs.recv() => match job {
                    Some(job) => self.job(agent, job).await,
                    None => return,
                },
                item = subscription.next() => match item {
                    Ok(Item::Event(args)) => {
                        if self.event(agent, &args) {
                            self.reread_all(agent, false).await;
                        }
                    }
                    Ok(Item::Gap) => self.reread_all(agent, false).await,
                    Err(error) => {
                        self.tell(format!(
                            "agent {}'s chat ended: {error:#}",
                            agent.0
                        ));
                        return self.end(agent, &mut jobs, &error);
                    }
                },
            }
            if !self.needed(agent) {
                return;
            }
        }
    }

    /// Whether anything needs `agent`'s follow; when nothing does,
    /// it is removed.
    fn needed(&self, agent: AgentNumber) -> bool {
        let mut followed = self.followed();
        let needed = followed.get(&agent).is_some_and(|found| {
            !found.watches.is_empty() || !found.owed.is_empty()
        });
        if !needed {
            followed.remove(&agent);
        }
        needed
    }

    /// Ends `agent`'s follow, which failed with `error`: each page
    /// asked and each reply owed is answered with the failure.
    fn end(
        &self,
        agent: AgentNumber,
        jobs: &mut mpsc::UnboundedReceiver<Job>,
        error: &anyhow::Error,
    ) {
        let removed = self.followed().remove(&agent);
        jobs.close();
        while let Ok(job) = jobs.try_recv() {
            if let Job::Page { client, open } = job {
                self.send(client, failed(&open.id, error));
            }
        }
        for owed in removed.into_iter().flat_map(|found| found.owed) {
            self.send(owed.client, failed(&owed.id, error));
        }
    }

    async fn job(&self, agent: AgentNumber, job: Job) {
        match job {
            Job::Page { client, open } => {
                self.page(agent, client, open).await;
            }
            Job::Held(held) => {
                let _ = held.send(());
            }
            Job::Reread(told) => {
                self.reread_all(agent, true).await;
                let _ = told.send(());
            }
            Job::Look => {}
        }
    }

    /// Sends `client` the page `open` asks for, from which it follows
    /// the chat; or the fault of why it could not be read.
    async fn page(
        &self,
        agent: AgentNumber,
        client: u64,
        open: Open,
    ) {
        let read = self.read(agent, &open).await;
        let mut followed = self.followed();
        let Some(found) = followed.get_mut(&agent) else {
            return;
        };
        let Some(at) = found.watches.iter().position(|watch| {
            watch.client == client && watch.chat == open.chat
        }) else {
            return;
        };
        let transcript = read.and_then(|page| {
            let mark = Mark::of(&page);
            let transcript = frames::transcript(
                page,
                open.chat.clone(),
                Some(open.id.clone()),
            )?;
            Ok((mark, transcript))
        });
        match transcript {
            Ok((mark, transcript)) => {
                found.watches[at].seen = Some(mark);
                self.send(
                    client,
                    ServerMessage::Transcript(transcript),
                );
            }
            Err(error) => {
                if found.watches[at].seen.is_none() {
                    found.watches.remove(at);
                }
                let fault = match refused_words(&error) {
                    Some(words) => fault(Some(&open.id), words),
                    None => server_failed(Some(&open.id), &error),
                };
                self.send(client, fault);
            }
        }
    }

    /// Takes in a change of `agent`'s chats: a turn to each watcher
    /// of its chat, and an idle status of the chief's current chat
    /// ends the work on the lines taken before it. `true` when the
    /// agent says its chats must be read again.
    fn event(&self, agent: AgentNumber, args: &[u8]) -> bool {
        let event =
            match ciborium::from_reader::<(ChatEvent,), _>(args) {
                Ok((event,)) => event,
                Err(error) => {
                    self.tell(format!(
                        "a change of agent {}'s chat: {error}",
                        agent.0
                    ));
                    return false;
                }
            };
        let mark = Mark {
            epoch: event.epoch,
            number: event.number,
        };
        let current =
            event.chat.from.is_none() && event.chat.past.is_none();
        let reset =
            event.ops.iter().any(|op| matches!(op, ChatOp::Reset));
        let status = event.ops.iter().rev().find_map(|op| match op {
            ChatOp::Status { working, pending } => {
                Some(!working && *pending == 0)
            }
            _ => None,
        });
        let mut followed = self.followed();
        let Some(found) = followed.get_mut(&agent) else {
            return false;
        };
        for watch in &found.watches {
            if !watch.seen.is_some_and(|seen| mark.after(seen)) {
                continue;
            }
            match frames::turn(event.clone(), &watch.chat) {
                Ok(Some(turn)) => {
                    self.send(
                        watch.client,
                        ServerMessage::Turn(turn),
                    );
                }
                Ok(None) => {}
                Err(error) => self.tell(format!(
                    "a change of agent {}'s chat: {error}",
                    agent.0
                )),
            }
        }
        if current && let Some(idle) = status {
            found.status = Some((mark, idle));
            if idle {
                self.ended(found, |taken| {
                    taken.is_none_or(|taken| mark.after(taken))
                });
            }
        }
        reset
    }

    /// Answers each line taken whose work `ends` says ended.
    fn ended(
        &self,
        found: &mut Followed,
        ends: impl Fn(Option<Mark>) -> bool,
    ) {
        found.owed.retain(|owed| {
            let ended = matches!(owed.taken, Taken::After(taken) if ends(taken));
            if ended {
                self.settle(owed);
            }
            !ended
        });
    }

    /// Sends the watchers of `agent`'s chats, or of its current
    /// session only when `current`, the chat read again, and ends
    /// the work on the lines taken when the agent is idle.
    async fn reread_all(&self, agent: AgentNumber, current: bool) {
        let (mut chats, owed) = {
            let followed = self.followed();
            let Some(found) = followed.get(&agent) else {
                return;
            };
            let chats = found
                .watches
                .iter()
                .map(|watch| watch.chat.clone())
                .filter(|chat| {
                    !current
                        || (chat.from.is_none()
                            && chat.past.is_none())
                })
                .collect::<Vec<_>>();
            (chats, !found.owed.is_empty())
        };
        chats.dedup();
        let chief = chief_chat(agent);
        if owed && !chats.contains(&chief) {
            chats.push(chief.clone());
        }
        for chat in chats {
            let open = Open {
                id: RequestId::new(),
                chat: chat.clone(),
                before: None,
            };
            let page = match self.read(agent, &open).await {
                Ok(page) => page,
                Err(error) => {
                    self.tell(format!(
                        "cannot read agent {}'s chat again: \
                         {error:#}",
                        agent.0
                    ));
                    continue;
                }
            };
            let mark = Mark::of(&page);
            let idle = !page.working && page.pending == 0;
            let transcript =
                frames::transcript(page, chat.clone(), None);
            let mut followed = self.followed();
            let Some(found) = followed.get_mut(&agent) else {
                return;
            };
            if chat == chief {
                found.status = Some((mark, idle));
                if idle {
                    self.ended(found, |_| true);
                }
            }
            let Ok(transcript) = transcript else {
                continue;
            };
            for watch in &mut found.watches {
                if watch.chat == chat && watch.seen.is_some() {
                    watch.seen = Some(mark);
                    self.send(
                        watch.client,
                        ServerMessage::Transcript(transcript.clone()),
                    );
                }
            }
        }
    }

    /// Where the chief's current chat with `agent` stands, and
    /// whether the agent is idle in it.
    async fn current(
        &self,
        agent: AgentNumber,
    ) -> Result<(Mark, bool)> {
        let open = Open {
            id: RequestId::new(),
            chat: chief_chat(agent),
            before: None,
        };
        let page = self.read(agent, &open).await?;
        Ok((Mark::of(&page), !page.working && page.pending == 0))
    }

    async fn read(
        &self,
        agent: AgentNumber,
        open: &Open,
    ) -> Result<ChatPage> {
        let at: Address = address::Agent(agent).address()?;
        let answer = self
            .calls
            .send(
                &at,
                ReadChat::NAME,
                encode_positional(&ReadChat(open.clone()))?,
            )
            .await?;
        Ok(ciborium::from_reader(answer.as_slice())?)
    }
}

/// The chief's current chat with `agent`.
fn chief_chat(agent: AgentNumber) -> protocol::ChatRef {
    protocol::ChatRef {
        from: None,
        kind: None,
        past: None,
        with: AgentId(u64::from(agent.0)),
    }
}
