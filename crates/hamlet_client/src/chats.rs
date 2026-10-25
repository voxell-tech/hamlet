//! Watched conversations, filled from transcripts and turn
//! operations.

use std::collections::HashMap;

use bevy_app::App;
use bevy_ecs::prelude::*;
use hamlet_protocol::client::{self as protocol, transcript};

use crate::index::{Index, Refiled};
use crate::messages::{
    JobFinished, Reconnected, Response, SessionOutput,
};
use crate::outbox::Outbox;
use crate::realm::Chief;

/// Watched chats and their incoming operations.
pub fn plugin(app: &mut App) {
    let world = app.world_mut();
    world.register_component::<Chat>();
    world.register_component::<TalksTo>();
    world.register_component::<Conversations>();
    world.register_component::<Transcript>();
    world.register_component::<Status>();
    world.register_component::<Usage>();
    world.register_component::<ConversationNumber>();
    app.init_resource::<Watches>()
        .add_observer(watch)
        .add_observer(unwatch)
        .add_observer(
            |gone: On<Remove<Chat>>, mut commands: Commands| {
                commands.entity(gone.entity).try_remove::<TalksTo>();
            },
        )
        .add_observer(follow)
        .add_observer(reopen)
        .add_observer(
            |response: On<Response>, mut watches: ResMut<Watches>| {
                if matches!(
                    response.message,
                    protocol::ServerMessage::Reply(_)
                        | protocol::ServerMessage::Fault(_)
                ) {
                    watches
                        .pages
                        .retain(|_, page| page.id != response.to);
                }
            },
        );
}

/// The earlier page each chat waits for.
#[derive(Resource, Default)]
struct Watches {
    pages: HashMap<Entity, Page>,
}

struct Page {
    id: protocol::RequestId,
    stale: bool,
}

fn open(outbox: &mut Outbox, chat: protocol::ChatRef) {
    outbox.send(|id| {
        protocol::ClientMessage::Open(protocol::Open {
            id,
            chat,
            before: None,
        })
    });
}

/// Points `chat` at `agent`, writing only on a difference.
fn talk(
    commands: &mut Commands,
    chat: Entity,
    talks: Option<&TalksTo>,
    agent: Option<Entity>,
) {
    if talks.map(|talks| talks.0) == agent {
        return;
    }
    let mut chat = commands.entity(chat);
    match agent {
        Some(agent) => chat.try_insert(TalksTo(agent)),
        None => chat.try_remove::<TalksTo>(),
    };
}

/// Opens a chat as it is watched, or watches something new.
fn watch(
    watched: On<Insert<Chat>>,
    chats: Query<(&Chat, Option<&TalksTo>)>,
    index: Res<Index>,
    mut outbox: ResMut<Outbox>,
    mut watches: ResMut<Watches>,
    mut commands: Commands,
) {
    let Ok((chat, talks)) = chats.get(watched.entity) else {
        return;
    };
    let agent = index.at(&chat.0.with);
    talk(&mut commands, watched.entity, talks, agent);
    watches.pages.remove(&watched.entity);
    open(&mut outbox, chat.0.clone());
}

/// Closes what a chat watched, removed or replaced.
fn unwatch(
    unwatched: On<Discard<Chat>>,
    chats: Query<&Chat>,
    mut outbox: ResMut<Outbox>,
    mut watches: ResMut<Watches>,
) {
    let Ok(chat) = chats.get(unwatched.entity) else {
        return;
    };
    watches.pages.remove(&unwatched.entity);
    outbox.send(|_| {
        protocol::ClientMessage::Close(protocol::Close {
            chat: chat.0.clone(),
        })
    });
}

/// Points every chat at the agent its address finds now.
fn follow(
    _: On<Refiled>,
    talks: Query<&TalksTo>,
    index: Res<Index>,
    mut commands: Commands,
) {
    for (chat, reference) in index.chats() {
        let agent = index.at(&reference.with);
        talk(&mut commands, chat, talks.get(chat).ok(), agent);
    }
}

/// Opens every chat again on a new connection.
fn reopen(
    _: On<Reconnected>,
    index: Res<Index>,
    mut outbox: ResMut<Outbox>,
    mut watches: ResMut<Watches>,
) {
    for (chat, reference) in index.chats() {
        watches.pages.remove(&chat);
        open(&mut outbox, reference.clone());
    }
}

