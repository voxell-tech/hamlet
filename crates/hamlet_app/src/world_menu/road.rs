//! The form a road is built with: a search over the hires of the
//! realm that the agent has no road to yet, one pressed to build it.

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
    self as agents, Deeds, Kind, Left, road_between,
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
pub(crate) struct Roading {
    search: String,
}

/// The hires `agent` could have a road to whose shown names hold
/// `search`, by name.
fn choices(
    world: &World,
    agent: Entity,
    search: &str,
) -> Vec<(Entity, String)> {
    let Some(mut hires) =
        world.try_query::<(Entity, &Kind, Option<&Left>)>()
    else {
        return Vec::new();
    };
    let search = search.trim().to_lowercase();
    let mut found = hires
        .iter(world)
        .filter(|(other, kind, left)| {
            *other != agent
                && kind.0 == protocol::AgentKind::Hire
                && left.is_none()
        })
        .map(|(other, ..)| other)
        .filter(|other| road_between(world, agent, *other).is_none())
        .map(|other| (other, agents::shown_of(world, other)))
        .filter(|(_, name)| name.to_lowercase().contains(&search))
        .collect::<Vec<_>>();
    found.sort_by(|a, b| a.1.cmp(&b.1));
    found
}

fn list(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            choices(world, agent, &world.resource::<Roading>().search)
        })
        .when(every_frame()),
        move |found| {
            if found.is_empty() {
                return description("No hire to build a road to.")
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
                                *world.resource_mut::<Roading>() =
                                    Roading::default();
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

/// The form a road from `agent` is built with.
pub(super) fn road(agent: Entity, at: Vec2) -> impl View {
    built(move |cx| {
        if !Deeds::is_allowed(cx.world, "road")
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
            label(format!("A road from {}", from))
                .size(heading)
                .bold(true),
            crate::search::search(
                "Search hires",
                String::new(),
                |world, text| {
                    world.resource_mut::<Roading>().search = text;
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
