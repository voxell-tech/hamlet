//! The menu of each kind of thing in the world.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::ui::AlignItems;
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, label, menu_item, row,
};
use hamlet_client::realm::{
    self as agents, Actions, Busy, HamletId, Identity, Kind, Lead,
    Left, LivesIn,
};
use hamlet_core::orders::{self, CarryHouse, CarryLibrary};
use hamlet_core::pick::{Also, FocusOn, Selected, Thing, select};
use hamlet_core::toasts::WatchRequest;
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::ICON;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, View, with_cx};

use super::action::Action;
use super::{close, restore, stop};
use crate::profile_tag::ProfileTag;
use crate::reference::agent_chip;
use crate::screens::terminal::{Pins, pin};

/// A living agent of a hamlet that is not its lead.
fn plain_agent(world: &World, thing: Thing) -> Option<Entity> {
    let (Thing::Agent(agent) | Thing::House(agent)) = thing else {
        return None;
    };
    let plain = world.get::<Kind>(agent).map(|kind| kind.0)
        == Some(protocol::AgentKind::Hamlet)
        && world.get::<Lead>(agent).is_none()
        && world.get::<Left>(agent).is_none();
    plain.then_some(agent)
}

/// Words around the chip of `them`, the other of two chosen agents,
/// named as in the chat; the agent the menu is of goes unnamed.
fn naming(
    world: &World,
    them: Entity,
    before: &str,
    after: &str,
) -> impl View {
    row((
        label(before.to_owned()).wrap(false),
        agent_chip(world, them),
        label(after.to_owned()).wrap(false),
    ))
    .gap(5.0)
    .align(AlignItems::Center)
}

/// What an item shows: its icon in `tone`, then `words`.
fn line(which: Glyph, tone: Tone, words: impl View) -> impl View {
    with_cx(move |cx| {
        row((glyph(which, ICON).tone(tone), words))
            .gap(cx.theme().space.menu_gap)
            .align(AlignItems::Center)
    })
}

/// An item of `text` under the icon `which` in `tone`.
pub(super) fn item(
    which: Glyph,
    tone: Tone,
    text: &str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    menu_item(line(which, tone, label(text).wrap(false)))
        .on_activate(move |world| {
            close(world);
            act(world);
        })
        .boxed()
}

/// An item that asks before it does `action`.
fn asking(
    which: Glyph,
    tone: Tone,
    words: impl View,
    action: Action,
    at: Vec2,
) -> AnyView {
    menu_item(line(which, tone, words))
        .on_activate(move |world| {
            super::ask_against(
                world,
                action.clone(),
                Rect::from_corners(at, at),
            );
        })
        .boxed()
}

/// Adds an agent under `parent`, while it is a living agent of a
/// hamlet.
fn add_under(world: &World, parent: Entity) -> Option<AnyView> {
    let of_hamlet = world.get::<Kind>(parent).map(|kind| kind.0)
        == Some(protocol::AgentKind::Hamlet);
    (of_hamlet
        && world.get::<Left>(parent).is_none()
        && Actions::is_allowed(world, "add-agent"))
    .then(|| {
        item(
            Glyph::Plus,
            Tone::Success,
            "Add Agent Under",
            move |world| {
                super::ask_add_agent(world, parent);
            },
        )
    })
}

/// The items about the city profile `agent` is of.
fn of_profile(world: &World, agent: Entity) -> Vec<AnyView> {
    let Some(tag) = ProfileTag::of(world, agent) else {
        return Vec::new();
    };
    let profile = tag.profile;
    let mut items = vec![item(
        Glyph::Book,
        Tone::Info,
        "Go to Profile",
        move |world| crate::hub::open_profile(world, profile),
    )];
    if tag.learned
        && world.get::<Left>(agent).is_none()
        && Actions::is_allowed(world, "dream")
    {
        let address = agents::name_of(world, agent);
        items.push(item(
            Glyph::Moon,
            Tone::Merged,
            "Dream",
            move |world| {
                crate::chat::command(
                    world,
                    format!("/dream {address}"),
                );
            },
        ));
    }
    items
}

/// Makes a city profile of what the agent `agent` learned.
fn make_profile(world: &World, agent: Entity) -> Option<AnyView> {
    let of_hamlet = world.get::<Kind>(agent).map(|kind| kind.0)
        == Some(protocol::AgentKind::Hamlet);
    let allowed = Actions::is_allowed(world, "make-profile");
    (of_hamlet && allowed).then(|| {
        item(
            Glyph::Profile,
            Tone::Info,
            "Make Profile",
            move |world| {
                let text = format!(
                    "/make-profile {}",
                    agents::name_of(world, agent)
                );
                orders::decree(world, agent, text);
            },
        )
    })
}

fn focus(thing: Thing) -> AnyView {
    item(Glyph::Aim, Tone::Dim, "Focus", move |w| {
        w.trigger(FocusOn { thing, stay: false });
    })
}

