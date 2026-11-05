//! The details sheet beside a thread: what the agent is, its memory,
//! its skills and character, where it comes from, and its side chats.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::realm::{
    Account, Kind, Mayor, OfScholar, Parent, Road, Scholar, id_of,
    name_of, roads_of,
};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    Fills, action, back_link, bare, hairline, lettered, note,
    scroll_area,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, View, with_cx};

use super::papers::Papers;
use super::thread::{dim, said};
use super::{Showing, conversations, memory, show};
use crate::convo::talk_to;
use crate::overview::heading;

/// How wide the sheet is.
const WIDTH: f32 = 300.0;

/// What the Agent section lists, found from the world.
#[derive(Clone, Debug, PartialEq)]
struct Facts {
    kind: &'static str,
    purpose: String,
    scholar: Option<(Entity, String)>,
    lead: Option<(Entity, String)>,
    hires: Vec<(Entity, String)>,
    roads: Vec<(Entity, String)>,
    model: String,
    spent: String,
    lineage: Vec<(Entity, String)>,
}

fn facts(world: &World, agent: Entity) -> Facts {
    let kind = match world.get::<Kind>(agent).map(|kind| kind.0) {
        Some(protocol::AgentKind::Hand) => "The Hand",
        Some(protocol::AgentKind::Worker) => "Worker",
        _ if world.get::<Mayor>(agent).is_some() => "Mayor",
        _ => "Hire",
    };
    let named = |entity: Entity| (entity, name_of(world, entity));
    let scholar = world
        .get::<OfScholar>(agent)
        .and_then(|of| hamlet_client::realm::scholar(world, of.0))
        .map(|entity| {
            let name = world
                .get::<Scholar>(entity)
                .map(|scholar| scholar.0.name.clone())
                .unwrap_or_default();
            (entity, name)
        });
    let lead = |entity: Entity| {
        let parent = world.get::<Parent>(entity)?.0;
        hamlet_client::realm::agent(world, parent)
    };
    let id = id_of(world, agent);
    let mut hires = Vec::new();
    if let Some(id) = id
        && let Some(mut query) =
            world.try_query::<(Entity, &Parent, &Kind)>()
    {
        hires = query
            .iter(world)
            .filter(|(entity, parent, kind)| {
                parent.0 == id
                    && kind.0 != protocol::AgentKind::Worker
                    && id_of(world, *entity).is_some()
            })
            .map(|(entity, ..)| named(entity))
            .collect();
        hires.sort_by_key(|(_, name)| name.clone());
    }
    let roads = roads_of(world, agent)
        .filter_map(|road| world.get::<Road>(road))
        .map(|road| {
            if road.ends.0 == agent {
                road.ends.1
            } else {
                road.ends.0
            }
        })
        .map(named)
        .collect();
    // The leads above it, from the Hand down to the agent.
    let mut lineage = vec![named(agent)];
    let mut at = agent;
    while let Some(above) = lead(at) {
        if lineage.iter().any(|(seen, _)| *seen == above) {
            break;
        }
        lineage.insert(0, named(above));
        at = above;
    }
    let account = world.get::<Account>(agent);
    Facts {
        kind,
        purpose: world
            .get::<hamlet_client::realm::Purpose>(agent)
            .map(|purpose| purpose.0.clone())
            .unwrap_or_default(),
        scholar,
        lead: lead(agent).map(named),
        hires,
        roads,
        model: account
            .map(|account| account.model.clone())
            .filter(|model| !model.is_empty())
            .unwrap_or_else(|| "its hamlet's model".into()),
        spent: account
            .map(|account| account.cost.clone())
            .unwrap_or_default(),
        lineage,
    }
}

