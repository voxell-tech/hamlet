//! The page of an agent that is gone, and of the library.

use bevy::ecs::entity::Entity;
use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{BehaviorExt, FrameProps, column, row};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_client::realm::{LivesIn, MergedInto, Purpose};
use hamlet_realm::pick::{Thing, select};
use hamlet_ui::styles::{
    Fills, action, bad_action, breadcrumb, buttons, filling,
    good_action, scroll_area, section, status_label_large, title,
};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::{AnyView, View, built, with_cx};

use super::thread::dim;
use super::{Showing, show};
use crate::convo::Convo;
use crate::lists;
use crate::world_menu::{self, Deed};

/// The page of an agent that is gone, with the way back.
pub(super) fn ruin(agent: Entity) -> impl View {
    built(move |cx| {
        let named = |agent: Entity| {
            hamlet_client::realm::shown_of(cx.world, agent)
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
        // The path to it: the realm, then the hamlet it lived in.
        let hamlet =
            cx.world.get::<LivesIn>(agent).map(|lives| lives.0);
        let mut trail = vec!["REALM".to_string()];
        if let Some(hamlet) = hamlet {
            trail.push(named(hamlet).to_uppercase());
        }
        let crumb = breadcrumb(
            trail,
            name.to_uppercase(),
            move |world, at| {
                let place = hamlet.filter(|_| at == 1);
                select(world, place.map(Thing::Common));
                if place.is_none() {
                    show(world, Showing::Realm);
                }
            },
        );
        let (section, gap) =
            (cx.theme().space.section, cx.theme().space.gap);
        cx.build(
            column((
                crumb,
                row((
                    title(name.to_uppercase()),
                    status_label_large(
                        Glyph::Gone,
                        fate,
                        Tone::Critical,
                    ),
                ))
                .gap(section)
                .align(AlignItems::Center)
                .tagged(Wraps),
                super::thread::said(purpose),
                dim("Its lodge is kept in the archive. Deleting it \
                     frees the space on disk."),
                buttons(vec![
                    good_action(
                        Glyph::Restore,
                        "RESTORE",
                        move |world| {
                            world_menu::restore(world, agent);
                        },
                    ),
                    bad_action(
                        Glyph::Delete,
                        "DELETE FOR GOOD",
                        move |world| {
                            world_menu::ask(
                                world,
                                Deed::Erase(agent),
                            );
                        },
                    ),
                ]),
            ))
            .gap(gap),
        )
    })
}

/// The library, when it is selected in the world: what it is, and
/// its shelves.
pub(super) fn library() -> impl View {
    with_cx(|cx| {
        column((
            breadcrumb(
                vec!["REALM".into()],
                "THE LIBRARY",
                |world, _| {
                    select(world, None);
                    show(world, Showing::Realm);
                },
            ),
            title("THE LIBRARY"),
            // On a line of their own, and on to another when the
            // panel is narrow.
            buttons(vec![action(Glyph::Move, "MOVE", |world| {
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

/// A page of `what` about the selected agent, under whose it is.
pub(crate) fn of_agent(
    what: &'static str,
    body: fn() -> AnyView,
) -> impl View {
    keyed(
        resource::<Convo, _>(|m| (m.session, m.with.clone())),
        move |(_, with)| {
            let whose = match with {
                Some(name) => {
                    format!("{}: {what}", name.to_uppercase())
                }
                None => format!("THE HAND: {what}"),
            };
            let body = body();
            with_cx(move |cx| {
                column((section(whose), body))
                    .fills_height()
                    .gap(cx.theme().space.section)
            })
            .boxed()
        },
    )
    .within(filling())
}