/// The chat watching `reference`.
pub fn find(
    world: &World,
    reference: &protocol::ChatRef,
) -> Option<Entity> {
    world.get_resource::<Index>()?.chats_at(reference).next()
}

/// The received transcript or turn for watched chat entities.
pub fn receive(world: &mut World, message: protocol::ServerMessage) {
    let reference = match &message {
        protocol::ServerMessage::Transcript(page) => &page.chat,
        protocol::ServerMessage::Turn(turn) => &turn.chat,
        _ => return,
    };
    let chats = world
        .resource::<Index>()
        .chats_at(reference)
        .collect::<Vec<_>>();
    let chief = world.resource::<Chief>().0.clone();
    for chat in chats {
        match &message {
            protocol::ServerMessage::Transcript(page) => {
                let mut watches = world.resource_mut::<Watches>();
                let prepend =
                    watches.pages.get(&chat).is_some_and(|pending| {
                        page.to.as_ref() == Some(&pending.id)
                    });
                if prepend {
                    if watches.pages.remove(&chat).unwrap().stale {
                        continue;
                    }
                } else if let Some(pending) =
                    watches.pages.get_mut(&chat)
                {
                    pending.stale = true;
                }
                if let Some(number) = page.number {
                    world
                        .entity_mut(chat)
                        .insert(ConversationNumber(number));
                }
                let mut items = page
                    .items
                    .iter()
                    .cloned()
                    .map(|item| received_item(item, &chief))
                    .collect::<Vec<_>>();
                let mut said =
                    world.get_mut::<Transcript>(chat).unwrap();
                if prepend {
                    items.append(&mut said.items);
                }
                said.items = items;
                said.earlier = page.earlier.clone();
                said.shape += 1;
                status(
                    world,
                    chat,
                    page.working,
                    page.pending,
                    page.last
                        .as_ref()
                        .map(|last| (last.at, last.what.clone())),
                );
                world
                    .entity_mut(chat)
                    .insert(Usage(page.usage.clone()));
            }
            protocol::ServerMessage::Turn(turn) => {
                if let Some(number) = turn.number {
                    world
                        .entity_mut(chat)
                        .insert(ConversationNumber(number));
                }
                for operation in &turn.operations {
                    if matches!(
                        operation,
                        protocol::TranscriptOperation::Reset {}
                    ) && let Some(page) = world
                        .resource_mut::<Watches>()
                        .pages
                        .get_mut(&chat)
                    {
                        page.stale = true;
                    }
                    match operation {
                        protocol::TranscriptOperation::Status {
                            working,
                            pending,
                            last,
                        } => {
                            status(
                                world,
                                chat,
                                *working,
                                *pending,
                                last.as_ref().map(|last| {
                                    (last.at, last.what.clone())
                                }),
                            );
                        }
                        protocol::TranscriptOperation::Usage {
                            usage,
                        } => {
                            world
                                .entity_mut(chat)
                                .insert(Usage(usage.clone()));
                        }
                        operation => world
                            .get_mut::<Transcript>(chat)
                            .unwrap()
                            .apply(operation, &chief),
                    }
                    world.trigger(SessionOutput {
                        chat,
                        operation: operation.clone(),
                    });
                }
            }
            _ => unreachable!(),
        }
    }
}

fn status(
    world: &mut World,
    chat: Entity,
    working: bool,
    pending: u64,
    last: Option<(u64, String)>,
) {
    let mut said = world.get_mut::<Transcript>(chat).unwrap();
    let finished = said.answering && !working;
    said.answering = working;
    let mut status = world.get_mut::<Status>(chat).unwrap();
    status.pending = pending + u64::from(working);
    status.last = last;
    if finished {
        world.trigger(JobFinished { chat });
    }
}

