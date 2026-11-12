//! The menu of each kind of thing in the world.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{BehaviorExt, Label, label, menu_item};
use hamlet_client::realm::{
    self as agents, Busy, Deeds, HamletId, Kind, Left, Mayor,
};
use hamlet_protocol::client as protocol;
use hamlet_realm::generator::society::Ground;
use hamlet_realm::pick::{Also, Selected, Thing, focus_on, select};
use hamlet_realm::{moving, orders};
use hamlet_ui::AnyView;

use super::deed::Deed;
use super::{close, restore, stop};

/// A hire of the hamlet that is not its mayor and has not left, with
/// its name.
fn plain_agent(
    world: &World,
    thing: Thing,
) -> Option<(Entity, String)> {
    let (Thing::Agent(agent) | Thing::House(agent)) = thing else {
        return None;
    };
    let plain = world.get::<Kind>(agent).map(|kind| kind.0)
        == Some(protocol::AgentKind::Hire)
        && world.get::<Mayor>(agent).is_none()
        && world.get::<Left>(agent).is_none();
    let name = agents::shown_of(world, agent);
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
fn asking(text: Label, deed: Deed, at: Vec2) -> AnyView {
    menu_item(text.wrap(false))
        .on_activate(move |world| {
            super::ask_against(
                world,
                deed.clone(),
                Rect::from_corners(at, at),
            );
        })
        .boxed()
}

/// Hires a scholar under `lead`, while it is a living hire.
fn hire_under(world: &World, lead: Entity) -> Option<AnyView> {
    let hire = world.get::<Kind>(lead).map(|kind| kind.0)
        == Some(protocol::AgentKind::Hire);
    (hire
        && world.get::<Left>(lead).is_none()
        && Deeds::is_allowed(world, "hire"))
    .then(|| {
        item("Hire Under...".into(), move |world| {
            super::ask_hire(world, lead);
        })
    })
}

fn focus(thing: Thing) -> AnyView {
    item("Focus".into(), move |w| focus_on(w, thing, false))
}

/// Builds a road from the hire `agent`, while it may have one.
fn road(world: &World, agent: Entity) -> Option<AnyView> {
    let hire = world.get::<Kind>(agent).map(|kind| kind.0)
        == Some(protocol::AgentKind::Hire);
    (hire
        && world.get::<Left>(agent).is_none()
        && Deeds::is_allowed(world, "road"))
    .then(|| {
        item("Build a Road...".into(), move |world| {
            super::ask_road(world, agent);
        })
    })
}

fn for_mayor(world: &World) -> Vec<AnyView> {
    let mut items = vec![item("Stop the Mayor".into(), |world| {
        if let Some(mayor) = agents::first_mayor(world) {
            stop(world, mayor);
        }
    })];
    if let Some(mayor) = agents::first_mayor(world)
        && let Some(hire) = hire_under(world, mayor)
    {
        items.insert(0, hire);
    }
    items
}

fn for_ruin(world: &World, agent: Entity) -> Vec<AnyView> {
    if Deeds::is_allowed(world, "restore") {
        vec![item(
            format!("Restore {}", agents::shown_of(world, agent)),
            move |world| restore(world, agent),
        )]
    } else {
        Vec::new()
    }
}

/// What can be done to `agent` that its chat, which choosing it
/// opens, has no control for.
fn for_agent(
    world: &World,
    thing: Thing,
    agent: Entity,
    at: Vec2,
) -> Vec<AnyView> {
    let name = |agent: Entity| agents::shown_of(world, agent);
    let mut items = Vec::from_iter(hire_under(world, agent));
    items.extend(road(world, agent));
    if world.get::<Busy>(agent).is_some() {
        items.push(item(format!("Stop {}", name(agent)), move |w| {
            stop(w, agent);
        }));
    }
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
    // Its roads are built and taken up in its details.
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
            label(format!("Merge {other} into {me}...")),
            into_me,
            at,
        ));
        items.push(asking(
            label(format!("Merge {me} into {other}...")),
            into_other,
            at,
        ));
    }
    if Deeds::is_allowed(world, "mayor") {
        items.push(asking(
            label(format!("Make {me} Mayor...")),
            Deed::Mayor(agent),
            at,
        ));
    }
    if Deeds::is_allowed(world, "end") {
        items.push(asking(
            label(format!("End {me}...")),
            Deed::End(agent),
            at,
        ));
    }
    items
}

/// What can be done to `hamlet` that its page, which choosing it
/// opens, has no control for.
fn for_common(
    world: &World,
    hamlet: Entity,
    at: Vec2,
) -> Vec<AnyView> {
    let name = agents::shown_of(world, hamlet);
    // It is closed by the name commands know it by.
    let id = world
        .get::<HamletId>(hamlet)
        .map(|id| id.0.clone())
        .unwrap_or_default();
    // A hire under its mayor, first, as what is done to it most.
    let mut items = agents::mayor_of(world, &id)
        .and_then(|mayor| {
            hire_under(world, mayor).map(|_| {
                item("Hire...".into(), move |world| {
                    super::ask_hire(world, mayor);
                })
            })
        })
        .into_iter()
        .collect::<Vec<_>>();
    // Last, and red, as nothing done to it is harder to undo.
    if !id.is_empty() && Deeds::is_allowed(world, "remove-hamlet") {
        items.push(asking(
            label(format!("Close Down {name}..."))
                .tone(Tone::Critical),
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

/// Whether [`items`] holds anything for `thing`: for the Hand, only
/// while it can be stopped.
pub(super) fn has_items(world: &World, thing: Thing) -> bool {
    match (thing, agents::hand(world)) {
        (Thing::Hand, Some(hand)) => {
            world.get::<Busy>(hand).is_some()
        }
        (Thing::Hand, None) => false,
        _ => true,
    }
}

/// Stops the Hand, while it works.
fn stop_hand(world: &World, hand: Entity) -> Option<AnyView> {
    world.get::<Busy>(hand).is_some().then(|| {
        item("Stop the Hand".into(), move |w| {
            stop(w, hand);
        })
    })
}

/// The menu of `thing`, whether or not it is chosen.
pub(super) fn items(
    world: &World,
    thing: Thing,
    at: Vec2,
) -> Vec<AnyView> {
    match thing {
        Thing::Keeper | Thing::Cottage => for_mayor(world),
        Thing::Ruin(agent) => for_ruin(world, agent),
        Thing::Agent(agent) | Thing::House(agent) => {
            for_agent(world, thing, agent, at)
        }
        Thing::Common(hamlet)
        | Thing::Clockwork(hamlet)
        | Thing::Workbench(hamlet) => for_common(world, hamlet, at),
        Thing::Chief(chief) | Thing::ChiefPlace(chief) => {
            for_chief(thing, chief)
        }
        Thing::Library => for_library(thing),
        // Its tower stands where the city puts it: only the view
        // moves.
        // Choosing it opens its chat, with the rest.
        Thing::Hand => match agents::hand(world) {
            Some(hand) => Vec::from_iter(stop_hand(world, hand)),
            None => Vec::new(),
        },
        // A scholar's house, or the scholar waiting on the pier.
        // Selecting it already opens its page, where it is renamed,
        // and it is in view.
        Thing::Scholar(scholar) => {
            let mut items = Vec::new();
            let housed = world
                .get::<hamlet_client::realm::Scholar>(scholar)
                .is_some_and(|s| s.0.house.is_some());
            if housed && Deeds::is_allowed(world, "move-house") {
                items.push(item("Move House".into(), move |w| {
                    orders::carry_house(w, scholar);
                }));
            }
            items
        }
    }
}
