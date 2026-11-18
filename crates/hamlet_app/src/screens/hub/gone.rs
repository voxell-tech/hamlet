//! The page of an agent that is gone, and of the library.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::AlignItems;
use bevy_fynix::views::{BehaviorExt, FrameProps, column, row};
use hamlet_client::realm::{LivesIn, MergedInto, Purpose};
use hamlet_realm::pick::{Thing, select};
use hamlet_ui::styles::{
    Fills, action, bad_action, breadcrumb, buttons, scroll_area,
    status_pill, tinted_action,
};
use hamlet_ui::theme::StatusKind;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::{View, built, with_cx};

use super::thread::dim;
use super::{Showing, look_at, show};
use crate::lists;
use crate::world_menu::{self, Action};

/// The page in `hub` of an agent that is gone, with the way back.
pub(super) fn ruin(hub: Entity, agent: Entity) -> impl View {
    built(move |cx| {
        let named = |agent: Entity| {
            hamlet_client::realm::shown_of(cx.world, agent)
        };
        let name = named(agent);
        let (fate, kind) = match cx.world.get::<MergedInto>(agent) {
            Some(host) => (
                format!("Merged into {}", named(host.0)),
                StatusKind::Merged,
            ),
            None => ("Removed".to_string(), StatusKind::Danger),
        };
        let purpose = cx
            .world
            .get::<Purpose>(agent)
            .map(|p| p.0.clone())
            .unwrap_or_default();
        // The path to it: the realm, then the hamlet it lived in.
        let hamlet =
            cx.world.get::<LivesIn>(agent).map(|lives| lives.0);
        let mut trail = vec!["Realm".to_string()];
        if let Some(hamlet) = hamlet {
            trail.push(named(hamlet));
        }
        let crumb =
            breadcrumb(trail, name.clone(), move |world, at| {
                match hamlet.filter(|_| at == 1) {
                    Some(hamlet) => {
                        show(world, hub, Showing::Hamlet(hamlet));
                        look_at(world, hub, Thing::Common(hamlet));
                    }
                    None => to_realm(world, hub),
                }
            });
        let section = cx.theme().space.section;
        // The path names it; its fate stands beside the path.
        let head = row((crumb, status_pill(Glyph::Gone, fate, kind)))
            .gap(section)
            .align(AlignItems::Center)
            .tagged(Wraps);
        cx.build(
            column((
                head,
                super::thread::said(purpose),
                dim("Its worktree is kept in the archive. Deleting \
                     it frees the space on disk."),
                buttons(vec![
                    tinted_action(
                        StatusKind::Success,
                        Glyph::Restore,
                        "Restore",
                        move |world| {
                            world_menu::restore(world, agent);
                        },
                    ),
                    bad_action(
                        Glyph::Delete,
                        "Delete",
                        move |world| {
                            world_menu::ask(
                                world,
                                Action::Erase(agent),
                            );
                        },
                    ),
                ]),
            ))
            .gap(section),
        )
    })
}

/// Turns `hub` to the realm's page, choosing nothing in the world
/// when it follows the selection.
fn to_realm(world: &mut World, hub: Entity) {
    if super::is_following(world, hub) {
        select(world, None);
    }
    show(world, hub, Showing::Realm);
}

/// The library's page in `hub`.
pub(super) fn library(hub: Entity) -> impl View {
    with_cx(move |cx| {
        column((
            breadcrumb(
                vec!["Realm".into()],
                "The library",
                move |world, _| to_realm(world, hub),
            ),
            // Wraps when the panel is narrow.
            buttons(vec![action(Glyph::Move, "Move", |world| {
                hamlet_realm::moving::pick_up(
                    world,
                    hamlet_realm::generator::society::Ground::Library,
                );
            })]),
            dim("Every agent shares these skills. An agent offers \
                 one and the librarian decides whether to keep it."),
            scroll_area((lists::skills(),)),
        ))
        .fills_height()
        .gap(cx.theme().space.section)
    })
}
