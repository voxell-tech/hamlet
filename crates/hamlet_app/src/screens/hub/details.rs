//! The details sheet beside a thread: what only it shows of the
//! agent. Its kind, scholar, lead and hires are in the path and the
//! rail, its model in the composer and its sessions and side chats in
//! the session menu, so they are not repeated here.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, label, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::realm::{
    Account, Allowance, Bylaws, Deeds, Kind, LivesIn, Road, Scholar,
    ToolsOff, id_of, name_of, roads_of, shown_of,
};
use hamlet_protocol::client as protocol;
use hamlet_realm::orders;
use hamlet_ui::styles::{
    CONTROL, Fills, ICON, action, back_link, bare, description,
    framed, lettered, note, scroll_area, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::hint::hinted;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::switch::switch;
use hamlet_ui::{AnyView, View, with_cx};

use super::memory;
use super::thread::said;
use crate::convo::talk_to;
use crate::overview::heading;

/// How wide the names of the settings are, so their values line up.
const NAMES: f32 = 76.0;

/// How wide the field of an amount in dollars is, and the least width
/// of the choice of a version above it.
const AMOUNT: f32 = 96.0;

/// What the sheet shows of the agent, found from the world.
#[derive(Clone, Debug, PartialEq)]
struct Facts {
    hand: bool,
    worker: bool,
    purpose: String,
    /// The version it is pinned to, none for its scholar's `main`.
    pin: Option<String>,
    /// Its scholar's named versions, when the chief may pin it to
    /// one.
    versions: Option<Vec<String>>,
    /// Its address, and what it is called in the world.
    address: String,
    called: String,
    roads: Vec<(Entity, String)>,
    spent: String,
    /// Its allowance in dollars, and the period of its hamlet's
    /// budget that it is for.
    allowance: Option<f64>,
    period: &'static str,
    tools_off: Vec<String>,
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
        worker: world.get::<Kind>(agent).map(|kind| kind.0)
            == Some(protocol::AgentKind::Worker),
        purpose: world
            .get::<hamlet_client::realm::Purpose>(agent)
            .map(|purpose| purpose.0.clone())
            .unwrap_or_default(),
        pin: world
            .get::<hamlet_client::realm::Pin>(agent)
            .map(|pin| pin.0.clone()),
        versions: Deeds::is_allowed(world, "pin")
            .then(|| versions_of(world, agent))
            .flatten(),
        address: name_of(world, agent),
        called: shown_of(world, agent),
        roads,
        spent: world
            .get::<Account>(agent)
            .map(|account| account.cost.clone())
            .unwrap_or_default(),
        allowance: world.get::<Allowance>(agent).map(|a| a.0),
        period: match world
            .get::<LivesIn>(agent)
            .and_then(|hamlet| world.get::<Bylaws>(hamlet.0))
            .map(|bylaws| bylaws.0.budget.period)
        {
            Some(protocol::Period::Month) => "a month",
            _ => "a day",
        },
        tools_off: world
            .get::<ToolsOff>(agent)
            .map(|off| off.0.clone())
            .unwrap_or_default(),
    }
}

