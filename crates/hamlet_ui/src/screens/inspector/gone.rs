//! The page of an agent that is gone, and of the library.

use bevy::ecs::entity::Entity;
use bevy::ui::AlignItems;
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, row, scroll,
};
use hamlet_core::agents::{LivesIn, MergedInto, Purpose};
use hamlet_realm::pick::{Selected, Thing, select};

use super::agent::dim;
use crate::icons::Glyph;
use crate::looks::{
    Fills, action, bad_action, good_action, stated, title,
};
use crate::platform::Wraps;
use crate::world_menu::{self, Deed};
use crate::{AnyView, Cx, View, lists};

/// The page of an agent that is gone, with the way back.
pub(super) fn ruin(agent: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let named = |agent: Entity| {
            hamlet_core::agents::name_of(cx.world, agent)
        };
        let name = named(agent);
        let fate = match cx.world.get::<MergedInto>(agent) {
            Some(host) => format!("Merged into {}", named(host.0)),
            None => "Removed".to_string(),
        };
        let purpose = cx
            .world
            .get::<Purpose>(agent)
            .map(|p| p.0.clone())
            .unwrap_or_default();
        cx.build(
            column((
                row((
                    title(name.to_uppercase()),
                    stated(Glyph::Gone, fate, Tone::Critical, 14.0),
                ))
                .gap(12.0)
                .align(AlignItems::Center)
                .tagged(Wraps),
                super::agent::said(purpose),
                dim("Its house is kept in the archive. Deleting it \
                     frees the space on disk."),
                row((
                    good_action(
                        Glyph::Restore,
                        "RESTORE",
                        move |world| {
                            world_menu::restore(world, agent)
                        },
                    ),
                    bad_action(
                        Glyph::Delete,
                        "DELETE FOR GOOD",
                        move |world| {
                            world_menu::ask(world, Deed::Erase(agent))
                        },
                    ),
                    action(
                        Glyph::Hamlet,
                        "ITS HAMLET",
                        move |world| {
                            let Some(lives) =
                                world.get::<LivesIn>(agent)
                            else {
                                return;
                            };
                            let hamlet = Thing::Common(lives.0);
                            world.resource_mut::<Selected>().0 =
                                Some(hamlet);
                        },
                    ),
                ))
                .gap(8.0)
                .tagged(Wraps),
            ))
            .gap(6.0),
        )
    })
}

/// The library, when it is selected in the world: what it is, and
/// its shelves.
pub(super) fn library() -> impl View {
    column((
        title("THE LIBRARY"),
        // On a line of their own, and on to another when the panel
        // is narrow.
        crate::looks::buttons(vec![
            crate::looks::back("ALL HAMLETS", |world| {
                select(world, None)
            })
            .boxed(),
            action(Glyph::Move, "MOVE", |world| {
                hamlet_realm::moving::pick_up(
                    world,
                    hamlet_realm::generator::society::Place::Library,
                )
            }),
        ]),
        dim("Every agent shares these skills. An agent offers one \
             and the librarian decides whether to keep it."),
        scroll((lists::skills(),)).fills_height(),
    ))
    .fills_height()
    .gap(10.0)
}
