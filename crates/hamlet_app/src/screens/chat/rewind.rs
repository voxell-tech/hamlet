//! The way back in a conversation: the mark on one of the chief's
//! own messages that takes the conversation back to it, and the
//! message's words put in the field to be changed and sent again.
//! A message that still waits its turn is taken back by a cross.

use bevy::app::App;
use bevy::camera::visibility::Visibility;
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::lifecycle::Insert;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::math::Rect;
use bevy::picking::hover::Hovered;
use bevy::ui::{
    ComputedNode, JustifyContent, UiGlobalTransform, UiRect, px,
};
use bevy_fynix::views::{BehaviorExt, FrameProps, button, column};
use bevy_fynix::{Signal, Visual, resource};
use hamlet_client::chats::Status;
use hamlet_client::messages::{Rewound, Withdraw};
use hamlet_client::outbox::RequestId;
use hamlet_ui::styles::{ICON, Looks, Tipped};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{Cx, View, built};

use super::attach::Attached;
use super::bubble::Bubble;
use super::field::set;
use super::reply::{Replying, unreplied};
use crate::convo::Convo;
use crate::world_menu::{self, Deed, Open};

/// The room of a mark about its picture, and the room it takes
/// beside its message.
const AROUND: f32 = 4.0;

pub(super) const ROOM: f32 = ICON + AROUND * 3.0;

pub(super) fn plugin(app: &mut App) {
    app.add_observer(reveal).add_observer(retype);
}

/// How opaque what a rewind forgets is while it is asked about.
const FORGOTTEN: f32 = 0.4;

/// A rewindable chat row, ordered by its message time.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
pub struct Rewinds {
    pub chat: Entity,
    /// The row's message.
    pub entry: String,
    /// The time of the row's message.
    pub at: u64,
}

/// The mark of a row, which takes the conversation back to it.
#[derive(Component)]
struct Mark;

/// The mark beside the message `entry` of `chat`, out of sight
/// until the pointer is on its row. It asks before it acts.
pub(super) fn mark(chat: Entity, entry: String) -> impl View {
    built(move |cx| {
        let back = button(glyph(Glyph::Restore, ICON))
            .padding(UiRect::all(px(AROUND)))
            .tip("Rewind to here")
            .variant(Variant::Text)
            .on_activate(move |world| ask(world, chat, &entry));
        let node = cx.build(
            column((back,))
                .justify(JustifyContent::Center)
                .padding(UiRect::right(px(AROUND)))
                .shrink(0.0)
                .tagged(Mark),
        );
        cx.world.entity_mut(node).insert(Visibility::Hidden);
        node
    })
}

/// The cross beside the chief's line `entry` of `chat` that waits
/// its turn, which takes it back.
pub(super) fn withdraw(chat: Entity, entry: String) -> impl View {
    let back = button(glyph(Glyph::Cross, ICON))
        .padding(UiRect::all(px(AROUND)))
        .tip("Take back")
        .variant(Variant::Text)
        .on_activate(move |world| {
            world.trigger(Withdraw {
                chat,
                request: RequestId::new(),
                item: entry.clone(),
            });
        });
    column((back,))
        .justify(JustifyContent::Center)
        .padding(UiRect::right(px(AROUND)))
        .shrink(0.0)
}

/// Asks before the conversation `chat` is taken back to its
/// message `entry`, against that message's bubble.
pub fn ask(world: &mut World, chat: Entity, entry: &str) {
    let mut rows = world.query::<(Entity, &Rewinds)>();
    let Some((row, at)) = rows
        .iter(world)
        .find(|(_, row)| row.chat == chat && row.entry == entry)
        .map(|(row, rewinds)| (row, rewinds.at))
    else {
        return;
    };
    let mut bubbles = world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<Bubble>>();
    let mut children = world.query::<&Children>();
    let anchor = children
        .query(world)
        .iter_descendants(row)
        .find_map(|node| bubbles.get(world, node).ok())
        .map(|(node, place)| {
            let scale = node.inverse_scale_factor();
            Rect::from_center_size(
                place.translation * scale,
                node.size() * scale,
            )
        })
        .unwrap_or_default();
    let deed = Deed::Rewind {
        chat,
        entry: entry.to_string(),
        at,
    };
    world_menu::ask_against(world, deed, anchor);
}