/// The named versions of the scholar `agent` is a hire of.
fn versions_of(world: &World, agent: Entity) -> Option<Vec<String>> {
    let id = id_of(world, agent)?;
    let mut scholars = world.try_query::<&Scholar>()?;
    scholars
        .iter(world)
        .find(|scholar| scholar.0.hires.contains(&id))
        .map(|scholar| {
            scholar
                .0
                .versions
                .iter()
                .map(|v| v.name.clone())
                .collect()
        })
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
    let had = facts.called.clone();
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

/// The amount typed in a field of dollars, without its `$`.
fn dollars(typed: &str) -> &str {
    typed.trim().trim_start_matches('$').trim()
}

/// Its allowance in dollars, typed over: Enter gives it the amount,
/// and a field left empty no allowance.
fn allowance_field(facts: &Facts) -> AnyView {
    let address = facts.address.clone();
    let shown = facts
        .allowance
        .map(|dollars| format!("${dollars:.2}"))
        .unwrap_or_default();
    let had = dollars(&shown).to_string();
    column((hinted(
        text_field(shown, move |world, typed| {
            let typed = dollars(&typed);
            if typed != had {
                let amount =
                    if typed.is_empty() { "none" } else { typed };
                crate::chat::command(
                    world,
                    format!("/allowance {address} {amount}"),
                );
            }
        })
        .rules(text_input),
        "No limit",
    ),))
    .width(px(AMOUNT))
    .boxed()
}

/// The version of its scholar it works from, chosen from the
/// scholar's versions where it may be pinned.
fn version(facts: &Facts) -> AnyView {
    let now = facts.pin.clone().unwrap_or_else(|| "main".into());
    let Some(versions) = &facts.versions else {
        return label(shown_version(&now)).boxed();
    };
    let mut names = vec!["main".to_string()];
    names.extend(versions.iter().cloned());
    let at = names.iter().position(|name| *name == now).unwrap_or(0);
    let words = names
        .iter()
        .map(|name| shown_version(name))
        .collect::<Vec<_>>();
    let address = facts.address.clone();
    // Framed as the fields beside it are.
    with_cx(move |cx| {
        let theme = cx.theme();
        row((dropdown(
            words,
            at,
            Glyph::Chevron.image(ICON),
            move |world, chosen| {
                if chosen != at
                    && let Some(to) = names.get(chosen)
                {
                    crate::chat::command(
                        world,
                        format!("/pin {address} {to}"),
                    );
                }
            },
        )
        .min_width(px(AMOUNT))
        .fill(theme.palette.input)
        .border(theme.space.border)
        .border_color(theme.palette.gold_5)
        .radius(theme.space.round),))
    })
    .boxed()
}

/// A version as the chief reads it: `main` is what its scholar knows
/// now.
fn shown_version(name: &str) -> String {
    match name {
        "main" => "Latest".to_string(),
        version => version.to_string(),
    }
}

/// The built-in tools a hire can be kept from, each with what it
/// does.
const TOOLS: [(&str, &str); 6] = [
    ("read", "Reads files"),
    ("write", "Writes whole files"),
    ("edit", "Changes part of a file"),
    ("bash", "Runs commands"),
    ("search", "Looks for words in files"),
    ("find", "Finds files by name"),
];

/// A switch for each of its built-in tools.
fn tools(facts: &Facts) -> AnyView {
    let rows = TOOLS
        .iter()
        .map(|&(tool, does)| {
            let on = !facts.tools_off.iter().any(|off| off == tool);
            let address = facts.address.clone();
            row((
                switch(on, move |world, on| {
                    let sign = if on { '+' } else { '-' };
                    crate::chat::command(
                        world,
                        format!("/tools {address} {sign}{tool}"),
                    );
                }),
                label(tool).bold(true).wrap(false),
                column((label(does).tone(Tone::Faint),))
                    .grow(1.0)
                    .min_width(px(0.0)),
            ))
            .gap(8.0)
            .align(AlignItems::Center)
            .boxed()
        })
        .collect::<Vec<_>>();
    card(Glyph::Key, "Tools", None, column(rows).gap(6.0))
}

/// An agent's name that leads to its thread.
fn link(entity: Entity, name: String) -> AnyView {
    lettered(name)
        .rules(back_link)
        .tagged(Variant::Text)
        .on_activate(move |world| talk_to(world, entity))
        .boxed()
}

/// Who it is and what it may spend, which a chief changes here.
fn about(facts: &Facts) -> Option<AnyView> {
    let mut parts = Vec::new();
    if !facts.purpose.is_empty() && !facts.hand {
        parts.push(said(facts.purpose.clone()));
    }
    if !facts.hand {
        parts.push(setting(
            "Called",
            "What it is called in the world; empty for its own name",
            nickname_field(facts),
        ));
        parts.push(setting(
            "Version",
            "The version of its scholar it works from",
            version(facts),
        ));
        parts.push(setting(
            "Allowance",
            "Dollars it may spend in its hamlet's budget period; \
             empty for no limit",
            row((
                allowance_field(facts),
                label(facts.period).tone(Tone::Faint).wrap(false),
            ))
            .gap(6.0)
            .align(AlignItems::Center)
            .grow(1.0),
        ));
    }
    if !facts.spent.is_empty() {
        parts.push(setting(
            "Spent",
            "What it has spent today",
            label(facts.spent.clone()),
        ));
    }
    (!parts.is_empty()).then(|| {
        card(
            Glyph::Robot,
            "Identity and budget",
            None,
            column(parts).gap(6.0),
        )
    })
}

/// The agents it has a road to, each to take up, and a way to build
/// another.
fn roads(agent: Entity, facts: &Facts) -> AnyView {
    let mut parts = Vec::new();
    for (other, name) in facts.roads.iter().cloned() {
        let cut = button(glyph(Glyph::Cross, ICON).tone(Tone::Faint))
            .padding(UiRect::all(px(2.0)))
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
    card(Glyph::Signpost, "Roads", None, column(parts).gap(6.0))
}

/// The cards of the agent's own facts that `build` makes, kept in
/// step with the world.
fn facts_cards(
    agent: Entity,
    build: fn(Entity, &Facts, bool) -> Vec<AnyView>,
) -> impl View {
    keyed(
        derived(move |world: &World| {
            let roads = Deeds::is_allowed(world, "road")
                || Deeds::is_allowed(world, "unroad");
            (facts(world, agent), roads)
        })
        .when(every_frame()),
        move |(facts, may_road)| {
            let parts = build(agent, facts, *may_road);
            with_cx(move |cx| {
                column(parts).gap(cx.theme().space.section)
            })
            .boxed()
        },
    )
    .within(bare())
}

/// Who it is and what it may spend.
fn head(_: Entity, facts: &Facts, _: bool) -> Vec<AnyView> {
    about(facts).into_iter().collect()
}

/// What it may do: its tools and its roads.
fn powers(
    agent: Entity,
    facts: &Facts,
    may_road: bool,
) -> Vec<AnyView> {
    let mut parts = Vec::new();
    if !facts.hand && !facts.worker {
        parts.push(tools(facts));
    }
    if may_road && !facts.hand {
        parts.push(roads(agent, facts));
    }
    parts
}

/// A part of the sheet on its card: its name under `which`, the one
/// thing done to it at the end of that line, and `body` under it.
pub(super) fn card(
    which: Glyph,
    name: &'static str,
    end: Option<AnyView>,
    body: impl View + 'static,
) -> AnyView {
    let end = end.unwrap_or_else(|| frame().boxed());
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        framed((
            row((heading(which, name), frame().grow(1.0), end))
                .gap(gap)
                .align(AlignItems::Center)
                .min_height(px(CONTROL)),
            column((body,)).padding(UiRect::top(px(gap / 2.0))),
        ))
    })
    .boxed()
}

