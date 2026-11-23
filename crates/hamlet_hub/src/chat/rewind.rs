//! The mark on the chief's message that rewinds the conversation to
//! it.

use std::collections::HashMap;

use bevy::app::App;
use bevy::camera::visibility::Visibility;
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::lifecycle::Insert;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ecs::world::World;
use bevy::math::Rect;
use bevy::picking::hover::Hovered;
use bevy::ui::{
    ComputedNode, JustifyContent, UiGlobalTransform, UiRect, px,
};
use hamlet_core::client::chats::Status;
use hamlet_core::client::messages::{
    Response, Rewind, Rewound, Withdraw,
};
use hamlet_core::client::outbox::RequestId;
use hamlet_core::fynix::views::{
    BehaviorExt, FrameProps, Piece, button, column,
};
use hamlet_core::fynix::{Signal, Visual, resource};
use hamlet_core::ui::styles::{ICON, Looks, Tipped};
use hamlet_core::ui::widgets::effects::Variant;
use hamlet_core::ui::widgets::icons::{Glyph, glyph};
use hamlet_core::ui::{Cx, View, built};

use super::attach::{Attached, Owner};
use super::bubble::Bubble;
use super::field::{draft_of, give_draft};
use super::reply::{
    AROUND, replies_in, reply_in, show_marks, unreplied,
};
use crate::convo::Convo;
use crate::world_menu::{self, Action, Open};

pub(super) fn plugin(app: &mut App) {
    app.add_observer(reveal)
        .add_observer(retype)
        .add_observer(answered);
}

/// How opaque what a rewind forgets is while it is asked about.
const FORGOTTEN: f32 = 0.4;

/// A rewindable chat row of a Hub, ordered by its message time.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
pub struct Rewinds {
    pub hub: Entity,
    pub chat: Entity,
    pub entry: String,
    pub at: u64,
}

/// The Hub each rewind under way was asked in, by its request, which
/// its words go back to.
#[derive(Resource, Default)]
struct Rewinding(HashMap<RequestId, Entity>);

/// Forgets a rewind once the server has answered it, taken back or
/// refused.
fn answered(
    response: On<Response>,
    rewinding: Option<ResMut<Rewinding>>,
) {
    if let Some(mut rewinding) = rewinding {
        rewinding.0.remove(&response.to);
    }
}

/// The mark of a row, which takes the conversation back to it.
#[derive(Component)]
struct Mark;