/// Opens a channel from `agent`, an agent of a hamlet, while there is
/// one it may have.
fn channel(world: &World, agent: Entity) -> Option<AnyView> {
    let of_hamlet = world.get::<Kind>(agent).map(|kind| kind.0)
        == Some(protocol::AgentKind::Hamlet);
    (of_hamlet
        && world.get::<Left>(agent).is_none()
        && Actions::is_allowed(world, "channel")
        && !super::channel::choices(world, agent, "").is_empty())
    .then(|| {
        item(
            Glyph::Signpost,
            Tone::Reference,
            "Open a Channel",
            move |world| {
                super::ask_channel(world, agent);
            },
        )
    })
}

/// Pins `agent`'s command log to the Terminal list, or lets it go.
fn pin_bash(world: &World, agent: Entity) -> Option<AnyView> {
    let id = world.get::<Identity>(agent)?.0;
    let logged = world
        .try_query::<&hamlet_client::terminals::Terminal>()?
        .iter(world)
        .any(|t| {
            t.0.kind == protocol::TerminalKind::Commands
                && t.0.agent == Some(id)
        });
    let key = format!("agent#{}", id.0);
    let pinned = world.get_resource::<Pins>()?.0.contains(&key);
    logged.then(|| {
        let (which, text) = match pinned {
            true => (Glyph::Pinned, "Unpin Bash"),
            false => (Glyph::Pin, "Pin Bash"),
        };
        item(which, Tone::Info, text, move |world| pin(world, &key))
    })
}

/// Stops `agent`, telling the chief so in a toast headed `head`.
fn stop_item(agent: Entity, head: String) -> AnyView {
    item(Glyph::Stop, Tone::Warning, "Stop", move |w| {
        let request = stop(w, agent);
        w.trigger(WatchRequest {
            request,
            head: Some(head.clone()),
        });
    })
}

fn for_lead(world: &World) -> Vec<AnyView> {
    let mut items =
        vec![item(Glyph::Stop, Tone::Warning, "Stop", |w| {
            if let Some(lead) = agents::first_lead(w) {
                let request = stop(w, lead);
                w.trigger(WatchRequest {
                    request,
                    head: Some("Stopped the Lead".into()),
                });
            }
        })];
    if let Some(lead) = agents::first_lead(world) {
        if let Some(add) = add_under(world, lead) {
            items.insert(0, add);
        }
        items.extend(pin_bash(world, lead));
    }
    items
}

fn for_ruin(world: &World, agent: Entity) -> Vec<AnyView> {
    let mut items = Vec::new();
    if Actions::is_allowed(world, "restore") {
        items.push(item(
            Glyph::Restore,
            Tone::Success,
            "Restore",
            move |world| restore(world, agent),
        ));
    }
    items.extend(make_profile(world, agent));
    items
}

/// What can be done to `agent` that its chat has no control for.
fn for_agent(
    world: &World,
    thing: Thing,
    agent: Entity,
    at: Vec2,
) -> Vec<AnyView> {
    let mut items = Vec::from_iter(add_under(world, agent));
    items.extend(channel(world, agent));
    items.extend(pin_bash(world, agent));
    if world.get::<Busy>(agent).is_some() {
        let head =
            format!("Stopped {}", agents::shown_of(world, agent));
        items.push(stop_item(agent, head));
    }
    items.extend(of_profile(world, agent));
    items.extend(make_profile(world, agent));
    items.push(item(
        Glyph::Notes,
        Tone::Dim,
        "Copy Agent ID",
        move |w| {
            crate::reference::copy_agent(w, agent);
        },
    ));
    if plain_agent(world, thing).is_none() {
        return items;
    }
    // The other of the two that are chosen, if there are two.
    let chosen =
        [world.resource::<Selected>().0, world.resource::<Also>().0];
    let other = chosen
        .into_iter()
        .flatten()
        .filter_map(|thing| plain_agent(world, thing))
        .find(|other| *other != agent);
    // Its channels are opened and closed in its details.
    if let Some(them) =
        other.filter(|_| Actions::is_allowed(world, "merge"))
    {
        let into_me = Action::Merge {
            absorbed: them,
            kept: agent,
        };
        let into_other = Action::Merge {
            absorbed: agent,
            kept: them,
        };
        items.push(asking(
            Glyph::Merge,
            Tone::Merged,
            naming(world, them, "Merge", "Into This"),
            into_me,
            at,
        ));
        items.push(asking(
            Glyph::Merge,
            Tone::Merged,
            naming(world, them, "Merge Into", ""),
            into_other,
            at,
        ));
    }
    // A new parent may be the lead, which is never moved itself.
    let parent = chosen
        .into_iter()
        .flatten()
        .filter_map(|thing| match thing {
            Thing::Agent(other) | Thing::House(other) => Some(other),
            _ => None,
        })
        .find(|other| {
            *other != agent
                && world.get::<Left>(*other).is_none()
                && world.get::<Kind>(*other).map(|kind| kind.0)
                    == Some(protocol::AgentKind::Hamlet)
                && world.get::<LivesIn>(*other).map(|lives| lives.0)
                    == world
                        .get::<LivesIn>(agent)
                        .map(|lives| lives.0)
        });
    if let Some(them) =
        parent.filter(|_| Actions::is_allowed(world, "reparent"))
    {
        items.push(asking(
            Glyph::Move,
            Tone::Attention,
            naming(world, them, "Move Under", ""),
            Action::Reparent {
                moved: agent,
                parent: them,
            },
            at,
        ));
        if world.get::<Lead>(them).is_none() {
            items.push(asking(
                Glyph::Move,
                Tone::Attention,
                naming(world, them, "Move", "Under This"),
                Action::Reparent {
                    moved: them,
                    parent: agent,
                },
                at,
            ));
        }
    }
    if Actions::is_allowed(world, "lead") {
        items.push(asking(
            Glyph::Lead,
            Tone::Attention,
            label("Make Lead").wrap(false),
            Action::Lead(agent),
            at,
        ));
    }
    if Actions::is_allowed(world, "end") {
        items.push(asking(
            Glyph::Delete,
            Tone::Critical,
            label("End").wrap(false).tone(Tone::Critical),
            Action::End(agent),
            at,
        ));
    }
    items
}

