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
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, label, row, text_field,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, keyed, resource, style,
};
use hamlet_client::realm::Deeds;
use hamlet_realm::orders::{self, Decree};
use hamlet_realm::pick::{Thing, select};
use hamlet_ui::styles::{
    self, Fills, ICON, action, bad_action, bare, breadcrumb, buttons,
    group, lettered, note, note_rules, primary_button, raised,
    scroll_area, status_label, text_input, title,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::tabs;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::heading;
use super::record::record_of;
use super::rows::{AgentRow, agent_rows, hamlet_rows, stale};
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
fn size(id: String) -> AnyView {
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

/// Everyone in `hamlet`, each pressed to talk to, its hires a step
/// in from it as the rail shows them.
fn list_of(hamlet: Entity) -> impl View {
    let people =
        derived(move |world: &World| agent_rows(world, Some(hamlet)))
            .when(stale());
    keyed(people, |rows| {
        let living = rows
            .iter()
            .filter(|row| !row.gone)
            .map(person)
            .collect::<Vec<_>>();
        let gone = rows
            .iter()
            .filter(|row| row.gone)
            .map(person)
            .collect::<Vec<_>>();
        let mut parts = vec![column(living).gap(0.0).boxed()];
        if !gone.is_empty() {
            parts.push(heading(
                Glyph::Gone,
                format!("Gone ({})", gone.len()),
            ));
            parts.push(column(gone).gap(0.0).boxed());
        }
        group(parts).boxed()
    })
    .within(bare())
}

/// How far a hire stands in from its lead.
const STEP: f32 = 18.0;

/// How far a row's icon stands in from its left edge.
const INSET: f32 = 6.0;

/// One agent as a row of the list: its kind, its name and state, what
/// it does under them and its model and spending at the end.
fn person(row_of: &AgentRow) -> AnyView {
    let agent = row_of.agent;
    let gone = row_of.gone;
    let thing = if gone {
        Thing::Ruin(agent)
    } else {
        Thing::Agent(agent)
    };
    let kind = if row_of.mayor {
        Glyph::Mayor
    } else {
        Glyph::Robot
    };
    let standing = row_of.standing;
    let under = if row_of.doing.is_empty() {
        row_of.purpose.clone()
    } else {
        row_of.doing.clone()
    };
    let (name, account, depth) =
        (row_of.name.clone(), row_of.account.clone(), row_of.depth);
    with_cx(move |cx| {
        let space = &cx.theme().space;
        let line = cx.theme().palette.gold_6;
        // A guide down each lead above it, under the lead's icon, so
        // that a lead's rows run together without a gap.
        let mut parts = (0..depth)
            .map(|_| {
                row((frame()
                    .width(px(1.0))
                    .height(percent(100.0))
                    .fill(line),))
                .width(px(STEP))
                .shrink(0.0)
                .padding(UiRect::left(px(INSET + ICON / 2.0 - 0.5)))
                .boxed()
            })
            .collect::<Vec<_>>();
        let words = column((
            row((
                label(name).bold(true).wrap(false),
                status_label(
                    standing.glyph(),
                    standing.word(),
                    standing.tone(),
                ),
            ))
            .gap(space.gap)
            .align(AlignItems::Center),
            one_line(label(under).tone(Tone::Dim)),
        ))
        .gap(1.0)
        .grow(1.0)
        .min_width(px(0.0));
        parts.push(
            button(
                row((
                    raised(glyph(kind, ICON).tone(Tone::Dim)),
                    words,
                    label(account).wrap(false).rules(note_rules),
                ))
                .gap(space.gap)
                .grow(1.0)
                .align(AlignItems::Center),
            )
            .padding(UiRect::new(
                px(INSET),
                px(6.0),
                px(5.0),
                px(5.0),
            ))
            .grow(1.0)
            .min_width(px(0.0))
            .rules(row_look)
            .tagged(Variant::Default)
            .tagged(world_menu::Asks(thing))
            .on_activate(move |world| {
                if gone {
                    select(world, Some(thing));
                } else {
                    talk_to(world, agent);
                }
            })
            .boxed(),
        );
        row(parts).gap(0.0).align(AlignItems::Stretch)
    })
    .boxed()
}

/// A row of the list: bare at rest, a faint fill under the pointer.
fn row_look(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| f.fill(Color::NONE).radius(t.space.round))
        .hovered(|s| s.frame(|f, t| f.fill(t.palette.button)))
        .pressed(|s| s.frame(|f, t| f.fill(t.palette.input)))
        .transition(Motion::Interact)
        .apply(cx);
}

/// The width of the bar of a hamlet's budget.
const METER: f32 = 120.0;

/// What the hamlet spent of its budget, in words and, when it has a
/// cap, as a bar filled as far as `spent` says.
pub(super) fn budget(words: String, spent: Option<f32>) -> AnyView {
    with_cx(move |cx| {
        let palette = &cx.theme().palette;
        let mut parts = vec![note(words.clone()).boxed()];
        if let Some(spent) = spent {
            let fill = if spent >= 1.0 {
                palette.red
            } else {
                palette.accent
            };
            parts.push(
                row((frame()
                    .width(percent(spent * 100.0))
                    .height(percent(100.0))
                    .radius(3.0)
                    .fill(fill),))
                .width(px(METER))
                .height(px(6.0))
                .radius(3.0)
                .fill(palette.gold_6)
                .boxed(),
            );
        }
        row(parts)
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center)
    })
    .boxed()
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
        let mut actions = Vec::new();
        if *hires {
            // Under its mayor; under another hire from that hire's
            // menu.
            let mayor_of = facts.id.clone();
            actions.push(action(
                Glyph::Scholar,
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
                Glyph::Robot,
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
        // What commands call it, when its name says otherwise.
        let name = if facts.title == facts.id {
            title(facts.title.clone()).boxed()
        } else {
            let id = facts.id.clone();
            title(facts.title.clone())
                .tooltip(move || {
                    note(format!("Commands call it {id}"))
                })
                .boxed()
        };
        let mut top = vec![
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
            )
            .boxed(),
            name,
            budget(facts.account.clone(), facts.spent),
        ];
        // The forms open only from these buttons, so without them
        // nothing stands under the account.
        if !actions.is_empty() {
            top.push(
                group((
                    buttons(actions),
                    worker_form(hamlet, facts.id.clone()),
                    naming(hamlet, facts.id.clone()),
                ))
                .boxed(),
            );
        }
        if *tier {
            top.push(size(facts.id.clone()));
        }
        group(top).boxed()
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