/// The mark beside message `entry` of `chat` in `hub`, shown on
/// hover.
pub(super) fn mark(
    hub: Entity,
    chat: Entity,
    entry: String,
) -> impl View {
    built(move |cx| {
        let back = button(glyph(Glyph::Restore, ICON))
            .padding(UiRect::all(px(AROUND)))
            .tip("Rewind to here")
            .variant(Variant::Text)
            .on_activate(move |world| ask(world, hub, chat, &entry));
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

/// The cross that withdraws the queued line `entry` of `chat`.
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

/// Asks before `chat` is rewound to its message `entry`, from `hub`.
pub fn ask(
    world: &mut World,
    hub: Entity,
    chat: Entity,
    entry: &str,
) {
    let mut rows = world.query::<(Entity, &Rewinds)>();
    let Some((row, at)) = rows
        .iter(world)
        .find(|(_, row)| {
            row.hub == hub && row.chat == chat && row.entry == entry
        })
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
    let action = Action::Rewind {
        hub,
        chat,
        entry: entry.to_string(),
        at,
    };
    world_menu::ask_against(world, action, anchor);
}

/// Rewinds `chat` to before its message `entry`, its words to go
/// back to `hub`.
pub fn rewind_to(
    world: &mut World,
    hub: Entity,
    chat: Entity,
    entry: &str,
) {
    let request = RequestId::new();
    world
        .get_resource_or_init::<Rewinding>()
        .0
        .insert(request.clone(), hub);
    world.trigger(Rewind {
        chat,
        request: request.clone(),
        item: entry.to_string(),
    });
    world.flush();
    super::asked_in(world, request, hub);
}

/// The message of `chat` a rewind is asked about in `hub`, and its
/// time.
fn asked(
    open: &Open,
    hub: Entity,
    chat: Entity,
) -> Option<(&str, u64)> {
    match open {
        Open::Question(
            Action::Rewind {
                hub: from,
                chat: of,
                entry,
                at,
            },
            _,
        ) if *from == hub && *of == chat => {
            Some((entry.as_str(), *at))
        }
        _ => None,
    }
}

/// Fades a row of `hub` said at `at` while a rewind past it is asked
/// about there.
pub(super) fn fading(
    hub: Entity,
    chat: Entity,
    at: u64,
) -> impl FnOnce(&mut Cx) {
    move |cx| {
        cx.set::<Visual>(move |visual, _| {
            visual.opacity(resource::<Open, _>(
                move |open| match asked(open, hub, chat) {
                    Some((_, from)) if at > from => FORGOTTEN,
                    _ => 1.0,
                },
            ))
        });
    }
}

/// Hides `entry`'s marks in `hub` while a rewind to it is asked about
/// there.
pub(super) fn hiding(
    hub: Entity,
    chat: Entity,
    entry: String,
) -> impl FnOnce(&mut Cx) {
    move |cx| {
        cx.set::<Visual>(move |visual, _| {
            let entry = entry.clone();
            visual.opacity(resource::<Open, _>(
                move |open| match asked(open, hub, chat) {
                    Some((of, _)) if of == entry => 0.0,
                    _ => 1.0,
                },
            ))
        });
    }
}

/// `ring` while a rewind to `entry` is asked about in `hub`, else
/// `rest`.
pub(super) fn ringed(
    hub: Entity,
    chat: Entity,
    entry: String,
    rest: Color,
    ring: Color,
) -> Signal<Color> {
    resource::<Open, _>(move |open| match asked(open, hub, chat) {
        Some((of, _)) if of == entry => ring,
        _ => rest,
    })
}

/// Shows a row's mark on hover while no turn runs.
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
    show_marks(row, hovered.get() && !working, &under, &mut marks);
}

/// Puts the rewound message back as the draft of its chat in the Hub
/// the rewind was asked in, shown in its field or not. Without that
/// Hub, a Hub showing the chat takes it, the one the chief works in
/// first.
fn retype(
    rewound: On<Rewound>,
    rewinding: Option<ResMut<Rewinding>>,
    mut commands: Commands,
) {
    let rewound = rewound.event().clone();
    // Read now: the answer that follows forgets the rewind.
    let asked = rewinding
        .and_then(|mut asked| asked.0.remove(&rewound.request));
    commands.queue(move |world: &mut World| {
        let chat = rewound.chat;
        let asked =
            asked.filter(|hub| world.get::<Convo>(*hub).is_some());
        // With the asking Hub closed, a Hub showing the chat takes
        // the words, else the one the chief works in, as the chat's
        // draft there.
        let Some(hub) = asked
            .or_else(|| showing(world, chat))
            .or_else(|| crate::hub::active(world))
        else {
            return;
        };
        let text = &rewound.text;
        let (answers, words) = match unreplied(text) {
            Some((answers, words)) => (Some(answers), words),
            None => (None, text.as_str()),
        };
        // Kept with the chat's draft, shown once the Hub is back on
        // it. A Hub that did not ask keeps a reply of its own.
        if asked.is_some() || !replies_in(world, hub, chat) {
            reply_in(world, hub, chat, answers);
        }
        let pieces = world.resource_mut::<Attached>().give_back(
            Owner { hub, chat },
            &rewound.images,
            &rewound.files,
            words,
            text.len() - words.len(),
        );
        // A Hub that did not ask keeps what it has typed, with the
        // returned words after it.
        let pieces = match asked {
            Some(_) => pieces,
            None => {
                let mut kept = draft_of(world, hub, chat);
                if !kept.is_empty() && !pieces.is_empty() {
                    kept.push(Piece::Text("\n".into()));
                }
                kept.extend(pieces);
                kept
            }
        };
        give_draft(world, hub, chat, pieces);
    });
}

/// A Hub that shows `chat`, the one the chief works in first.
fn showing(world: &mut World, chat: Entity) -> Option<Entity> {
    let active = crate::hub::active(world);
    let mut hubs = world.query::<(Entity, &Convo)>();
    let showing = hubs
        .iter(world)
        .filter(|(_, convo)| convo.chat == chat)
        .map(|(hub, _)| hub)
        .collect::<Vec<_>>();
    active
        .filter(|hub| showing.contains(hub))
        .or_else(|| showing.first().copied())
}