/// What can be done to `hamlet` that its page has no control for.
fn for_common(
    world: &World,
    hamlet: Entity,
    at: Vec2,
) -> Vec<AnyView> {
    // It is closed by the name commands know it by.
    let id = world
        .get::<HamletId>(hamlet)
        .map(|id| id.0.clone())
        .unwrap_or_default();
    // An agent added under its lead, first, as what is done to it
    // most.
    let mut items = agents::lead_of(world, &id)
        .and_then(|lead| {
            add_under(world, lead).map(|_| {
                item(
                    Glyph::Plus,
                    Tone::Success,
                    "Add Agent",
                    move |w| {
                        super::ask_add_agent(w, lead);
                    },
                )
            })
        })
        .into_iter()
        .collect::<Vec<_>>();
    items.push(item(
        Glyph::Notes,
        Tone::Dim,
        "Copy Hamlet ID",
        move |w| {
            crate::reference::copy_hamlet(w, hamlet);
        },
    ));
    // Last, and red, as nothing done to it is harder to undo.
    if !id.is_empty() && Actions::is_allowed(world, "remove-hamlet") {
        items.push(asking(
            Glyph::Delete,
            Tone::Critical,
            label("Close Down").wrap(false).tone(Tone::Critical),
            Action::Close(id),
            at,
        ));
    }
    items
}

fn for_chief(thing: Thing, chief: Entity) -> Vec<AnyView> {
    let home = Thing::ChiefPlace(chief);
    vec![
        item(
            Glyph::Face,
            Tone::Accent,
            "Change Your Look",
            move |w| {
                select(w, Some(home));
            },
        ),
        focus(thing),
        item(
            Glyph::Hearth,
            Tone::Warning,
            "Go to the Hearth",
            move |w| {
                w.trigger(FocusOn {
                    thing: home,
                    stay: false,
                });
            },
        ),
        item(
            Glyph::Move,
            Tone::Dim,
            "Move the Hearth",
            crate::chief::carry_home,
        ),
    ]
}

fn for_library(thing: Thing) -> Vec<AnyView> {
    vec![
        focus(thing),
        item(Glyph::Move, Tone::Dim, "Move", |w| {
            w.trigger(CarryLibrary);
        }),
    ]
}

/// Whether [`items`] holds anything for `thing`.
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
    world
        .get::<Busy>(hand)
        .is_some()
        .then(|| stop_item(hand, "Stopped the Hand".into()))
}

/// The menu of `thing`, whether or not it is chosen.
pub(super) fn items(
    world: &World,
    thing: Thing,
    at: Vec2,
) -> Vec<AnyView> {
    match thing {
        Thing::Keeper | Thing::Cottage => for_lead(world),
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
        // Its tower stays where the city puts it.
        Thing::Hand => match agents::hand(world) {
            Some(hand) => Vec::from_iter(stop_hand(world, hand)),
            None => Vec::new(),
        },
        // Selecting it already opens its page.
        Thing::Profile(profile) => {
            let mut items = Vec::new();
            let housed = world
                .get::<hamlet_client::realm::Profile>(profile)
                .is_some_and(|s| s.0.house.is_some());
            if housed && Actions::is_allowed(world, "move-house") {
                items.push(item(
                    Glyph::Move,
                    Tone::Dim,
                    "Move",
                    move |w| {
                        w.trigger(CarryHouse(profile));
                    },
                ));
            }
            items
        }
    }
}
