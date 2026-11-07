//! One hamlet's page of the Hub: what it is, what can be done to it,
//! and its agents, its bylaws and its record as tabs.

use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, column, dropdown, frame,
    label, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_client::realm::Deeds;
use hamlet_realm::orders::{self, Decree};
use hamlet_realm::pick::{Thing, select};
use hamlet_ui::styles::{
    self, Fills, ICON, action, bad_action, bare, beside, breadcrumb,
    buttons, group, lettered, note, primary_button, scroll_area,
    small_rules, text_input, title,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::tabs;
use hamlet_ui::{AnyView, View, built, with_cx};

use super::heading;
use super::record::record_of;
use super::rows::{
    AgentRow, agent_rows, hamlet_rows, pressable, stale,
};
use crate::convo::talk_to;
use crate::forms::{self, Asking};
use crate::roll::{either, moved};
use crate::world_menu;
use crate::world_menu::Deed;

/// The field a hamlet's new name is typed in.
#[derive(Component)]
pub(crate) struct NameField;

/// The hamlet that is being given a new name, if one is.
#[derive(Resource, Default, PartialEq)]
pub struct Naming(pub(super) Option<Entity>);

/// The words typed in the field under a root marked `M`.
pub(crate) fn typed<M: Component>(world: &mut World) -> String {
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
                        .rules(text_input)
                        .tagged(NameField),
                    lettered("Save")
                        .rules(primary_button)
                        .tagged(Variant::Primary)
                        .on_activate(move |world| {
                            let name = typed::<NameField>(world);
                            world.resource_mut::<Naming>().0 = None;
                            decree(
                                world,
                                &id,
                                format!("/name-hamlet {id} {name}"),
                            );
                        }),
                    lettered("Cancel")
                        .rules(styles::button)
                        .on_activate(|world| {
                            world.resource_mut::<Naming>().0 = None;
                        }),
                ))
                .gap(cx.theme().space.gap)
                .align(AlignItems::Center)
            })
            .boxed()
        },
    )
    .within(bare())
    .boxed()
}

/// Gives the chief's command `text` to the hamlet `id`.
fn decree(world: &mut World, id: &str, text: String) {
    world.trigger(Decree {
        hamlet: Some(id.to_string()),
        text,
    });
    world.flush();
}

/// The tiers a hamlet may hold, in hires.
const TIERS: [&str; 4] = ["50", "100", "200", "500"];

/// How many hires the hamlet `id` may hold, chosen from its tiers.
fn size(allowed: bool, id: String) -> AnyView {
    if !allowed {
        return column(()).boxed();
    }
    let names = TIERS
        .iter()
        .map(|tier| format!("Holds up to {tier} hires"))
        .collect::<Vec<_>>();
    row((
        label("Size").tone(Tone::Faint).wrap(false),
        dropdown(
            names,
            TIERS.len(),
            Glyph::Chevron.image(ICON),
            move |world, chosen| {
                if let Some(tier) = TIERS.get(chosen) {
                    decree(world, &id, format!("/tier {tier}"));
                }
            },
        )
        .placeholder("Choose how many hires it holds"),
    ))
    .gap(8.0)
    .align(AlignItems::Center)
    .boxed()
}

/// The form a worker is started in the hamlet `id` with.
fn worker_form(hamlet: Entity, id: String) -> AnyView {
    forms::form(
        Asking::Worker(hamlet),
        "Start a worker on one task in this hamlet. It answers once \
         and ends.",
        &[("Task", "")],
        "Start",
        move |world, typed| {
            let [task] = typed else {
                return false;
            };
            if task.is_empty() {
                return false;
            }
            decree(world, &id, format!("/worker {task}"));
            true
        },
    )
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
    (Page::Agents, Glyph::Robot, "Agents"),
    (Page::Bylaws, Glyph::Book, "Bylaws"),
    (Page::Record, Glyph::History, "Record"),
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
                format!("{} (mayor)", row.name)
            } else {
                row.name.clone()
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
        // Each lead's hires stand a step in from it, joined to it by
        // the tree's lines.
        let living = rows
            .iter()
            .filter(|row| !row.gone)
            .cloned()
            .collect::<Vec<_>>();
        let living = branches(&living)
            .into_iter()
            .zip(&living)
            .enumerate()
            .map(|(at, (lines, row))| {
                with_lines(lines, card(row).boxed(), at > 0).boxed()
            })
            .collect::<Vec<_>>();
        let gone = rows
            .iter()
            .filter(|row| row.gone)
            .map(|row| {
                column((card(row),)).padding(UiRect::left(px(
                    row.depth as f32 * STEP,
                )))
            })
            .collect::<Vec<_>>();
        // Under the tab that names them. The rows hold the gap over
        // them, so the lines run on through it.
        let mut parts = vec![column(living).gap(0.0).boxed()];
        if !gone.is_empty() {
            parts.push(heading(
                Glyph::Gone,
                format!("Gone ({})", gone.len()),
            ));
            parts.push(group(gone).boxed());
        }
        group(parts).boxed()
    })
    .within(bare())
}

/// How far a hire stands in from its lead.
const STEP: f32 = 18.0;

