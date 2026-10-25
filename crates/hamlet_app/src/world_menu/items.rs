//! The menu of each kind of thing in the world.

use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy_fynix::ViewExt;
use bevy_fynix::views::{BehaviorExt, label, menu_item};
use hamlet_client::gifts::{give, offers};
use hamlet_client::messages::ChiefMessage;
use hamlet_client::realm::{
    self as agents, Busy, Deeds, HOME, HamletId, Left, Mayor, Road,
    road_between, roads_of,
};
use hamlet_realm::generator::society::Ground;
use hamlet_realm::pick::{Also, Selected, Thing, focus_on, select};
use hamlet_realm::{moving, orders};
use hamlet_ui::AnyView;

use super::deed::Deed;
use super::{Open, close, restore, stop};
use crate::chat;
use crate::convo::{talk_to, talk_to_mayor};
use crate::inspector::show_prompt;

/// An agent of the hamlet that is not its mayor and has not left,
/// with its name.
fn plain_agent(
    world: &World,
    thing: Thing,
) -> Option<(Entity, String)> {
    let (Thing::Agent(agent) | Thing::House(agent)) = thing else {
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
    let mut peers = roads_of(world, agent)
        .filter_map(|road| world.get::<Road>(road))
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

fn new_conversation(agent: Entity) -> AnyView {
    item("New Conversation".into(), move |world| {
        if let Some(to) = agents::address_of(world, agent) {
            world.trigger(ChiefMessage {
                to,
                text: "/new".into(),
            });
            world.flush();
        }
    })
}

fn gifts(world: &World, agent: Entity, at: Vec2) -> Vec<AnyView> {
    if !Deeds::is_allowed(world, "give")
        || offers(world, agent).is_empty()
    {
        return Vec::new();
    }
    vec![
        menu_item(label("Give a skill...").wrap(false))
            .on_activate(move |world| {
                *world.resource_mut::<Open>() =
                    Open::Skills(agent, at);
            })
            .boxed(),
    ]
}

fn for_mayor(world: &World, at: Vec2) -> Vec<AnyView> {
    let mut items = vec![
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
            chat::decree(world, HOME, "/undo".into());
        }),
    ];
    if let Some(mayor) = agents::mayor(world) {
        items.insert(1, new_conversation(mayor));
        items.extend(gifts(world, mayor, at));
    }
    items
}

fn for_ruin(world: &World, agent: Entity) -> Vec<AnyView> {
    if Deeds::is_allowed(world, "restore") {
        vec![item(
            format!("Restore {}", agents::name_of(world, agent)),
            move |world| restore(world, agent),
        )]
    } else {
        Vec::new()
    }
}

fn for_agent(
    world: &World,
    thing: Thing,
    agent: Entity,
    at: Vec2,
) -> Vec<AnyView> {
    let name = |agent: Entity| agents::name_of(world, agent);
    let mut items =
        vec![item(format!("Talk to {}", name(agent)), move |w| {
            talk_to(w, agent);
        })];
    items.push(new_conversation(agent));
    items.extend(gifts(world, agent, at));
    items.push(item("Prompt".into(), move |w| {
        talk_to(w, agent);
        show_prompt(w);
    }));
    items.push(focus(thing));
    items.push(item("Follow".into(), move |w| {
        focus_on(w, thing, true);
    }));
    if world.get::<Busy>(agent).is_some() {
        items.push(item(format!("Stop {}", name(agent)), move |w| {
            stop(w, agent);
        }));
    }
    items.push(item("Undo the Last Change".into(), move |world| {
        orders::decree(world, agent, "/undo".into());
    }));
    let Some((_, me)) = plain_agent(world, thing) else {
        return items;
    };
    // The other of the two that are chosen, if there are two.
    let chosen =
        [world.resource::<Selected>().0, world.resource::<Also>().0];
    let other = chosen
        .into_iter()
        .flatten()
        .filter_map(|thing| plain_agent(world, thing))
        .find(|(other, _)| *other != agent);
    let chosen = other.as_ref().map(|(them, _)| *them);
    if let Some((them, other)) = other.clone() {
        let joined = road_between(world, agent, them).is_some();
        if joined && Deeds::is_allowed(world, "unroad") {
            items.push(item(
                format!("Remove the road to {other}"),
                move |w| orders::part(w, agent, them),
            ));
        } else if !joined && Deeds::is_allowed(world, "road") {
            items.push(item(
                format!("Build a road to {other}"),
                move |w| orders::introduce(w, agent, them),
            ));
        }
    }
    if let Some((them, other)) =
        other.filter(|_| Deeds::is_allowed(world, "merge"))
    {
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
        if Some(peer) != chosen && Deeds::is_allowed(world, "unroad")
        {
            items.push(item(
                format!("Remove the road to {name}"),
                move |w| orders::part(w, agent, peer),
            ));
        }
    }
    if Deeds::is_allowed(world, "mayor") {
        items.push(asking(
            format!("Make {me} Mayor..."),
            Deed::Mayor(agent),
            at,
        ));
    }
    if Deeds::is_allowed(world, "remove") {
        items.push(asking(
            format!("Remove {me}..."),
            Deed::Remove(agent),
            at,
        ));
    }
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
        .get::<HamletId>(hamlet)
        .map(|id| id.0.clone())
        .unwrap_or_default();
    if Deeds::is_allowed(world, "move-hamlet") {
        items.push(item(format!("Move {name}..."), move |w| {
            orders::carry(w, hamlet);
        }));
    }
    if !id.is_empty()
        && id != HOME
        && Deeds::is_allowed(world, "remove-hamlet")
    {
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
            select(world, Some(home));
        }),
        focus(thing),
        item("Go to the Hearth".into(), move |w| {
            focus_on(w, home, false);
        }),
        item("Move the Hearth...".into(), crate::chief::carry_home),
    ]
}

fn for_library(thing: Thing) -> Vec<AnyView> {
    vec![
        focus(thing),
        item("Move the Library...".into(), |w| {
            moving::pick_up(w, Ground::Library);
        }),
    ]
}

fn for_bazaar(thing: Thing) -> Vec<AnyView> {
    vec![
        focus(thing),
        item("Move the Agent Bazaar...".into(), |w| {
            moving::pick_up(w, Ground::Bazaar);
        }),
    ]
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
            item(text, move |w| give(w, agent, &offer))
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
        Thing::Keeper | Thing::Cottage => for_mayor(world, at),
        Thing::Ruin(agent) => for_ruin(world, agent),
        Thing::Agent(agent) | Thing::House(agent) => {
            for_agent(world, thing, agent, at)
        }
        Thing::Common(hamlet) => for_common(world, thing, hamlet, at),
        Thing::Clockwork(hamlet) | Thing::Workbench(hamlet) => {
            for_common(world, Thing::Common(hamlet), hamlet, at)
        }
        Thing::Chief(chief) | Thing::ChiefPlace(chief) => {
            for_chief(thing, chief)
        }
        Thing::Library => for_library(thing),
        Thing::Bazaar | Thing::Stall(_) => for_bazaar(thing),
    }
}
