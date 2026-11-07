//! The way back in a conversation: the mark on one of the chief's
//! own messages that takes the conversation back to it, and the
//! message's words put in the field to be changed and sent again.

use bevy::app::App;
use bevy::camera::visibility::Visibility;
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::lifecycle::Insert;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::math::Rect;
use bevy::picking::hover::Hovered;
use bevy::ui::{
    ComputedNode, JustifyContent, UiGlobalTransform, UiRect, px,
};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column,
};
use bevy_fynix::{ScopedExt, Signal, Visual, resource};
use hamlet_client::chats::{Chat, Status};
use hamlet_client::messages::Rewound;
use hamlet_ui::styles::{ICON, back_link, note};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{Cx, View, built};

use super::Draft;
use super::bubble::Bubble;
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
    pub session: Entity,
    /// The row's message.
    pub entry: String,
    /// The time of the row's message.
    pub at: u64,
}

/// The mark of a row, which takes the conversation back to it.
#[derive(Component)]
struct Mark;

/// The mark beside the message `entry` of `session`, out of sight
/// until the pointer is on its row. It asks before it acts.
pub(super) fn mark(session: Entity, entry: String) -> impl View {
    built(move |cx| {
        let back = button(glyph(Glyph::Restore, ICON))
            .padding(UiRect::all(px(AROUND)))
            .tooltip(|| note("Rewind to here"))
            .rules(back_link)
            .tagged(Variant::Text)
            .on_activate(move |world| ask(world, session, &entry));
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

/// Asks before the conversation `session` is taken back to its
/// message `entry`, against that message's bubble.
pub fn ask(world: &mut World, session: Entity, entry: &str) {
    let mut rows = world.query::<(Entity, &Rewinds)>();
    let Some((row, at)) = rows
        .iter(world)
        .find(|(_, row)| row.session == session && row.entry == entry)
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
        session,
        entry: entry.to_string(),
        at,
    };
    world_menu::ask_against(world, deed, anchor);
}

/// The message of `session` a rewind is asked about, and when it was
/// said.
fn asked(open: &Open, session: Entity) -> Option<(&str, u64)> {
    match open {
        Open::Question(
            Deed::Rewind {
                session: of,
                entry,
                at,
            },
            _,
        ) if *of == session => Some((entry.as_str(), *at)),
        _ => None,
    }
}

/// Rules that fade a row of `session` said at `at` while a rewind
/// that would forget it is asked about. The message asked about is
/// kept clear, as what the question is against.
pub(super) fn fading(
    session: Entity,
    at: u64,
) -> impl FnOnce(&mut Cx) {
    move |cx| {
        cx.set::<Visual>(move |visual, _| {
            visual.opacity(resource::<Open, _>(
                move |open| match asked(open, session) {
                    Some((_, from)) if at > from => FORGOTTEN,
                    _ => 1.0,
                },
            ))
        });
    }
}

/// Rules that hide the marks of the message `entry` of `session`
/// while a rewind to it is asked about.
pub(super) fn hiding(
    session: Entity,
    entry: String,
) -> impl FnOnce(&mut Cx) {
    move |cx| {
        cx.set::<Visual>(move |visual, _| {
            let entry = entry.clone();
            visual.opacity(resource::<Open, _>(
                move |open| match asked(open, session) {
                    Some((of, _)) if of == entry => 0.0,
                    _ => 1.0,
                },
            ))
        });
    }
}

/// `ring` while a rewind to the message `entry` of `session` is asked
/// about, else `rest`.
pub(super) fn ringed(
    session: Entity,
    entry: String,
    rest: Color,
    ring: Color,
) -> Signal<Color> {
    resource::<Open, _>(move |open| match asked(open, session) {
        Some((of, _)) if of == entry => ring,
        _ => rest,
    })
}

/// Shows the mark of a row while the pointer is on the row and no
/// turn runs in its conversation.
fn reveal(
    hover: On<Insert<Hovered>>,
    rows: Query<(&Hovered, &Rewinds)>,
    sessions: Query<(&Status, Option<&Chat>)>,
    under: Query<&Children>,
    mut marks: Query<&mut Visibility, With<Mark>>,
) {
    let row = hover.entity;
    let Ok((hovered, rewinds)) = rows.get(row) else {
        return;
    };
    let working =
        sessions.get(rewinds.session).is_ok_and(|(status, chat)| {
            status.is_working()
                || chat.is_some_and(|chat| chat.0.past.is_some())
        });
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
    mut draft: ResMut<Draft>,
    mut replying: ResMut<Replying>,
) {
    if rewound.event().chat != convo.session {
        return;
    }
    let text = &rewound.event().text;
    let (answers, words) = match unreplied(text) {
        Some((answers, words)) => {
            (Some((convo.session, answers)), words)
        }
        None => (None, text.as_str()),
    };
    replying.0 = answers;
    draft.text = words.to_string();
    // The field is focused two steps on. A send starts a step
    // earlier, at the one that clears it.
    draft.step = 2;
}
