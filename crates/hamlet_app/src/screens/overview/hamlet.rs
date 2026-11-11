//! One hamlet's page of the Hub: what it is, what can be done to it,
//! and its agents, its bylaws and its record as tabs.

use std::f32::consts::TAU;

use bevy::app::{App, Update};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::events::PointerClick;
use bevy::picking::pointer::PointerButton;
use bevy::text::EditableText;
use bevy::ui::{
    AlignItems, AngularColorStop, BorderGradient, ConicGradient,
    PositionType, UiPosition, UiRect, Val, px,
};
use bevy_fynix::tokens::{Motion, SurfaceTokens, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, label, row, text_field,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource, style,
};
use hamlet_client::realm::{Deeds, Works};
use hamlet_realm::orders::Decree;
use hamlet_realm::pick::{Thing, select};
use hamlet_ui::styles::{
    Fills, ICON, action, bare, buttons, crumb, crumb_here,
    crumb_slash, crumbs, group, more_button, note, note_rules,
    raised, reference, scroll_area, status_icon, text_input,
    transparent,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::widgets::tabs::{self, Spread};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::heading;
use super::record::record_of;
use super::rows::{AgentRow, agent_rows, hamlet_rows, stale};
use crate::convo::talk_to;
use crate::dock::{HUB, show_tab};
use crate::forms::{self, Asking};
use crate::roll::{either, moved};
use crate::{words, world_menu};

/// The field a hamlet's new name is typed in.
#[derive(Component)]
pub(crate) struct NameField;

/// The name of `.0` at the end of its path, which a double click
/// renames.
#[derive(Component)]
struct NameCrumb(Entity);

/// The narrowest the field a hamlet's new name is typed in is.
const NAME_FIELD: f32 = 160.0;

/// The hamlet that is being given a new name, if one is.
#[derive(Resource, Default, PartialEq)]
pub struct Naming(pub(super) Option<Entity>);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Naming>()
        .add_observer(rename_twice_clicked)
        .add_systems(
            Update,
            (focus_name, end_naming)
                .chain()
                // Only with a keyboard and its focus, which a
                // headless window has neither of.
                .run_if(resource_exists::<ButtonInput<KeyCode>>)
                .run_if(resource_exists::<InputFocus>),
        );
}

/// Renames the hamlet whose name is clicked twice at the end of its
/// path.
fn rename_twice_clicked(
    click: On<PointerClick>,
    crumbs: Query<&NameCrumb>,
    deeds: Option<Res<Deeds>>,
    mut naming: ResMut<Naming>,
) {
    let Ok(crumb) = crumbs.get(click.event_target()) else {
        return;
    };
    let allowed =
        deeds.is_some_and(|deeds| deeds.is_supported("name-hamlet"));
    if allowed
        && click.button == PointerButton::Primary
        && click.count == 2
    {
        naming.0 = Some(crumb.0);
    }
}

/// Puts the keyboard in a new name field.
fn focus_name(
    fields: Query<&Children, Added<NameField>>,
    texts: Query<(), With<EditableText>>,
    mut focus: ResMut<InputFocus>,
) {
    for children in &fields {
        if let Some(&text) =
            children.iter().find(|&&kid| texts.contains(kid))
        {
            focus.set(text, FocusCause::Navigated);
        }
    }
}

/// Whether the keyboard is in a name field, as it is when Enter
/// commits one: a press elsewhere takes it out first.
fn in_name_field(world: &World) -> bool {
    world
        .get_resource::<InputFocus>()
        .and_then(InputFocus::get)
        .and_then(|input| world.get::<ChildOf>(input))
        .is_some_and(|parent| {
            world.get::<NameField>(parent.parent()).is_some()
        })
}

/// Leaves the name being typed on Escape, or once the keyboard is
/// elsewhere.
fn end_naming(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    fields: Query<Entity, With<NameField>>,
    parents: Query<&ChildOf>,
    mut naming: ResMut<Naming>,
) {
    if naming.0.is_none() {
        return;
    }
    let Ok(field) = fields.single() else {
        return;
    };
    let inside = focus.get().is_some_and(|input| {
        input == field
            || parents
                .iter_ancestors(input)
                .any(|above| above == field)
    });
    if keys.just_pressed(KeyCode::Escape) || !inside {
        naming.0 = None;
    }
}

