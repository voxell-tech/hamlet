//! One hamlet's page of the Inspector: what it is, what can be done
//! to it, and its agents, its bylaws and its record as tabs.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_client::realm::{Deeds, HOME};
use hamlet_realm::orders;
use hamlet_realm::pick::{Thing, select};

use super::heading;
use super::record::record_of;
use super::rows::{
    AgentRow, agent_rows, hamlet_rows, pressable, stale,
};
use crate::feel::Mood;
use crate::icons::Glyph;
use crate::looks::{
    Fills, action, back, bad_action, bare, beside, brass, buttons,
    group, lamp, lettered, note, scroll_area, stated_large, title,
    well,
};
use crate::me::talk_to;
use crate::roll::{either, moved};
use crate::world_menu::Deed;
use crate::{AnyView, View, chat, tabs, with_cx, world_menu};

/// The field a hamlet's new name is typed in.
#[derive(Component)]
pub(crate) struct NameField;

/// The hamlet that is being given a new name, if one is.
#[derive(Resource, Default, PartialEq)]
pub struct Naming(pub(super) Option<Entity>);

/// The words typed in the field under a root marked `M`.
fn typed<M: Component>(world: &mut World) -> String {
    let roots = world
        .query_filtered::<&Children, With<M>>()
        .iter(world)
        .flat_map(|children| children.iter().copied())
        .collect::<Vec<_>>();
    roots
        .into_iter()
        .find_map(|child| world.get::<EditableText>(child))
        .map(|text| text.value().to_string())
        .unwrap_or_default()
}

/// The field and the buttons a hamlet is given a new name with.
fn naming(hamlet: Entity, id: String) -> AnyView {
    keyed(
        resource::<Naming, _>(move |n| n.0 == Some(hamlet)),
        move |open| {
            if !*open {
                return column(()).boxed();
            }
            let id = id.clone();
            with_cx(move |cx| {
                row((
                    text_field(String::new(), |_, _| {})
                        .grow(1.0)
                        .rules(well)
                        .tagged(NameField),
                    lettered("SAVE")
                        .rules(lamp)
                        .tagged(Mood::Good)
                        .on_activate(move |world| {
                            let name = typed::<NameField>(world);
                            world.resource_mut::<Naming>().0 = None;
                            chat::send(
                                world,
                                format!("/name-hamlet {id} {name}"),
                            );
                        }),
                    lettered("CANCEL").rules(brass).on_activate(
                        |world| {
                            world.resource_mut::<Naming>().0 = None;
                        },
                    ),
                ))
                .gap(cx.theme().space.gap)
                .align(AlignItems::Center)
            })
        },
    )
    .within(bare())
    .boxed()
}

/// The page of a hamlet that is shown.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    #[default]
    Agents,
    Bylaws,
    Record,
}

const PAGES: [(Page, Glyph, &str); 3] = [
    (Page::Agents, Glyph::Robot, "AGENTS"),
    (Page::Bylaws, Glyph::Book, "BYLAWS"),
    (Page::Record, Glyph::History, "RECORD"),
];

/// Everyone in `hamlet`, each pressed to talk to.
fn list_of(hamlet: Entity) -> impl View {
    let people =
        derived(move |world: &World| agent_rows(world, Some(hamlet)))
            .when(stale());
    keyed(people, |rows| {
        let card = |row: &AgentRow| {
            let agent = row.agent;
            let title = if row.mayor {
                format!("{} (mayor)", row.name.to_uppercase())
            } else {
                row.name.to_uppercase()
            };
            let state = row.state.clone();
            let beside = match (row.gone, row.waiting, row.busy) {
                (true, ..) => (Glyph::Gone, state, Tone::Critical),
                (_, 0, true) => (Glyph::Bolt, state, Tone::Accent),
                (_, 0, false) => (Glyph::Moon, state, Tone::Dim),
                (_, n, _) => (
                    Glyph::Bell,
                    format!("Waits for you ({n})"),
                    Tone::Critical,
                ),
            };
            let gone = row.gone;
            let thing = if gone {
                Thing::Ruin(agent)
            } else {
                Thing::Agent(agent)
            };
            pressable(
                thing,
                title,
                beside,
                vec![row.purpose.clone(), row.account.clone()],
                move |world| {
                    if gone {
                        select(world, Some(thing));
                    } else {
                        talk_to(world, agent);
                    }
                },
            )
        };
        let living = rows
            .iter()
            .filter(|row| !row.gone)
            .map(card)
            .collect::<Vec<_>>();
        let gone = rows
            .iter()
            .filter(|row| row.gone)
            .map(card)
            .collect::<Vec<_>>();
        let mut parts = vec![
            heading(
                Glyph::Robot,
                format!("AGENTS ({})", living.len()),
            ),
            group(living).boxed(),
        ];
        if !gone.is_empty() {
            parts.push(heading(
                Glyph::Gone,
                format!("GONE ({})", gone.len()),
            ));
            parts.push(group(gone).boxed());
        }
        group(parts).boxed()
    })
    .within(bare())
}

/// One hamlet: what it is, what can be done to it, and its pages.
pub fn hamlet(hamlet: Entity) -> AnyView {
    let facts = derived(move |world: &World| {
        hamlet_rows(world)
            .into_iter()
            .find(|row| row.hamlet == hamlet)
            .map(|facts| {
                let has = |deed| Deeds::is_allowed(world, deed);
                (
                    facts,
                    has("name-hamlet"),
                    has("move-hamlet"),
                    has("remove-hamlet"),
                )
            })
    })
    .when(either(stale(), moved::<Deeds>()));
    let head = keyed(facts, move |facts| {
        let Some((facts, rename, carry, close)) = facts else {
            return column(()).boxed();
        };
        let (mark, state, tone) = facts.state();
        let id = facts.id.clone();
        let known = if facts.title == facts.id {
            String::new()
        } else {
            format!("Known to the server as {}", facts.id)
        };
        let mut actions = vec![
            back("ALL HAMLETS", |world| select(world, None)).boxed(),
        ];
        if *rename {
            actions.push(action(
                Glyph::Rename,
                "RENAME",
                move |world| {
                    world.resource_mut::<Naming>().0 = Some(hamlet);
                },
            ));
        }
        if *carry {
            actions.push(action(Glyph::Move, "MOVE", move |world| {
                orders::carry(world, hamlet);
            }));
        }
        if *close && facts.id != HOME {
            actions.push(bad_action(
                Glyph::Gone,
                "CLOSE DOWN",
                move |world| {
                    world_menu::ask(world, Deed::Close(id.clone()));
                },
            ));
        }
        group((
            beside((
                title(facts.title.to_uppercase()),
                stated_large(mark, state, tone),
            )),
            note(facts.account.clone()),
            note(known),
            buttons(actions),
            naming(hamlet, facts.id.clone()),
            crate::folder::section(hamlet),
        ))
        .boxed()
    })
    .within(bare());
    let pages = tabs::strip(&PAGES, |world, page| {
        *world.resource_mut::<Page>() = page;
    });
    let page =
        keyed(resource::<Page, _>(|page| *page), move |page| {
            match page {
                Page::Agents => list_of(hamlet).boxed(),
                Page::Bylaws => crate::limits::panel(hamlet),
                Page::Record => record_of(hamlet).boxed(),
            }
        })
        .within(bare());
    with_cx(move |cx| {
        column((head, pages, scroll_area((page,))))
            .fills_height()
            .gap(cx.theme().space.section)
    })
}