/// The message of `chat` a rewind is asked about, and when it was
/// said.
fn asked(open: &Open, chat: Entity) -> Option<(&str, u64)> {
    match open {
        Open::Question(
            Deed::Rewind {
                chat: of,
                entry,
                at,
            },
            _,
        ) if *of == chat => Some((entry.as_str(), *at)),
        _ => None,
    }
}

/// Rules that fade a row of `chat` said at `at` while a rewind
/// that would forget it is asked about. The message asked about is
/// kept clear, as what the question is against.
pub(super) fn fading(chat: Entity, at: u64) -> impl FnOnce(&mut Cx) {
    move |cx| {
        cx.set::<Visual>(move |visual, _| {
            visual.opacity(resource::<Open, _>(
                move |open| match asked(open, chat) {
                    Some((_, from)) if at > from => FORGOTTEN,
                    _ => 1.0,
                },
            ))
        });
    }
}

/// Rules that hide the marks of the message `entry` of `chat`
/// while a rewind to it is asked about.
pub(super) fn hiding(
    chat: Entity,
    entry: String,
) -> impl FnOnce(&mut Cx) {
    move |cx| {
        cx.set::<Visual>(move |visual, _| {
            let entry = entry.clone();
            visual.opacity(resource::<Open, _>(
                move |open| match asked(open, chat) {
                    Some((of, _)) if of == entry => 0.0,
                    _ => 1.0,
                },
            ))
        });
    }
}

/// `ring` while a rewind to the message `entry` of `chat` is asked
/// about, else `rest`.
pub(super) fn ringed(
    chat: Entity,
    entry: String,
    rest: Color,
    ring: Color,
) -> Signal<Color> {
    resource::<Open, _>(move |open| match asked(open, chat) {
        Some((of, _)) if of == entry => ring,
        _ => rest,
    })
}

/// Shows the mark of a row while the pointer is on the row and no
/// turn runs in its conversation.
fn reveal(
    hover: On<Insert<Hovered>>,
    rows: Query<(&Hovered, &Rewinds)>,
    chats: Query<&Status>,
    under: Query<&Children>,
    mut marks: Query<&mut Visibility, With<Mark>>,
) {
    let row = hover.entity;
    let Ok((hovered, rewinds)) = rows.get(row) else {
        return;
    };
    let working =
        chats.get(rewinds.chat).is_ok_and(Status::is_working);
    let want = match hovered.get() && !working {
        true => Visibility::Inherited,
        false => Visibility::Hidden,
    };
    for node in under.iter_descendants(row) {
        if let Ok(mut shown) = marks.get_mut(node)
            && *shown != want
        {
            *shown = want;
        }
    }
}

/// Puts the words of the message the open conversation was taken
/// back to in the field, with the caret at their end, and what it
/// replied to over the field.
fn retype(
    rewound: On<Rewound>,
    convo: Res<Convo>,
    mut replying: ResMut<Replying>,
    mut attached: ResMut<Attached>,
    mut commands: Commands,
) {
    if rewound.event().chat != convo.chat {
        return;
    }
    let text = &rewound.event().text;
    let (answers, words) = match unreplied(text) {
        Some((answers, words)) => {
            (Some((convo.chat, answers)), words)
        }
        None => (None, text.as_str()),
    };
    replying.0 = answers;
    let pieces = attached.give_back(
        &rewound.event().images,
        &rewound.event().files,
        words,
        text.len() - words.len(),
    );
    commands.queue(move |world: &mut World| set(world, pieces));
}