/// The tree's lines in front of one agent's card.
struct Branch {
    /// For each level between the top and its lead's, whether a
    /// guide runs down it past this card.
    rails: Vec<bool>,
    /// Whether it is the last of its lead's hires.
    last: bool,
    /// Whether it is under a lead at all.
    nested: bool,
}

/// The lines in front of each of `rows`, which are in the tree's
/// order.
fn branches(rows: &[AgentRow]) -> Vec<Branch> {
    // Whether the agent last seen at each depth has more after it.
    let mut more = Vec::<bool>::new();
    rows.iter()
        .enumerate()
        .map(|(at, row)| {
            let depth = row.depth;
            let last = rows[at + 1..]
                .iter()
                .take_while(|after| after.depth >= depth)
                .all(|after| after.depth > depth);
            let rails =
                more.get(1..depth).unwrap_or_default().to_vec();
            more.resize(depth + 1, false);
            more[depth] = !last;
            Branch {
                rails,
                last,
                nested: depth > 0,
            }
        })
        .collect()
}

/// `card` behind the tree's lines, with the gap to the card before it
/// over it when there is one, so that the lines run on through it up
/// to the lead. Each guide runs down the middle of a step, and the
/// elbow meets the card at its title.
fn with_lines(
    branch: Branch,
    card: AnyView,
    after: bool,
) -> impl View {
    built(move |cx| {
        let theme = cx.theme();
        let colour = theme.palette.gold_6;
        let over = if after { theme.space.gap } else { 0.0 };
        let elbow = over
            + theme.space.border
            + theme.space.card_padding_y
            + theme.text.heading * 0.6;
        let line = |on: bool| {
            frame().width(px(1.0)).fill(if on {
                colour
            } else {
                Color::NONE
            })
        };
        let guide = |on: bool| {
            row((frame().width(px(STEP / 2.0)), line(on)))
                .width(px(STEP))
                .align(AlignItems::Stretch)
                .boxed()
        };
        let mut parts = branch
            .rails
            .iter()
            .map(|on| guide(*on))
            .collect::<Vec<_>>();
        if branch.nested {
            parts.push(
                row((
                    frame().width(px(STEP / 2.0)),
                    column((
                        line(true).height(px(elbow)),
                        line(!branch.last).grow(1.0),
                    ))
                    .gap(0.0),
                    column((frame()
                        .width(px(STEP / 2.0 - 1.0))
                        .height(px(1.0))
                        .fill(colour),))
                    .padding(UiRect::top(px(elbow))),
                ))
                .gap(0.0)
                .align(AlignItems::Stretch)
                .boxed(),
            );
        }
        parts.push(
            column((card,))
                .grow(1.0)
                .min_width(px(0.0))
                .padding(UiRect::top(px(over)))
                .boxed(),
        );
        cx.build(row(parts).gap(0.0).align(AlignItems::Stretch))
    })
}

/// One hamlet: what it is, what can be done to it, and its pages.
pub fn hamlet(hamlet: Entity) -> impl View {
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
                    (has("hire"), has("worker"), has("tier")),
                )
            })
    })
    .when(either(stale(), moved::<Deeds>()));
    let head = keyed(facts, move |facts| {
        let Some((
            facts,
            rename,
            carry,
            close,
            (hires, workers, tier),
        )) = facts
        else {
            return column(()).boxed();
        };
        let id = facts.id.clone();
        // What commands call it, when its name says otherwise.
        let known = if facts.title == facts.id {
            String::new()
        } else {
            facts.id.clone()
        };
        let mut actions = Vec::new();
        if *hires {
            // Under its mayor; under another hire from that hire's
            // menu.
            let mayor_of = facts.id.clone();
            actions.push(action(
                Glyph::Robot,
                "Hire a scholar",
                move |world| {
                    if let Some(mayor) =
                        hamlet_client::realm::mayor_of(
                            world, &mayor_of,
                        )
                    {
                        world_menu::ask_hire(world, mayor);
                    }
                },
            ));
        }
        if *workers {
            actions.push(action(
                Glyph::Chip,
                "Start a worker",
                move |world| {
                    forms::toggle(world, Asking::Worker(hamlet));
                },
            ));
        }
        if *rename {
            actions.push(action(
                Glyph::Rename,
                "Rename",
                move |world| {
                    world.resource_mut::<Naming>().0 = Some(hamlet);
                },
            ));
        }
        if *carry {
            actions.push(action(Glyph::Move, "Move", move |world| {
                orders::carry(world, hamlet);
            }));
        }
        if *close {
            actions.push(bad_action(
                Glyph::Gone,
                "Close down",
                move |world| {
                    world_menu::ask(world, Deed::Close(id.clone()));
                },
            ));
        }
        group((
            breadcrumb(
                vec!["Realm".into()],
                facts.title.clone(),
                |world, _| {
                    select(world, None);
                    crate::hub::show(
                        world,
                        crate::hub::Showing::Realm,
                    );
                },
            ),
            beside((
                title(facts.title.clone()),
                label(known)
                    .tone(Tone::Faint)
                    .rules(small_rules)
                    .tooltip(|| note("What the server calls it")),
            )),
            note(facts.account.clone()),
            buttons(actions),
            size(*tier, facts.id.clone()),
            worker_form(hamlet, facts.id.clone()),
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
