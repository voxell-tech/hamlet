//! The page of an agent that is gone, and of the library.

use bevy::ecs::entity::Entity;
use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{BehaviorExt, FrameProps, column, row};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_client::realm::{LivesIn, MergedInto, Purpose};
use hamlet_realm::pick::{Selected, Thing, select};

use super::agent::dim;
use super::papers::Papers;
use crate::convo::Convo;
use crate::icons::Glyph;
use crate::looks::{
    Fills, action, bad_action, bare, buttons, filling, good_action,
    group, note, scroll_area, section, stated_large, title,
};
use crate::platform::Wraps;
use crate::world_menu::{self, Deed};
use crate::{AnyView, View, built, lists, overview, with_cx};

/// The page of an agent that is gone, with the way back.
pub(super) fn ruin(agent: Entity) -> impl View {
    built(move |cx| {
        let named = |agent: Entity| {
            hamlet_client::realm::name_of(cx.world, agent)
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
        let (section, gap) =
            (cx.theme().space.section, cx.theme().space.gap);
        cx.build(
            column((
                row((
                    title(name.to_uppercase()),
                    stated_large(Glyph::Gone, fate, Tone::Critical),
                ))
                .gap(section)
                .align(AlignItems::Center)
                .tagged(Wraps),
                super::agent::said(purpose),
                dim("Its house is kept in the archive. Deleting it \
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
            title("THE LIBRARY"),
            // On a line of their own, and on to another when the
            // panel is narrow.
            buttons(vec![
                crate::looks::back("ALL HAMLETS", |world| {
                    select(world, None);
                })
                .boxed(),
                action(Glyph::Move, "MOVE", |world| {
                    hamlet_realm::moving::pick_up(
                        world,
                        hamlet_realm::generator::society::Ground::Library,
                    );
                }),
            ]),
            dim("Every agent shares these skills. An agent offers \
                 one and the librarian decides whether to keep it."),
            scroll_area((lists::skills(),)),
        ))
        .fills_height()
        .gap(cx.theme().space.section)
    })
}

/// The skills the selected agent wrote and keeps in its own house,
/// over those of the library, which every agent has.
fn skills() -> AnyView {
    let own = keyed(
        resource::<Papers, _>(|p| p.skills.clone()),
        |skills| {
            if skills.is_empty() {
                return note("None of its own yet.").boxed();
            }
            let rows = skills
                .iter()
                .map(|(name, does)| {
                    lists::item(
                        name.clone(),
                        does.clone(),
                        Tone::Body,
                    )
                })
                .collect::<Vec<_>>();
            group(rows).boxed()
        },
    )
    .within(bare());
    // One scroll for both lists, so that a long one of its own
    // does not push the library out of reach.
    with_cx(move |cx| {
        scroll_area((column((
            own,
            overview::heading(
                Glyph::Book,
                "THE LIBRARY, SHARED BY ALL",
            ),
            lists::skills(),
        ))
        .gap(cx.theme().space.section),))
    })
    .boxed()
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
                None => format!("THE MAYOR: {what}"),
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

/// The Skills panel: the selected agent's own skills, and the
/// library's.
pub(crate) fn skills_screen() -> AnyView {
    of_agent("ITS OWN SKILLS", skills).boxed()
}