fn received_item(item: protocol::Item, chief: &str) -> Item {
    let entry = match item.kind {
        protocol::ItemKind::User if item.author.is_some() => {
            Entry::Agent {
                name: item.author.unwrap(),
                text: item.text,
            }
        }
        protocol::ItemKind::User => Entry::Chief {
            name: item.chief.unwrap_or_else(|| chief.into()),
            text: item.text,
        },
        protocol::ItemKind::Assistant => Entry::Assistant(item.text),
        protocol::ItemKind::Reasoning => Entry::Reasoning(item.text),
        protocol::ItemKind::Notice => Entry::Notice(item.text),
        protocol::ItemKind::Error => Entry::Error(item.text),
        protocol::ItemKind::Answered => Entry::Answered(item.text),
        protocol::ItemKind::Tool => {
            let tool = item.tool.unwrap_or(protocol::ItemTool {
                name: String::new(),
                summary: String::new(),
                exit: None,
                ms: None,
                failure: None,
                change: None,
            });
            Entry::Tool {
                name: tool.name,
                summary: tool.summary,
                exit: tool.exit,
                ms: tool.ms,
                failure: tool.failure,
                change: tool.change,
            }
        }
    };
    Item {
        id: item.id,
        at: item.at,
        entry,
    }
}

/// An earlier-page request for a chat with a cursor and no page
/// outstanding.
pub fn earlier(world: &mut World, chat: Entity) {
    if world.resource::<Watches>().pages.contains_key(&chat) {
        return;
    }
    let Some(reference) =
        world.get::<Chat>(chat).map(|chat| chat.0.clone())
    else {
        return;
    };
    let Some(said) = world.get::<Transcript>(chat) else {
        return;
    };
    if said.earlier.is_none() {
        return;
    }
    let Some(before) = said.items.first().map(|item| item.id.clone())
    else {
        return;
    };
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Open(protocol::Open {
            id,
            chat: reference,
            before: Some(before),
        })
    });
    world
        .resource_mut::<Watches>()
        .pages
        .insert(chat, Page { id, stale: false });
}

/// A watched conversation identified by the server.
#[derive(Component, Clone, Debug, PartialEq)]
#[component(
    immutable,
    on_insert = crate::index::file_chat,
    on_discard = crate::index::unfile_chat
)]
#[require(Transcript, Status)]
pub struct Chat(pub protocol::ChatRef);

/// The server's stable conversation number for a watched chat.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConversationNumber(pub i64);

/// The agent addressed by a chat.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[relationship(relationship_target = Conversations)]
pub struct TalksTo(pub Entity);

/// The chats addressing an agent.
#[derive(Component, Debug, Default)]
#[relationship_target(relationship = TalksTo)]
pub struct Conversations(Vec<Entity>);

impl Conversations {
    /// The chat entities addressing this agent.
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }
}

/// A displayed chief input, agent output or tool result.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Chief {
        name: String,
        text: String,
    },
    Assistant(String),
    Reasoning(String),
    Tool {
        name: String,
        summary: String,
        exit: Option<i64>,
        ms: Option<u64>,
        failure: Option<String>,
        change: Option<protocol::Change>,
    },
    Error(String),
    Notice(String),
    /// The chief's answers, with each question quoted above them.
    Answered(String),
    Agent {
        name: String,
        text: String,
    },
}

/// A transcript entry with its server id and time.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub entry: Entry,
    /// Milliseconds since 1970.
    pub at: u64,
}

impl transcript::Kept for Item {
    fn id(&self) -> &str {
        &self.id
    }

    fn append(&mut self, text: &str) {
        match &mut self.entry {
            Entry::Chief { text: shown, .. }
            | Entry::Agent { text: shown, .. }
            | Entry::Assistant(shown)
            | Entry::Reasoning(shown)
            | Entry::Notice(shown)
            | Entry::Answered(shown)
            | Entry::Error(shown) => shown.push_str(text),
            Entry::Tool { .. } => {}
        }
    }

    fn close(&mut self, closed: transcript::Closed<'_>) -> bool {
        let Entry::Tool {
            exit,
            ms,
            failure,
            change,
            ..
        } = &mut self.entry
        else {
            return false;
        };
        *exit = Some(closed.exit);
        *ms = closed.ms;
        *failure = closed.failure.map(str::to_owned);
        if let Some(closed) = closed.change {
            *change = closed.cloned();
        }
        true
    }
}

/// The received items and paging state of a watched chat.
#[derive(Component, Default, Clone, Debug)]
pub struct Transcript {
    pub(crate) items: Vec<Item>,
    pub(crate) shape: u64,
    pub(crate) answering: bool,
    pub(crate) earlier: Option<String>,
}

