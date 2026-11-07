//! The details sheet beside a thread: what only it shows of the
//! agent. Its kind, scholar, lead and hires are in the path and the
//! rail, its model in the composer and its sessions and side chats in
//! the session menu, so they are not repeated here.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, label, row,
    text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::realm::{
    Account, Deeds, Kind, Road, name_of, roads_of, shown_of,
};
use hamlet_protocol::client as protocol;
use hamlet_realm::orders;
use hamlet_ui::styles::{
    Fills, ICON, action, back_link, bare, hairline, lettered, note,
    scroll_area, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, View, with_cx};

use super::memory;
use super::thread::said;
use crate::convo::talk_to;
use crate::overview::heading;

/// How wide the names of the settings are, so their values line up.
const NAMES: f32 = 76.0;

/// What the sheet shows of the agent, found from the world.
#[derive(Clone, Debug, PartialEq)]
struct Facts {
    hand: bool,
    purpose: String,
    /// The version it is pinned to, none for its scholar's `main`.
    pin: Option<String>,
    /// Its address, and what it is called in the world.
    address: String,
    nickname: Option<String>,
    roads: Vec<(Entity, String)>,
    spent: String,
}

fn facts(world: &World, agent: Entity) -> Facts {
    let mut roads = roads_of(world, agent)
        .filter_map(|road| world.get::<Road>(road))
        .map(|road| {
            if road.ends.0 == agent {
                road.ends.1
            } else {
                road.ends.0
            }
        })
        .map(|other| (other, shown_of(world, other)))
        .collect::<Vec<_>>();
    roads.sort_by(|a, b| a.1.cmp(&b.1));
    Facts {
        hand: world.get::<Kind>(agent).map(|kind| kind.0)
            == Some(protocol::AgentKind::Hand),
        purpose: world
            .get::<hamlet_client::realm::Purpose>(agent)
            .map(|purpose| purpose.0.clone())
            .unwrap_or_default(),
        pin: world
            .get::<hamlet_client::realm::Pin>(agent)
            .map(|pin| pin.0.clone()),
        address: name_of(world, agent),
        nickname: world
            .get::<hamlet_client::realm::Nickname>(agent)
            .map(|nickname| nickname.0.clone()),
        roads,
        spent: world
            .get::<Account>(agent)
            .map(|account| account.cost.clone())
            .unwrap_or_default(),
    }
}

/// A setting: its name, which says more on hover, and its value.
fn setting(
    name: &'static str,
    says: &'static str,
    value: impl View + 'static,
) -> AnyView {
    row((
        column((label(name).tone(Tone::Faint).wrap(false),))
            .width(px(NAMES))
            .tooltip(move || note(says)),
        value,
    ))
    .gap(8.0)
    .align(AlignItems::Center)
    .boxed()
}

/// What a hire is called in the world, typed over: Enter gives it the
/// nickname, and a field left empty its own name again.
fn nickname_field(facts: &Facts) -> AnyView {
    let address = facts.address.clone();
    let had = facts.nickname.clone().unwrap_or_default();
    text_field(had.clone(), move |world, typed| {
        let typed = typed.trim().to_string();
        if typed != had {
            crate::chat::command(
                world,
                format!("/nickname {address} {typed}")
                    .trim_end()
                    .into(),
            );
        }
    })
    .grow(1.0)
    .min_width(px(0.0))
    .rules(text_input)
    .boxed()
}

/// A field that gives the command `/<command> <address> <typed>` on
/// Enter, and empties itself after.
fn command_field(address: &str, command: &'static str) -> AnyView {
    let address = address.to_string();
    text_field(String::new(), move |world, typed| {
        let typed = typed.trim();
        if !typed.is_empty() {
            crate::chat::command(
                world,
                format!("/{command} {address} {typed}"),
            );
        }
    })
    .grow(1.0)
    .min_width(px(0.0))
    .rules(text_input)
    .boxed()
}

/// An agent's name that leads to its thread.
fn link(entity: Entity, name: String) -> AnyView {
    lettered(name)
        .rules(back_link)
        .tagged(Variant::Text)
        .on_activate(move |world| talk_to(world, entity))
        .boxed()
}