/// The way to its whole prompt, on a page of its own.
fn prompt() -> AnyView {
    card(
        Glyph::Notes,
        "Prompt",
        Some(action(Glyph::Into, "Open", super::thread::show_prompt)),
        description(
            "Everything it is told at the start of each turn.",
        ),
    )
}

/// A reflection asked now, which writes to its memory.
fn reflect() -> AnyView {
    action(Glyph::Rename, "Note what it learned", |world| {
        if let Some(to) = super::papers::selected(world) {
            world.trigger(hamlet_client::messages::ChiefMessage {
                to,
                text: "/reflect".into(),
            });
        }
    })
    .tooltip(|| {
        note("It writes what it learned so far to its memory")
    })
    .boxed()
}

/// The sheet that takes the place of the thread of `agent` while it
/// is open.
pub(super) fn sheet(agent: Entity) -> impl View {
    with_cx(move |cx| {
        let gap = cx.theme().space.section;
        column((scroll_area((column((
            facts_cards(agent, head),
            memory::character_section(),
            card(
                Glyph::Notes,
                "Memory",
                Some(reflect()),
                memory::view(),
            ),
            card(Glyph::Book, "Skills", None, memory::skills()),
            prompt(),
            facts_cards(agent, powers),
        ))
        .gap(gap)
        .width(percent(100.0)),)),))
        .width(percent(100.0))
        .min_width(px(0.0))
        .fills_height()
    })
}
