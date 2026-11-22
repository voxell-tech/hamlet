//! One hamlet's page of the Hub.

use std::f32::consts::TAU;

use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{
    AlignItems, AngularColorStop, BorderGradient, ConicGradient,
    JustifyContent, PositionType, UiPosition, UiRect, Val, px,
};
use bevy_fynix::tokens::{MotionTokens, SurfaceTokens, Tone};
use bevy_fynix::views::{
    self, BehaviorExt, FrameProps, TooltipExt, button, column,
    dropdown, frame, label, row,
};
use bevy_fynix::{
    Binding, ScopedExt, ViewExt, component, derived, every_frame,
    keyed, style,
};
use hamlet_client::realm::{Actions, Works};
use hamlet_realm::orders::Decree;
use hamlet_realm::pick::{Thing, select};
use hamlet_ui::styles::{
    Fills, ICON, LINE, action, bare, buttons, crumb, crumb_slash,
    crumbs, group, more_button, note, note_rules, raised,
    scroll_area, status_icon, transparent,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::forms::{self, Asking};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::widgets::rainbow::rainbow_bar;
use hamlet_ui::widgets::tabs::{self, Spread};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::heading;
use super::record::record_of;
use super::rows::{AgentRow, Meter, agent_rows, hamlet_rows, stale};
use crate::hub::Showing;
use crate::roll::{either, moved};
use crate::{profile_tag, words, world_menu};

/// What the form of a hamlet's worker asks.
const WORKER: &str = "worker";

/// Gives the chief's command `text` to the hamlet `id`.
fn decree(world: &mut World, id: &str, text: String) {
    world.trigger(Decree {
        hamlet: Some(id.to_string()),
        text,
    });
    world.flush();
}

/// The tiers a hamlet may hold, in agents.
const TIERS: [&str; 4] = ["50", "100", "200", "500"];

/// How many agents the hamlet `id` may hold, chosen from its tiers.
fn size(hamlet: Entity, id: String) -> AnyView {
    let names = TIERS
        .iter()
        .map(|tier| format!("{tier} agents"))
        .collect::<Vec<_>>();
    row((
        label("Size").tone(Tone::Faint).wrap(false),
        dropdown(
            names,
            Binding::new(
                derived(move |world: &World| {
                    let tier = world
                        .get::<Works>(hamlet)
                        .map(|works| works.tier.to_string());
                    TIERS
                        .iter()
                        .position(|t| Some(*t) == tier.as_deref())
                        .unwrap_or(TIERS.len())
                })
                .when(every_frame()),
                move |world, chosen: usize| {
                    if let Some(tier) = TIERS.get(chosen) {
                        decree(world, &id, format!("/tier {tier}"));
                    }
                },
            ),
            Glyph::Chevron.image(ICON),
        )
        .placeholder("Choose how many agents it holds"),
    ))
    .gap(8.0)
    .align(AlignItems::Center)
    .boxed()
}

/// The form a worker is started in the hamlet `id` with.
fn worker_form(hamlet: Entity, id: String) -> AnyView {
    forms::form(
        Asking::about(WORKER, hamlet),
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
#[derive(Component, Default, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    #[default]
    Agents,
    Limits,
    History,
}

/// The tabs of `hub`'s hamlet pages.
fn pages_of(
    hub: Entity,
) -> [((Entity, Page), Glyph, &'static str); 3] {
    [
        ((hub, Page::Agents), Glyph::Robot, "Agents"),
        ((hub, Page::Limits), Glyph::Book, "Limits"),
        ((hub, Page::History), Glyph::History, "History"),
    ]
}

/// Turns `hub`'s hamlet page to `page`.
fn turn_to(world: &mut World, hub: Entity, page: Page) {
    if let Some(mut now) = world.get_mut::<Page>(hub) {
        *now = page;
    }
}

/// Everyone in `hamlet` as a tree, each pressed to talk to in `hub`.
fn list_of(hub: Entity, hamlet: Entity) -> impl View {
    let people =
        derived(move |world: &World| agent_rows(world, Some(hamlet)))
            .when(stale());
    keyed(people, move |rows| {
        let tree = |gone: bool| {
            let rows = rows
                .iter()
                .filter(|row| row.gone == gone)
                .collect::<Vec<_>>();
            branches(&rows)
                .into_iter()
                .zip(rows)
                .map(|(branch, row)| person(hub, row, branch))
                .collect::<Vec<_>>()
        };
        let (living, gone) = (tree(false), tree(true));
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

/// How far an agent stands in from its parent.
const STEP: f32 = 18.0;

/// How far a row's icon stands in from its left edge.
const INSET: f32 = 6.0;

/// The room over a row's first line.
const TOP: f32 = 4.0;

/// The tree's lines in front of one agent's row.
struct Branch {
    /// For each level above its parent's, whether a guide runs past.
    rails: Vec<bool>,
    /// Whether it is the last of its parent's agents.
    last: bool,
    /// Whether it has agents of its own under it.
    has_children: bool,
}

/// The lines in front of each of `rows`, in tree order.
fn branches(rows: &[&AgentRow]) -> Vec<Branch> {
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
            let has_children = rows
                .get(at + 1)
                .is_some_and(|next| next.depth > depth);
            Branch {
                rails,
                last,
                has_children,
            }
        })
        .collect()
}

/// The guides and elbow in front of a row at `depth`.
fn lines(cx: &Cx, depth: usize, branch: &Branch) -> Vec<AnyView> {
    let colour = cx.theme().palette.hairline;
    let shown = |on: bool| if on { colour } else { Color::NONE };
    // Where the elbow meets the row: the middle of its first line.
    let elbow = TOP + cx.theme().text.body * 0.6;
    let step = |inside: AnyView| {
        row((inside,))
            .width(px(STEP))
            .shrink(0.0)
            .padding(UiRect::left(
                px(INSET + ICON / 2.0 - LINE / 2.0),
            ))
            .align(AlignItems::Stretch)
            .boxed()
    };
    let mut parts = branch
        .rails
        .iter()
        .map(|on| {
            step(frame().width(px(LINE)).fill(shown(*on)).boxed())
        })
        .collect::<Vec<_>>();
    // A line over the row; one with no height runs to its foot.
    let over = |left: f32,
                top: f32,
                width: f32,
                height: Option<f32>| {
        frame()
            .width(px(width))
            .height(height.map_or(Val::Auto, px))
            .fill(colour)
            .position(PositionType::Absolute)
            .inset(UiRect::new(
                px(left),
                Val::Auto,
                px(top),
                if height.is_some() { Val::Auto } else { px(0.0) },
            ))
            .boxed()
    };
    // Where the line under a row's icon runs, at `depth`.
    let under_icon = |depth: usize| {
        depth as f32 * STEP + INSET + ICON / 2.0 - LINE / 2.0
    };
    if depth > 0 {
        parts.push(step(
            column((
                frame()
                    .width(px(LINE))
                    .height(px(elbow + LINE))
                    .fill(colour),
                frame()
                    .width(px(LINE))
                    .grow(1.0)
                    .fill(shown(!branch.last)),
            ))
            .gap(0.0)
            .boxed(),
        ));
        // The elbow's foot, up to just short of the icon.
        let from = under_icon(depth - 1);
        let to = depth as f32 * STEP + INSET - 2.0;
        parts.push(over(from, elbow, to - from, Some(LINE)));
    }
    if branch.has_children {
        // Down from under its icon to its first agent.
        let top = elbow + ICON / 2.0 + 2.0;
        parts.push(over(under_icon(depth), top, LINE, None));
    }
    parts
}

/// One agent as a row of the list in `hub`.
fn person(hub: Entity, row_of: &AgentRow, branch: Branch) -> AnyView {
    let agent = row_of.agent;
    let gone = row_of.gone;
    let thing = if gone {
        Thing::Ruin(agent)
    } else {
        Thing::Agent(agent)
    };
    let kind = if row_of.lead {
        Glyph::Lead
    } else if row_of.worker {
        Glyph::Robot
    } else {
        Glyph::HamletAgent
    };
    let standing = row_of.standing;
    let (name, tag, under, account, depth) = (
        row_of.name.clone(),
        row_of.tag.clone(),
        row_of.under.clone(),
        row_of.account.clone(),
        row_of.depth,
    );
    with_cx(move |cx| {
        let mut parts = lines(cx, depth, &branch);
        let space = &cx.theme().space;
        let mut head = vec![
            raised(glyph(kind, ICON).tone(Tone::Dim)).boxed(),
            label(name.clone()).bold(true).wrap(false).boxed(),
        ];
        head.extend(tag.as_ref().map(profile_tag::tag));
        head.extend([
            status_icon(
                standing.glyph(),
                standing.word(),
                standing.kind(),
            )
            .boxed(),
            frame().grow(1.0).boxed(),
            label(account.clone())
                .wrap(false)
                .rules(note_rules)
                .boxed(),
        ]);
        let mut words = vec![
            row(head)
                .gap(space.gap)
                .align(AlignItems::Center)
                .boxed(),
        ];
        if !under.is_empty() {
            // Under the name, past the icon.
            words.push(
                row(
                    (one_line(label(under.clone()).tone(Tone::Dim)),),
                )
                .padding(UiRect::left(px(ICON + space.gap)))
                .min_width(px(0.0))
                .boxed(),
            );
        }
        parts.push(
            button(
                column(words).gap(2.0).grow(1.0).min_width(px(0.0)),
            )
            .padding(UiRect::new(
                px(INSET),
                px(6.0),
                px(TOP),
                px(4.0),
            ))
            .grow(1.0)
            .min_width(px(0.0))
            .rules(row_look)
            .tagged(Variant::Default)
            .tagged(world_menu::Asks(thing))
            .on_activate(move |world| {
                if gone {
                    crate::hub::show(
                        world,
                        hub,
                        Showing::Gone(agent),
                    );
                    crate::hub::look_at(world, hub, thing);
                } else {
                    crate::hub::talk_in(world, hub, agent);
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
        .frame(|f, t| {
            f.fill(transparent(t.palette.button))
                .radius(t.space.round)
        })
        .hovered(|s| s.frame(|f, t| f.fill(t.palette.button)))
        .pressed(|s| s.frame(|f, t| f.fill(t.pressed())))
        .transition(HamletTheme::interact)
        .apply(cx);
}

/// The width of the ring of a hamlet's budget, and how thick it is.
const METER: f32 = 16.0;
const RING: f32 = 4.0;

/// The share of its budget from which a hamlet's circle turns yellow.
const NEAR_CAP: f32 = 0.8;
/// The least of its circle that spending shows.
const MIN_SHOWN: f32 = 0.06;

/// The width of the bar of a budget with no limit.
const UNLIMITED: f32 = 28.0;

/// What the hamlet spent: a ring under a cap, else a bar, then
/// `words`.
pub(super) fn budget(words: String, spent: Option<Meter>) -> AnyView {
    with_cx(move |cx| {
        let palette = &cx.theme().palette;
        let mut parts = Vec::new();
        if spent == Some(Meter::Unlimited) {
            parts.push(rainbow_bar(UNLIMITED, RING + 2.0).boxed());
        }
        if let Some(Meter::Share(spent)) = spent {
            let fill = if spent >= 1.0 {
                palette.status.danger.solid
            } else if spent >= NEAR_CAP {
                palette.status.warning.solid
            } else {
                palette.accent
            };
            // Whatever is spent shows, if only as a sliver.
            let shown = if spent > 0.0 {
                spent.clamp(MIN_SHOWN, 1.0)
            } else {
                0.0
            };
            let turn = shown * TAU;
            let ring = ConicGradient::new(
                UiPosition::CENTER,
                vec![
                    AngularColorStop::new(fill, 0.0),
                    AngularColorStop::new(fill, turn),
                    AngularColorStop::new(palette.hairline, turn),
                    AngularColorStop::new(palette.hairline, TAU),
                ],
            );
            parts.push(
                frame()
                    .width(px(METER))
                    .height(px(METER))
                    .shrink(0.0)
                    .radius(METER / 2.0)
                    .border(RING)
                    .tagged(BorderGradient::from(ring))
                    .boxed(),
            );
        }
        parts.push(note(words.clone()).boxed());
        row(parts)
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center)
            .tagged(Wraps)
    })
    .boxed()
}

/// One hamlet in `hub`: what it is, what can be done to it, and its
/// pages.
pub fn hamlet(hub: Entity, hamlet: Entity) -> impl View {
    let facts = derived(move |world: &World| {
        hamlet_rows(world)
            .into_iter()
            .find(|row| row.hamlet == hamlet)
            .map(|facts| {
                let has =
                    |command| Actions::is_allowed(world, command);
                (
                    facts,
                    has("move-hamlet"),
                    has("remove-hamlet"),
                    (has("add-agent"), has("worker"), has("tier")),
                )
            })
    })
    .when(either(stale(), moved::<Actions>()));
    let head = keyed(facts, move |facts| {
        let Some((facts, carry, close, (adds, workers, tier))) =
            facts
        else {
            return column(()).boxed();
        };
        let mut actions = Vec::new();
        if *adds {
            // Under its lead; under another agent from its menu.
            let lead_of = facts.id.clone();
            actions.push(action(
                Glyph::Profile,
                "Add Agent",
                move |world| {
                    if let Some(lead) =
                        hamlet_client::realm::lead_of(world, &lead_of)
                    {
                        world_menu::ask_add_agent(world, lead);
                    }
                },
            ));
        }
        if *workers {
            actions.push(action(
                Glyph::Robot,
                "Start a Worker",
                move |world| {
                    forms::toggle(
                        world,
                        Asking::about(WORKER, hamlet),
                    );
                },
            ));
        }
        // Taken up here, it is set down in the world.
        if *carry {
            actions.push(action(
                Glyph::Move,
                "Move It",
                move |world| {
                    hamlet_realm::orders::carry(world, hamlet);
                },
            ));
        }
        // Its menu, as a right click on it opens.
        if *adds || *close {
            actions.push(more_button(
                "Add agents or close it down",
                move |world| {
                    world_menu::open_for(
                        world,
                        Thing::Common(hamlet),
                    );
                },
            ));
        }
        // A double click on its name renames it.
        let path = crumbs(vec![
            crumb("Realm", move |world| {
                if crate::hub::is_following(world, hub) {
                    select(world, None);
                }
                crate::hub::show(world, hub, Showing::Realm);
            })
            .boxed(),
            crumb_slash().boxed(),
            crate::naming::name_crumb(hamlet, facts.title.clone()),
        ])
        .boxed();
        // The meter is pressed to change the budget it shows.
        let spending =
            button(budget(facts.account.clone(), facts.spent))
                .padding(UiRect::all(px(0.0)))
                .justify(JustifyContent::FlexStart)
                .rules(views::tint)
                .tooltip(|| note("Change the budget"))
                .on_activate(move |world| {
                    turn_to(world, hub, Page::Limits);
                    *world.resource_mut::<crate::limits::Part>() =
                        crate::limits::Part::Budget;
                });
        let mut top = vec![path, spending.boxed()];
        // The forms open only from these buttons.
        if !actions.is_empty() {
            top.push(
                group((
                    buttons(actions),
                    worker_form(hamlet, facts.id.clone()),
                ))
                .boxed(),
            );
        }
        if *tier {
            top.push(size(hamlet, facts.id.clone()));
        }
        group(top).boxed()
    })
    .within(bare());
    let pages = tabs::tipped_strip(
        &pages_of(hub),
        &[words::AGENTS, words::LIMITS, words::HISTORY],
        Spread::Shared,
        |world, (hub, page)| turn_to(world, hub, page),
    );
    let page = keyed(
        component::<Page, _>(hub, |page| {
            page.copied().unwrap_or_default()
        }),
        move |page| match page {
            Page::Agents => list_of(hub, hamlet).boxed(),
            Page::Limits => crate::limits::panel(hamlet),
            Page::History => record_of(hamlet).boxed(),
        },
    )
    .within(bare());
    with_cx(move |cx| {
        column((head, pages, scroll_area((page,))))
            .fills_height()
            .gap(cx.theme().space.section)
    })
}
