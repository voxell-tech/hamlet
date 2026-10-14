//! The way back in a conversation: the mark on one of the chief's
//! own messages that takes the conversation back to it, and the
//! message's words put in the field to be changed and sent again.

use bevy::app::App;
use bevy::camera::visibility::Visibility;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::lifecycle::Insert;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::math::Vec2;
use bevy::picking::hover::Hovered;
use bevy::ui::{JustifyContent, UiRect, px};
use bevy_fynix::ScopedExt;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, label,
};
use hamlet_core::session::{Rewound, Status};

use super::Draft;
use crate::feel::Mood;
use crate::icons::{Glyph, glyph};
use crate::looks::{ICON, retrace};
use crate::me::Me;
use crate::world_menu::{self, Deed};
use crate::{Cx, Ui};

pub(super) fn plugin(app: &mut App) {
    app.add_observer(reveal).add_observer(retype);
}

/// The room of a mark about its picture, and the room it takes
/// beside its message.
const AROUND: f32 = 4.0;
pub(super) const ROOM: f32 = ICON + AROUND * 3.0;

/// On a row of the chat: the message `entry` of the conversation
/// `session`, which that conversation can be taken back to.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rewinds {
    pub session: Entity,
    pub entry: u64,
}

/// The mark of a row, which takes the conversation back to it.
#[derive(Component)]
struct Mark;

/// The mark beside the message `entry` of `session`, out of sight
/// until the pointer is on its row. It asks before it acts.
pub(super) fn mark(session: Entity, entry: u64) -> Ui {
    Ui::new(move |cx: &mut Cx| {
        let back = button(glyph(Glyph::Restore, ICON))
            .padding(UiRect::all(px(AROUND)))
            .tooltip(|| label("Rewind to here").size(12.0))
            .rules(retrace)
            .tagged(Mood::Ink)
            .on_activate(move |world| {
                let deed = Deed::Rewind { session, entry };
                world_menu::ask(world, deed);
            });
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

/// The question asked before the conversation `session` is taken
/// back to its message `entry`, at `at` in the window.
pub fn asked(session: Entity, entry: u64, at: Vec2) -> Ui {
    world_menu::question(Deed::Rewind { session, entry }, at)
}

/// Shows the mark of a row while the pointer is on the row and no
/// turn runs in its conversation.
fn reveal(
    hover: On<Insert, Hovered>,
    rows: Query<(&Hovered, &Rewinds)>,
    sessions: Query<&Status>,
    under: Query<&Children>,
    mut marks: Query<&mut Visibility, With<Mark>>,
) {
    let row = hover.entity;
    let Ok((hovered, rewinds)) = rows.get(row) else {
        return;
    };
    let working =
        sessions.get(rewinds.session).is_ok_and(Status::working);
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
/// back to in the field, with the caret at their end.
fn retype(
    rewound: On<Rewound>,
    me: Res<Me>,
    mut draft: ResMut<Draft>,
) {
    if rewound.event().session != me.session {
        return;
    }
    draft.text = rewound.event().text.clone();
    // The field is focused two steps on. A send starts a step
    // earlier, at the one that clears it.
    draft.step = 2;
}