/// Its settings, which a chief changes here.
fn settings(facts: &Facts) -> AnyView {
    let mut parts = Vec::new();
    if !facts.hand {
        parts.push(setting(
            "Called",
            "What it is called in the world; empty for its own name",
            nickname_field(facts),
        ));
        parts.push(setting(
            "Version",
            "The version of its scholar it works from",
            label(match &facts.pin {
                Some(pin) => pin.clone(),
                None => "main".to_string(),
            }),
        ));
        parts.push(setting(
            "Allowance",
            "Dollars it may spend a day, or none",
            command_field(&facts.address, "allowance"),
        ));
        parts.push(setting(
            "Tools",
            "-bash turns a tool off, +bash on again",
            command_field(&facts.address, "tools"),
        ));
    }
    if !facts.spent.is_empty() {
        parts.push(setting(
            "Spent",
            "What it has spent today",
            label(facts.spent.clone()),
        ));
    }
    column(parts).gap(6.0).boxed()
}

/// The agents it has a road to, each to take up, and a way to build
/// another.
fn roads(agent: Entity, facts: &Facts) -> AnyView {
    let mut parts = vec![heading(Glyph::Signpost, "Roads")];
    for (other, name) in facts.roads.iter().cloned() {
        let cut = button(glyph(Glyph::Cross, ICON).tone(Tone::Faint))
            .padding(bevy::ui::UiRect::all(px(2.0)))
            .rules(bevy_fynix::views::tint)
            .tooltip(|| note("Take up the road"))
            .on_activate(move |world| {
                orders::part(world, agent, other);
            });
        parts.push(
            row((link(other, name), cut))
                .gap(6.0)
                .align(AlignItems::Center)
                .boxed(),
        );
    }
    parts.push(
        row((action(Glyph::Plus, "Build a road", move |world| {
            crate::world_menu::ask_road(world, agent);
        }),))
        .boxed(),
    );
    column(parts).gap(6.0).boxed()
}

fn agent_section(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            let roads = Deeds::is_allowed(world, "road")
                || Deeds::is_allowed(world, "unroad");
            (facts(world, agent), roads)
        })
        .when(every_frame()),
        move |(facts, may_road)| {
            let mut parts = Vec::new();
            if !facts.purpose.is_empty() {
                parts.push(said(facts.purpose.clone()));
            }
            parts.push(settings(facts));
            if *may_road && !facts.hand {
                parts.push(roads(agent, facts));
            }
            column(parts).gap(12.0).boxed()
        },
    )
    .within(bare())
}

/// Its own skills and its character, which a chief may edit.
fn skills_and_character() -> impl View {
    let edit =
        action(Glyph::Rename, "Edit", super::papers::edit_character);
    column((
        heading(Glyph::Book, "Skills"),
        memory::skills(),
        heading(Glyph::Robot, "Character"),
        memory::character(),
        row((edit,)),
    ))
    .gap(6.0)
}

/// Its prompt in full, and a reflection asked now.
fn pages() -> impl View {
    row((
        action(Glyph::Lens, "Prompt", super::thread::show_prompt),
        action(Glyph::Notes, "Reflect now", |world| {
            if let Some(to) = super::papers::selected(world) {
                world.trigger(
                    hamlet_client::messages::ChiefMessage {
                        to,
                        text: "/reflect".into(),
                    },
                );
            }
        })
        .tooltip(|| note("Write down what it learned so far")),
    ))
    .gap(8.0)
    .tagged(hamlet_ui::widgets::platform::Wraps)
}

/// The sheet that takes the place of the thread of `agent` while it
/// is open.
pub(super) fn sheet(agent: Entity) -> impl View {
    with_cx(move |cx| {
        let line = cx.theme().palette.gold_6;
        let gap = cx.theme().space.section;
        column((scroll_area((column((
            agent_section(agent),
            pages(),
            hairline(line),
            heading(Glyph::Notes, "Memory"),
            memory::view(),
            hairline(line),
            skills_and_character(),
        ))
        .gap(gap)
        .width(percent(100.0)),)),))
        .width(percent(100.0))
        .min_width(px(0.0))
        .fills_height()
    })
}
