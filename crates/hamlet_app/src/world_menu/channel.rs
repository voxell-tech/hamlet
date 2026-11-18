//! The form a channel is opened with: a search over the agents of the
//! agent's hamlet that it has no channel to yet, one pressed to open
//! it.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::ui::{AlignItems, JustifyContent, percent, px};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, popup,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::realm::{
    self as agents, Actions, Kind, Left, LivesIn, channel_between,
};
use hamlet_protocol::client as protocol;
use hamlet_realm::orders;
use hamlet_ui::styles::{
    self, choice, description, lettered, scroll_area,
};
use hamlet_ui::{View, built};

use super::close;

/// How tall the list grows before it scrolls.
const LIST: f32 = 240.0;

/// What is typed in the search.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct Channeling {
    search: String,
}

/// The agents `agent` could have a channel to whose shown names hold
/// `search`, by name: the living agents of its own hamlet it has no
/// channel to yet, as a chief opens no channel between hamlets.
pub(super) fn choices(
    world: &World,
    agent: Entity,
    search: &str,
) -> Vec<(Entity, String)> {
    let Some(home) = world.get::<LivesIn>(agent).map(|home| home.0)
    else {
        return Vec::new();
    };
    let Some(mut agents_found) =
        world.try_query::<(Entity, &Kind, &LivesIn, Option<&Left>)>()
    else {
        return Vec::new();
    };
    let search = search.trim().to_lowercase();
    let mut found = agents_found
        .iter(world)
        .filter(|(other, kind, lives, left)| {
            *other != agent
                && kind.0 == protocol::AgentKind::Hamlet
                && lives.0 == home
                && left.is_none()
        })
        .map(|(other, ..)| other)
        .filter(|other| {
            channel_between(world, agent, *other).is_none()
        })
        .map(|other| (other, agents::shown_of(world, other)))
        .filter(|(_, name)| name.to_lowercase().contains(&search))
        .collect::<Vec<_>>();
    found.sort_by(|a, b| a.1.cmp(&b.1));
    found
}

fn list(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            choices(
                world,
                agent,
                &world.resource::<Channeling>().search,
            )
        })
        .when(every_frame()),
        move |found| {
            if found.is_empty() {
                return description("No agent to open a channel to.")
                    .boxed();
            }
            column(
                found
                    .iter()
                    .cloned()
                    .map(|(other, name)| {
                        button(label(name).wrap(false))
                            .width(percent(100.0))
                            .justify(JustifyContent::FlexStart)
                            .align(AlignItems::Center)
                            .rules(choice(false))
                            .on_activate(move |world| {
                                close(world);
                                *world.resource_mut::<Channeling>() =
                                    Channeling::default();
                                orders::introduce(
                                    world, agent, other,
                                );
                            })
                            .boxed()
                    })
                    .collect::<Vec<_>>(),
            )
            .gap(2.0)
            .boxed()
        },
    )
    .within(column(()))
}

/// The form a channel from `agent` is opened with.
pub(super) fn channel(agent: Entity, at: Vec2) -> impl View {
    built(move |cx| {
        if !Actions::is_allowed(cx.world, "channel")
            || cx.world.get_entity(agent).is_err()
        {
            return cx.build(frame());
        }
        let from = agents::shown_of(cx.world, agent);
        let (gap, padding, heading) = {
            let theme = cx.theme();
            (
                theme.space.section,
                theme.space.overlay_padding,
                theme.text.heading,
            )
        };
        let body = column((
            label(format!("A channel from {}", from))
                .size(heading)
                .bold(true),
            crate::search::search(
                "Search agents",
                String::new(),
                |world, text| {
                    world.resource_mut::<Channeling>().search = text;
                },
            ),
            column((scroll_area((list(agent),)),))
                .max_height(px(LIST)),
            lettered("Cancel")
                .rules(styles::button)
                .on_activate(close),
        ))
        .gap(gap)
        .width(px(300.0));
        cx.build(
            popup(Rect::from_corners(at, at), (body,))
                .on_dismiss(|world, _| close(world))
                .padding(bevy::ui::UiRect::all(px(padding))),
        )
    })
}