/// A line of the sheet: what it is, and the value beside it.
fn fact(name: &'static str, value: impl View + 'static) -> AnyView {
    row((label(name).tone(Tone::Faint).wrap(false), value))
        .gap(8.0)
        .align(AlignItems::Center)
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

fn names(list: &[(Entity, String)]) -> AnyView {
    if list.is_empty() {
        return dim("none").boxed();
    }
    column(
        list.iter()
            .cloned()
            .map(|(entity, name)| link(entity, name))
            .collect::<Vec<_>>(),
    )
    .boxed()
}

fn agent_section(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| facts(world, agent))
            .when(every_frame()),
        |facts| {
            let mut parts = vec![
                heading(Glyph::Robot, "AGENT"),
                fact("Kind", label(facts.kind)),
                said(facts.purpose.clone()),
            ];
            if let Some((scholar, name)) = &facts.scholar {
                let scholar = *scholar;
                parts.push(fact(
                    "Scholar",
                    lettered(name.clone())
                        .rules(back_link)
                        .tagged(Variant::Text)
                        .on_activate(move |world| {
                            show(world, Showing::Scholar(scholar));
                        }),
                ));
            }
            if let Some((lead, name)) = &facts.lead {
                parts.push(fact("Lead", link(*lead, name.clone())));
            }
            if !facts.hires.is_empty() {
                parts.push(fact("Hires", names(&facts.hires)));
            }
            parts.push(fact("Roads", names(&facts.roads)));
            parts.push(fact("Model", label(facts.model.clone())));
            if !facts.spent.is_empty() {
                parts.push(fact("Spent", label(facts.spent.clone())));
            }
            column(parts).gap(6.0).boxed()
        },
    )
    .within(bare())
}

fn lineage_section(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| facts(world, agent).lineage)
            .when(every_frame()),
        |lineage| {
            let steps = lineage
                .iter()
                .cloned()
                .enumerate()
                .flat_map(|(at, (entity, name))| {
                    let mut step = Vec::new();
                    if at > 0 {
                        step.push(
                            label("->").tone(Tone::Dim).boxed(),
                        );
                    }
                    step.push(link(entity, name));
                    step
                })
                .collect::<Vec<_>>();
            column((
                heading(Glyph::Compass, "LINEAGE"),
                row(steps)
                    .gap(6.0)
                    .align(AlignItems::Center)
                    .tagged(hamlet_ui::widgets::platform::Wraps),
            ))
            .gap(6.0)
            .boxed()
        },
    )
    .within(bare())
}

/// Its own skills and its character, which a chief may edit.
fn skills_and_character() -> impl View {
    let skills = keyed(
        resource::<Papers, _>(|p| p.skills.clone()),
        |skills| {
            if skills.is_empty() {
                return note("No skills of its own yet.").boxed();
            }
            column(
                skills
                    .iter()
                    .map(|(name, does)| {
                        crate::lists::item(
                            name.clone(),
                            does.clone(),
                            Tone::Body,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .gap(4.0)
            .boxed()
        },
    )
    .within(bare());
    let character = keyed(
        resource::<Papers, _>(|p| p.character.clone()),
        |text| {
            if text.is_empty() {
                note("Write here how this agent behaves and speaks.")
                    .boxed()
            } else {
                hamlet_ui::markdown::fixed(text, Tone::Body).boxed()
            }
        },
    )
    .within(bare());
    let edit =
        action(Glyph::Rename, "EDIT", super::papers::edit_character);
    let failed = keyed(
        resource::<Papers, _>(|p| p.failed.clone()),
        |failed| match failed {
            Some(why) => {
                label(why.clone()).tone(Tone::Critical).boxed()
            }
            None => column(()).boxed(),
        },
    )
    .within(bare());
    column((
        heading(Glyph::Book, "SKILLS"),
        skills,
        heading(Glyph::Robot, "CHARACTER"),
        character,
        row((edit,)),
        failed,
    ))
    .gap(6.0)
}

/// The prompt, the sessions and the reflection, as buttons.
fn pages() -> impl View {
    row((
        action(Glyph::History, "SESSIONS", |world| {
            show(world, Showing::Sessions);
        }),
        action(Glyph::Lens, "PROMPT", super::thread::show_prompt),
        action(Glyph::Notes, "REFLECT NOW", |world| {
            if let Some(to) = super::papers::selected(world) {
                world.trigger(
                    hamlet_client::messages::ChiefMessage {
                        to,
                        text: "/reflect".into(),
                    },
                );
            }
        }),
    ))
    .gap(8.0)
    .tagged(hamlet_ui::widgets::platform::Wraps)
}

/// The sheet beside the thread of `agent`.
pub(super) fn sheet(agent: Entity) -> impl View {
    with_cx(move |cx| {
        let line = cx.theme().palette.gold_6;
        let gap = cx.theme().space.section;
        column((scroll_area((column((
            agent_section(agent),
            pages(),
            hairline(line),
            heading(Glyph::Notes, "MEMORY"),
            memory::view(),
            hairline(line),
            skills_and_character(),
            hairline(line),
            lineage_section(agent),
            hairline(line),
            heading(Glyph::Talk, "SIDE CHATS"),
            conversations::list(),
            note("Letters and questions with other agents."),
        ))
        .gap(gap),)),))
        .width(px(WIDTH))
        .fills_height()
    })
}