impl Transcript {
    fn apply(
        &mut self,
        operation: &protocol::TranscriptOperation,
        chief: &str,
    ) {
        // An operation on an item this window does not hold changes
        // nothing.
        let _ =
            transcript::apply(&mut self.items, operation, |item| {
                received_item(item.clone(), chief)
            });
        match operation {
            protocol::TranscriptOperation::Reset {} => {
                self.earlier = None;
                self.shape += 1;
            }
            protocol::TranscriptOperation::Add { .. }
            | protocol::TranscriptOperation::Close { .. } => {
                self.shape += 1;
            }
            _ => {}
        }
    }

    /// A transcript built from received items.
    pub fn from_items(items: Vec<Item>) -> Self {
        Self {
            shape: items.len() as u64,
            items,
            ..Self::default()
        }
    }

    /// A transcript with an earlier-page cursor.
    pub fn with_earlier(mut self, earlier: Option<String>) -> Self {
        self.earlier = earlier;
        self
    }

    /// A transcript with its answering state set.
    pub fn with_answering(mut self, answering: bool) -> Self {
        self.answering = answering;
        self
    }

    /// The transcript's items in display order.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// The transcript item with a server id.
    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }

    /// A count of structural changes, unchanged while text grows.
    pub fn shape(&self) -> u64 {
        self.shape
    }

    /// The presence of an unfinished answer in the transcript.
    pub fn is_answering(&self) -> bool {
        self.answering
    }

    /// The cursor for a page before the displayed items.
    pub fn earlier(&self) -> Option<&str> {
        self.earlier.as_deref()
    }
}

/// A chat's pending work and latest status words.
#[derive(Component, Default, Clone, Debug)]
pub struct Status {
    /// Messages queued or running.
    pub pending: u64,
    pub last: Option<(u64, String)>,
}

impl Status {
    /// The presence of running or queued work.
    pub fn is_working(&self) -> bool {
        self.pending > 0
    }
}

/// The server's received usage view.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Usage(pub protocol::UsageView);

impl Usage {
    /// The server's ready-made usage words.
    pub fn summary(&self) -> String {
        self.0.summary.clone()
    }

    /// The input and output tokens reported by the server.
    pub fn tokens(&self) -> u64 {
        self.0.input + self.0.output
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use hamlet_protocol::client as protocol;
    use hamlet_testing::realm::{agent, state};

    use super::{Chat, TalksTo};
    use crate::outbox::Outbox;
    use crate::realm::{self, Left};
    use crate::{ClientPlugin, receive};

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(ClientPlugin { server: None });
        app
    }

    fn with(name: &str) -> protocol::ChatRef {
        protocol::ChatRef {
            kind: None,
            from: None,
            with: protocol::Address {
                hamlet: realm::HOME.into(),
                agent: name.into(),
            },
            past: None,
        }
    }

    #[test]
    fn a_chat_added_and_removed_in_one_update_opens_and_closes_once()
    {
        let mut app = app();
        let chat = app.world_mut().spawn(Chat(with("founder"))).id();
        app.world_mut().despawn(chat);
        app.update();
        let sent = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .collect::<Vec<_>>();
        assert!(
            matches!(&sent[..], [protocol::ClientMessage::Open(open), protocol::ClientMessage::Close(close)] if open.chat == with("founder") && close.chat == with("founder")),
            "{sent:?}"
        );
    }

    #[test]
    fn a_chat_watched_before_its_agent_arrives_talks_to_it_when_it_comes()
     {
        let mut app = app();
        let chat = app.world_mut().spawn(Chat(with("scout"))).id();
        let mut realm = state();
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(realm.clone()),
        );
        assert!(app.world().get::<TalksTo>(chat).is_none());
        realm.agents.push(agent("home", 1, "scout"));
        realm.revision += 1;
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(realm),
        );
        let scout = realm::named(app.world(), "scout").unwrap();
        assert_eq!(
            app.world().get::<TalksTo>(chat),
            Some(&TalksTo(scout))
        );
        app.world_mut().entity_mut(scout).insert(Left);
        assert!(app.world().get::<TalksTo>(chat).is_none());
    }
}
