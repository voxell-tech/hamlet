//! The menu of each kind of thing in the world: the items that were
//! contributed, and the lines that depend on what else is chosen.

use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::ui::AlignItems;
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, label, menu_item, row,
};
use hamlet_agents_api::agents::{self, Road};
use hamlet_agents_api::hamlet::offers;
use hamlet_chat_api::orders::part;
use hamlet_realm_api::orders;
use hamlet_realm_api::pick::{Also, Selected, Thing};
use hamlet_sdk::command::{RunCommand, Subject};
use hamlet_sdk::context::{ContextItems, ContextWhen, Line};

use super::acts::plain_agent;
use super::deed::Deed;
use super::{Open, close, subject};
use crate::AnyView;
use crate::icons::glyph;
use crate::looks::ICON;

/// The sections of the lines that depend on the other agent that is
/// chosen, among those of the window's own items.
const ROAD: u32 = 60;
const MERGE: u32 = 80;
const PEERS: u32 = 90;

/// An agent of the hamlet that is not its mayor and has not left,
/// with its name.
fn resident(world: &World, thing: Thing) -> Option<(Entity, String)> {
    let (Thing::Resident(agent) | Thing::ResidentHouse(agent)) =
        thing
    else {
        return None;
    };
    let name = world.get::<Name>(agent)?.as_str().to_string();
    plain_agent(world, agent).then_some((agent, name))
}

fn item(
    text: String,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    menu_item(label(text).wrap(false))
        .on_activate(move |world| {
            close(world);
            act(world);
        })
        .boxed()
}

/// An item that asks before it does `deed`.
fn asking(text: String, deed: Deed, at: Vec2) -> AnyView {
    menu_item(label(text).wrap(false))
        .on_activate(move |world| {
            *world.resource_mut::<Open>() =
                Open::Question(deed.clone(), at);
        })
        .boxed()
}

/// An item that runs the command of `line` on `subject`, and closes
/// the menu unless the command opened another.
fn contributed(line: &Line, subject: &Subject) -> AnyView {
    let (command, subject) = (line.command.clone(), subject.clone());
    let words = label(line.said(&subject)).wrap(false);
    let content = match line.icon {
        Some(which) => {
            row((glyph(which, ICON).tone(Tone::Faint), words))
                .gap(10.0)
                .align(AlignItems::Center)
                .boxed()
        }
        None => words.boxed(),
    };
    menu_item(content)
        .on_activate(move |world| {
            world.trigger(RunCommand::on(
                command.clone(),
                subject.clone(),
            ));
            world.flush();
            if matches!(
                *world.resource::<Open>(),
                Open::Menu(..) | Open::Ground(..)
            ) {
                close(world);
            }
        })
        .boxed()
}

/// The agents `agent` has a road to, the mayor aside, by name.
fn joined(world: &World, agent: Entity) -> Vec<(Entity, String)> {
    let Some(mut roads) = world.try_query::<&Road>() else {
        return Vec::new();
    };
    let mut peers = roads
        .iter(world)
        .filter(|road| road.has(agent))
        .map(|road| match road.ends {
            (a, b) if a == agent => b,
            (a, _) => a,
        })
        .filter(|peer| world.get::<agents::Mayor>(*peer).is_none())
        .map(|peer| (peer, agents::name_of(world, peer)))
        .collect::<Vec<_>>();
    peers.sort_by(|a, b| a.1.cmp(&b.1));
    peers
}

/// The lines of an agent that depend on the other agent that is
/// chosen, and on its roads.
fn between(
    world: &World,
    thing: Thing,
    at: Vec2,
) -> Vec<(u32, AnyView)> {
    let Some((agent, me)) = resident(world, thing) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    // The other of the two that are chosen, if there are two.
    let chosen =
        [world.resource::<Selected>().0, world.resource::<Also>().0];
    let other = chosen
        .into_iter()
        .flatten()
        .filter_map(|thing| resident(world, thing))
        .find(|(other, _)| *other != agent);
    let chosen = other.as_ref().map(|(them, _)| *them);
    if let Some((them, other)) = other.clone() {
        let joined =
            world.try_query::<&Road>().is_some_and(|mut roads| {
                roads
                    .iter(world)
                    .any(|road| road.has(agent) && road.has(them))
            });
        lines.push(if joined {
            (
                ROAD,
                item(
                    format!("Remove the road to {other}"),
                    move |w| part(w, agent, them),
                ),
            )
        } else {
            (
                ROAD,
                item(format!("Build a road to {other}"), move |w| {
                    orders::introduce(w, agent, them)
                }),
            )
        });
    }
    if let Some((them, other)) = other {
        let into_me = Deed::Merge {
            absorbed: them,
            kept: agent,
        };
        let into_other = Deed::Merge {
            absorbed: agent,
            kept: them,
        };
        lines.push((
            MERGE,
            asking(
                format!("Merge {other} into {me}..."),
                into_me,
                at,
            ),
        ));
        lines.push((
            MERGE,
            asking(
                format!("Merge {me} into {other}..."),
                into_other,
                at,
            ),
        ));
    }
    // The roads it has, besides the one to the other of two that
    // are chosen, which is offered above.
    for (peer, name) in joined(world, agent) {
        if Some(peer) != chosen {
            lines.push((
                PEERS,
                item(
                    format!("Remove the road to {name}"),
                    move |w| part(w, agent, peer),
                ),
            ));
        }
    }
    lines
}

/// The items for `subject` that may show, and `lines`, in the order
/// of their sections.
fn listed(
    world: &World,
    subject: &Subject,
    mut lines: Vec<(u32, AnyView)>,
) -> Vec<AnyView> {
    let items = world.resource::<ContextItems>();
    lines.extend(
        items
            .of(&subject.kind)
            .filter(|line| {
                world
                    .get::<ContextWhen>(line.item)
                    .is_none_or(|when| (when.0)(world, subject))
            })
            .map(|line| (line.section, contributed(line, subject))),
    );
    lines.sort_by_key(|(section, _)| *section);
    lines.into_iter().map(|(_, view)| view).collect()
}

/// The skills `agent` could be given, each pressed to give it.
pub(super) fn skills(world: &World, agent: Entity) -> Vec<AnyView> {
    offers(world, agent)
        .into_iter()
        .map(|offer| {
            let text = match &offer.from {
                Some(from) => format!("{} (of {from})", offer.skill),
                None => format!("{} (starter)", offer.skill),
            };
            item(text, move |w| {
                orders::give(
                    w,
                    agent,
                    &offer.skill,
                    offer.from.as_deref(),
                )
            })
        })
        .collect()
}

/// The menu of `thing`.
pub(super) fn items(
    world: &mut World,
    thing: Thing,
    at: Vec2,
) -> Vec<AnyView> {
    let subject = subject::of(world, thing);
    let lines = between(world, thing, at);
    listed(world, &subject, lines)
}

/// The menu of the bare ground.
pub(super) fn ground(world: &World) -> Vec<AnyView> {
    listed(world, &subject::ground(), Vec::new())
}
