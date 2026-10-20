//! Watched conversations, filled from transcripts and turn
//! operations.

use std::collections::HashMap;

use bevy_app::{App, PostUpdate};
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use hamlet_protocol::{transcript, v0};

use crate::messages::{
    JobFinished, Reconnected, Response, SessionOutput,
};
use crate::outbox::Outbox;
use crate::realm::{
    Chief, Citizen, Left, LivesIn, MAYOR, Mayor, Settlement,
};

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
        .add_systems(PostUpdate, watch)
        .add_observer(
            |_: On<Reconnected>, mut watches: ResMut<Watches>| {
                watches.reopen = true;
            },
        )
        .add_observer(
            |response: On<Response>, mut watches: ResMut<Watches>| {
                if matches!(
                    response.message,
                    v0::ServerMessage::Reply(_)
                        | v0::ServerMessage::Fault(_)
                ) {
                    watches
                        .pages
                        .retain(|_, page| page.id != response.to);
                }
            },
        );
}

#[derive(Resource, Default)]
struct Watches {
    chats: HashMap<Entity, v0::ChatRef>,
    pages: HashMap<Entity, Page>,
    reopen: bool,
}

struct Page {
    id: String,
    stale: bool,
}

fn open(world: &mut World, chat: v0::ChatRef) {
    world.resource_mut::<Outbox>().send(|id| {
        v0::ClientMessage::Open(v0::Open {
            id,
            chat,
            before: None,
        })
    });
}

fn watch(world: &mut World) {
    let chats = world
        .query::<(Entity, &Chat)>()
        .iter(world)
        .map(|(entity, chat)| (entity, chat.0.clone()))
        .collect::<HashMap<_, _>>();
    let mut watches = world.remove_resource::<Watches>().unwrap();
    for (entity, reference) in &watches.chats {
        if chats.get(entity) != Some(reference) {
            if let Ok(mut entity) = world.get_entity_mut(*entity) {
                entity.remove::<TalksTo>();
            }
            world.resource_mut::<Outbox>().send(|_| {
                v0::ClientMessage::Close(v0::Close {
                    chat: reference.clone(),
                })
            });
        }
    }
    let agents = world
        .query_filtered::<
            (Entity, &Name, &LivesIn, Has<Mayor>),
            (With<Citizen>, Without<Left>),
        >()
        .iter(world)
        .filter_map(|(entity, name, lives, mayor)| {
            Some((
                entity,
                world.get::<Settlement>(lives.0)?.name.clone(),
                name.as_str().to_string(),
                mayor,
            ))
        })
        .collect::<Vec<_>>();
    for (entity, reference) in &chats {
        let agent = agents
            .iter()
            .find(|(_, hamlet, name, mayor)| {
                hamlet == &reference.with.hamlet
                    && if reference.with.agent == MAYOR {
                        *mayor
                    } else {
                        name == &reference.with.agent
                    }
            })
            .map(|(entity, ..)| *entity);
        let related =
            world.get::<TalksTo>(*entity).map(|talks| talks.0);
        if related != agent {
            if let Some(agent) = agent {
                world.entity_mut(*entity).insert(TalksTo(agent));
            } else {
                world.entity_mut(*entity).remove::<TalksTo>();
            }
        }
        if watches.reopen
            || watches.chats.get(entity) != Some(reference)
        {
            watches.pages.remove(entity);
            open(world, reference.clone());
        }
    }
    watches.pages.retain(|entity, _| chats.contains_key(entity));
    watches.chats = chats;
    watches.reopen = false;
    world.insert_resource(watches);
}

/// The received transcript or turn for watched chat entities.
pub fn receive(world: &mut World, message: v0::ServerMessage) {
    let reference = match &message {
        v0::ServerMessage::Transcript(page) => &page.chat,
        v0::ServerMessage::Turn(turn) => &turn.chat,
        _ => return,
    };
    let chats = world
        .query::<(Entity, &Chat)>()
        .iter(world)
        .filter(|(_, chat)| &chat.0 == reference)
        .map(|(entity, _)| entity)
        .collect::<Vec<_>>();
    let chief = world.resource::<Chief>().0.clone();
    for chat in chats {
        match &message {
            v0::ServerMessage::Transcript(page) => {
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
            v0::ServerMessage::Turn(turn) => {
                if let Some(number) = turn.number {
                    world
                        .entity_mut(chat)
                        .insert(ConversationNumber(number));
                }
                for op in &turn.ops {
                    if matches!(op, v0::Op::Reset {})
                        && let Some(page) = world
                            .resource_mut::<Watches>()
                            .pages
                            .get_mut(&chat)
                    {
                        page.stale = true;
                    }
                    match op {
                        v0::Op::Status {
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
                        v0::Op::Usage { usage } => {
                            world
                                .entity_mut(chat)
                                .insert(Usage(usage.clone()));
                        }
                        op => world
                            .get_mut::<Transcript>(chat)
                            .unwrap()
                            .apply(op, &chief),
                    }
                    world.trigger(SessionOutput {
                        chat,
                        op: op.clone(),
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

fn received_item(item: v0::Item, chief: &str) -> Item {
    let entry = match item.kind {
        v0::ItemKind::User if item.author.is_some() => Entry::Agent {
            name: item.author.unwrap(),
            text: item.text,
        },
        v0::ItemKind::User => Entry::User {
            user: item.chief.unwrap_or_else(|| chief.into()),
            text: item.text,
        },
        v0::ItemKind::Assistant => Entry::Assistant(item.text),
        v0::ItemKind::Reasoning => Entry::Reasoning(item.text),
        v0::ItemKind::Notice => Entry::Notice(item.text),
        v0::ItemKind::Error => Entry::Error(item.text),
        v0::ItemKind::Answered => Entry::Answered(item.text),
        v0::ItemKind::Tool => {
            let tool = item.tool.unwrap_or(v0::ItemTool {
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
        v0::ClientMessage::Open(v0::Open {
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
#[require(Transcript, Status)]
pub struct Chat(pub v0::ChatRef);

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
    User {
        user: String,
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
        change: Option<v0::Change>,
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
            Entry::User { text: shown, .. }
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
    fn apply(&mut self, op: &v0::Op, chief: &str) {
        // An op on an item this window does not hold changes nothing.
        let _ = transcript::apply(&mut self.items, op, |item| {
            received_item(item.clone(), chief)
        });
        match op {
            v0::Op::Reset {} => {
                self.earlier = None;
                self.shape += 1;
            }
            v0::Op::Add { .. } | v0::Op::Close { .. } => {
                self.shape += 1
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
    pub fn answering(&self) -> bool {
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
    pub fn working(&self) -> bool {
        self.pending > 0
    }
}

/// The server's received usage view.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Usage(pub v0::UsageView);

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
