//! The menu of each kind of thing in the world.

use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy_fynix::ViewExt;
use bevy_fynix::views::{BehaviorExt, label, menu_item};
use hamlet_core::agents::{
    self, Busy, Left, Mayor, Road, Settlement,
};
use hamlet_rules::hamlet::HOME;
use hamlet_world::generator::society::Place as Ground;
use hamlet_world::moving;
use hamlet_world::pick::{Also, Selected, Thing, focus_on, select};

use super::deed::Deed;
use super::{Open, close, restore, stop};
use crate::inspector::show_prompt;
use crate::me::{talk_to, talk_to_mayor};
use crate::{AnyView, chat, hand};

/// An agent of the hamlet that is not its mayor and has not left,
/// with its name.
fn resident(world: &World, thing: Thing) -> Option<(Entity, String)> {
    let (Thing::Resident(agent) | Thing::ResidentHouse(agent)) =
        thing
    else {
        return None;
    };
    let plain = world.get::<Mayor>(agent).is_none()
        && world.get::<Left>(agent).is_none();
    let name = world.get::<Name>(agent)?.as_str().to_string();
    plain.then_some((agent, name))
}

pub(super) fn item(
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
        .filter(|peer| world.get::<Mayor>(*peer).is_none())
        .map(|peer| (peer, agents::name_of(world, peer)))
        .collect::<Vec<_>>();
    peers.sort_by(|a, b| a.1.cmp(&b.1));
    peers
}

fn focus(thing: Thing) -> AnyView {
    item("Focus".into(), move |w| focus_on(w, thing, false))
}

fn for_mayor() -> Vec<AnyView> {
    vec![
        item("Talk to the Mayor".into(), talk_to_mayor),
        item("Prompt".into(), |world| {
            talk_to_mayor(world);
            show_prompt(world);
        }),
        item("Stop the Mayor".into(), |world| {
            if let Some(mayor) = agents::mayor(world) {
                stop(world, mayor);
            }
        }),
        item("Undo the Last Change".into(), |world| {
            chat::decree(world, HOME, "/undo".into())
        }),
    ]
}

fn for_ruin(world: &World, agent: Entity) -> Vec<AnyView> {
    vec![item(
        format!("Restore {}", agents::name_of(world, agent)),
        move |world| restore(world, agent),
    )]
}

fn for_resident(
    world: &World,
    thing: Thing,
    agent: Entity,
    at: Vec2,
) -> Vec<AnyView> {
    let name = |agent: Entity| agents::name_of(world, agent);
    let mut items =
        vec![item(format!("Talk to {}", name(agent)), move |w| {
            talk_to(w, agent)
        })];
    items.push(item("Prompt".into(), move |w| {
        talk_to(w, agent);
        show_prompt(w);
    }));
    items.push(focus(thing));
    items.push(item("Follow".into(), move |w| {
        focus_on(w, thing, true)
    }));
    if world.get::<Busy>(agent).is_some() {
        items.push(item(format!("Stop {}", name(agent)), move |w| {
            stop(w, agent)
        }));
    }
    let Some((_, me)) = resident(world, thing) else {
        return items;
    };
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
        if joined {
            items.push(item(
                format!("Remove the road to {other}"),
                move |w| hand::part(w, agent, them),
            ));
        } else {
            items.push(item(
                format!("Build a road to {other}"),
                move |w| hand::introduce(w, agent, them),
            ));
        }
    }
    if !hand::offers(world, agent).is_empty() {
        items.push(
            menu_item(label("Give a skill...").wrap(false))
                .on_activate(move |world| {
                    *world.resource_mut::<Open>() =
                        Open::Skills(agent, at);
                })
                .boxed(),
        );
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
        items.push(asking(
            format!("Merge {other} into {me}..."),
            into_me,
            at,
        ));
        items.push(asking(
            format!("Merge {me} into {other}..."),
            into_other,
            at,
        ));
    }
    // The roads it has, besides the one to the other of
    // two that are chosen, which is offered above.
    for (peer, name) in joined(world, agent) {
        if Some(peer) != chosen {
            items.push(item(
                format!("Remove the road to {name}"),
                move |w| hand::part(w, agent, peer),
            ));
        }
    }
    items.push(item("Undo the Last Change".into(), move |world| {
        hand::decree(world, agent, "/undo".into())
    }));
    items.push(asking(
        format!("Make {me} Mayor..."),
        Deed::Mayor(agent),
        at,
    ));
    items.push(asking(
        format!("Remove {me}..."),
        Deed::Remove(agent),
        at,
    ));
    items
}

fn for_common(
    world: &World,
    thing: Thing,
    hamlet: Entity,
    at: Vec2,
) -> Vec<AnyView> {
    let name = agents::name_of(world, hamlet);
    let mut items = vec![focus(thing)];
    // It is closed by the name commands know it by. The
    // first hamlet is the home itself and stays.
    let id = world
        .get::<Settlement>(hamlet)
        .map(|settlement| settlement.name.clone())
        .unwrap_or_default();
    items.push(item(format!("Move {name}..."), move |w| {
        hand::carry(w, hamlet)
    }));
    if !id.is_empty() && id != HOME {
        items.push(asking(
            format!("Close Down {name}..."),
            Deed::Close(id),
            at,
        ));
    }
    items
}

fn for_chief(thing: Thing, chief: Entity) -> Vec<AnyView> {
    let home = Thing::ChiefPlace(chief);
    vec![
        item("Change Your Look".into(), move |world| {
            select(world, Some(home))
        }),
        focus(thing),
        item("Go to the Hearth".into(), move |w| {
            focus_on(w, home, false)
        }),
        item("Move the Hearth...".into(), crate::chief::carry_home),
    ]
}

fn for_library(thing: Thing) -> Vec<AnyView> {
    vec![
        focus(thing),
        item("Move the Library...".into(), |w| {
            moving::pick_up(w, Ground::Library)
        }),
    ]
}

/// The skills `agent` could be given, each pressed to give it.
pub(super) fn skills(world: &World, agent: Entity) -> Vec<AnyView> {
    hand::offers(world, agent)
        .into_iter()
        .map(|offer| {
            let text = match &offer.from {
                Some(from) => format!("{} (of {from})", offer.skill),
                None => format!("{} (starter)", offer.skill),
            };
            item(text, move |w| hand::give(w, agent, &offer))
        })
        .collect()
}

/// The menu of `thing`.
pub(super) fn items(
    world: &World,
    thing: Thing,
    at: Vec2,
) -> Vec<AnyView> {
    match thing {
        Thing::Keeper | Thing::Cottage => for_mayor(),
        Thing::Ruin(agent) => for_ruin(world, agent),
        Thing::Resident(agent) | Thing::ResidentHouse(agent) => {
            for_resident(world, thing, agent, at)
        }
        Thing::Common(hamlet) => for_common(world, thing, hamlet, at),
        Thing::Clockwork(hamlet) | Thing::Workbench(hamlet) => {
            for_common(world, Thing::Common(hamlet), hamlet, at)
        }
        Thing::Chief(chief) | Thing::ChiefPlace(chief) => {
            for_chief(thing, chief)
        }
        Thing::Library => for_library(thing),
    }
}