/// Brings the Hub to the front at the page of `hamlet`, with the
/// field its new name is typed in.
pub(crate) fn rename(world: &mut World, hamlet: Entity) {
    crate::hub::show(world, crate::hub::Showing::Hamlet(hamlet));
    show_tab(world, HUB);
    world.resource_mut::<Naming>().0 = Some(hamlet);
}

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

/// The name `title` at the end of the path to `hamlet`, or, while it
/// is renamed, the field its new name is typed in: Enter keeps it,
/// Escape or a press elsewhere drops it.
fn naming(hamlet: Entity, id: String, title: String) -> AnyView {
    keyed(
        resource::<Naming, _>(move |n| n.0 == Some(hamlet)),
        move |open| {
            if !*open {
                return crumb_here(title.clone())
                    .tagged(NameCrumb(hamlet))
                    .boxed();
            }
            let id = id.clone();
            text_field(title.clone(), move |world, name| {
                if !in_name_field(world) {
                    return;
                }
                world.resource_mut::<Naming>().0 = None;
                let name = name.trim();
                if !name.is_empty() {
                    decree(
                        world,
                        &id,
                        format!("/name-hamlet {id} {name}"),
                    );
                }
            })
            .min_width(px(NAME_FIELD))
            .rules(text_input)
            .tagged(NameField)
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

/// How many hires the hamlet `id` may hold, chosen from its tiers,
/// showing the one it holds now.
fn size(hamlet: Entity, id: String) -> AnyView {
    let names = TIERS
        .iter()
        .map(|tier| format!("{tier} hires"))
        .collect::<Vec<_>>();
    row((
        label("Size").tone(Tone::Faint).wrap(false),
        dropdown(
            names,
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
        let tree = |gone: bool| {
            let rows = rows
                .iter()
                .filter(|row| row.gone == gone)
                .collect::<Vec<_>>();
            branches(&rows)
                .into_iter()
                .zip(rows)
                .map(|(branch, row)| person(row, branch))
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

/// How far a hire stands in from its lead.
const STEP: f32 = 18.0;

/// How far a row's icon stands in from its left edge.
const INSET: f32 = 6.0;

/// The room over a row's first line.
const TOP: f32 = 4.0;

/// The tree's lines in front of one agent's row.
struct Branch {
    /// For each level between the top and its lead's, whether a
    /// guide runs down it past this row.
    rails: Vec<bool>,
    /// Whether it is the last of its lead's hires.
    last: bool,
    /// Whether it has hires of its own under it.
    leads: bool,
}

/// The lines in front of each of `rows`, which are in the tree's
/// order.
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
            let leads = rows
                .get(at + 1)
                .is_some_and(|next| next.depth > depth);
            Branch { rails, last, leads }
        })
        .collect()
}

/// The lines in front of a row at `depth`: a guide down each level
/// above its lead's that has more rows after it, then an elbow from
/// its lead to its icon, running on down while its lead has more.
/// Each line runs under the icon of the lead it comes from.
fn lines(cx: &Cx, depth: usize, branch: &Branch) -> Vec<AnyView> {
    let colour = cx.theme().palette.gold_6;
    let shown = |on: bool| if on { colour } else { Color::NONE };
    // Where the elbow meets the row: the middle of its first line.
    let elbow = TOP + cx.theme().text.body * 0.6;
    let step = |inside: AnyView| {
        row((inside,))
            .width(px(STEP))
            .shrink(0.0)
            .padding(UiRect::left(px(INSET + ICON / 2.0 - 0.5)))
            .align(AlignItems::Stretch)
            .boxed()
    };
    let mut parts = branch
        .rails
        .iter()
        .map(|on| {
            step(frame().width(px(1.0)).fill(shown(*on)).boxed())
        })
        .collect::<Vec<_>>();
    // A line laid over the row, `left` and `top` in from its edges;
    // one with no height runs to its foot.
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
    let under_icon =
        |depth: usize| depth as f32 * STEP + INSET + ICON / 2.0 - 0.5;
    if depth > 0 {
        parts.push(step(
            column((
                frame()
                    .width(px(1.0))
                    .height(px(elbow + 1.0))
                    .fill(colour),
                frame()
                    .width(px(1.0))
                    .grow(1.0)
                    .fill(shown(!branch.last)),
            ))
            .gap(0.0)
            .boxed(),
        ));
        // The elbow's foot, up to just short of the icon.
        let from = under_icon(depth - 1);
        let to = depth as f32 * STEP + INSET - 2.0;
        parts.push(over(from, elbow, to - from, Some(1.0)));
    }
    if branch.leads {
        // Down from under its icon to its first hire.
        let top = elbow + ICON / 2.0 + 2.0;
        parts.push(over(under_icon(depth), top, 1.0, None));
    }
    parts
}

/// One agent as a row of the list: its kind, its name and state, and
/// its own model and spending at the end of that line, then what its
/// row says under them, behind the tree's lines.
fn person(row_of: &AgentRow, branch: Branch) -> AnyView {
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
    let (name, under, account, depth) = (
        row_of.name.clone(),
        row_of.under.clone(),
        row_of.account.clone(),
        row_of.depth,
    );
    with_cx(move |cx| {
        let mut parts = lines(cx, depth, &branch);
        let space = &cx.theme().space;
        let mut words = vec![
            row((
                raised(glyph(kind, ICON).tone(Tone::Dim)),
                label(name.clone()).bold(true).wrap(false),
                status_icon(
                    standing.glyph(),
                    standing.word(),
                    standing.kind(),
                ),
                frame().grow(1.0),
                label(account.clone()).wrap(false).rules(note_rules),
            ))
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
        .frame(|f, t| {
            f.fill(transparent(t.palette.button))
                .radius(t.space.round)
        })
        .hovered(|s| s.frame(|f, t| f.fill(t.palette.button)))
        .pressed(|s| s.frame(|f, t| f.fill(t.pressed())))
        .transition(Motion::Interact)
        .apply(cx);
}

/// The width of the ring of a hamlet's budget, and how thick it is.
const METER: f32 = 16.0;
const RING: f32 = 4.0;

/// The share of its budget from which a hamlet's circle turns yellow.
const NEAR_CAP: f32 = 0.8;
/// The least of its circle that spending shows.
const MIN_SHOWN: f32 = 0.06;

/// What the hamlet spent of its budget: when it has a cap, a ring
/// with nothing inside, drawn clockwise from the top as far as
/// `spent` says, in the accent while the budget is fine, yellow near
/// the cap and red at it, then the words.
pub(super) fn budget(words: String, spent: Option<f32>) -> AnyView {
    with_cx(move |cx| {
        let palette = &cx.theme().palette;
        let mut parts = Vec::new();
        if let Some(spent) = spent {
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
                    AngularColorStop::new(palette.gold_6, turn),
                    AngularColorStop::new(palette.gold_6, TAU),
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
        // Everything else done to it, as a right click on its common
        // in the world offers: renaming, moving, closing it down.
        if *rename || *carry || *close {
            actions.push(more_button(
                "Rename, move or close it down",
                move |world| {
                    world_menu::open_for(
                        world,
                        Thing::Common(hamlet),
                    );
                },
            ));
        }
        // The path names it, renames it at a double click on its
        // name, and says what commands call it when its name says
        // otherwise.
        let path = crumbs(vec![
            crumb("Realm", |world| {
                select(world, None);
                crate::hub::show(world, crate::hub::Showing::Realm);
            })
            .boxed(),
            crumb_slash().boxed(),
            naming(hamlet, facts.id.clone(), facts.title.clone()),
        ]);
        let called = facts.title != facts.id;
        let rename = *rename;
        let path = if !called && !rename {
            path.boxed()
        } else {
            let id = facts.id.clone();
            path.tooltip(move || {
                let mut says = Vec::new();
                if called {
                    says.push(
                        row((
                            note("Commands call it"),
                            reference(id.clone()),
                        ))
                        .gap(4.0)
                        .align(AlignItems::Center)
                        .boxed(),
                    );
                }
                if rename {
                    says.push(
                        note("Double-click its name to rename it.")
                            .boxed(),
                    );
                }
                column(says).gap(4.0)
            })
            .boxed()
        };
        let mut top =
            vec![path, budget(facts.account.clone(), facts.spent)];
        // The forms open only from these buttons, so without them
        // nothing stands under the account.
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
        &PAGES,
        &[words::AGENTS, words::BYLAWS, words::RECORD],
        Spread::Shared,
        |world, page| {
            *world.resource_mut::<Page>() = page;
        },
    );
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
