//! The Hub's rail: the realm as its chain of command. The realm and
//! the Hand first, then what waits for the chief, then each hamlet as
//! the tree of its hires under its mayor, then the city's profiles,
//! the library and those that left. A row keeps its place as its
//! agent works and rests; only its mark changes.

pub(super) mod lines;
mod rows;
mod sections;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::text::EditableText;
use bevy::ui::{
    AlignItems, ComputedNode, Display, UiGlobalTransform, UiRect,
    percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::{MotionTokens, Tone};
use bevy_fynix::views::{
    self, Axis, BehaviorExt, FrameProps, TooltipExt, button, column,
    frame, label, row, text_field,
};
use bevy_fynix::{
    Dragging, OverrideCursor, ScopedExt, ViewExt, derived,
    every_frame, keyed, resource, style,
};
use hamlet_realm::pick::{Selected, Thing, bring_into_view, select};
use hamlet_ui::styles::{ICON, note, scroll_area, text_input};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::hint::search;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::switch::labelled_switch;
use hamlet_ui::widgets::tabs::{self, Spread};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use self::lines::{Group, Line, Section, Target};
use self::rows::{
    chevron, expanded, group_meaning, mark_icon, row_rules, washed,
};
use self::sections::{hamlet_names, sections};
use super::{Showing, show};
use crate::convo::{talk_to, talk_to_hand};
use crate::words;
use crate::world_menu::Asks;

/// The width of the rail at first, the least it is dragged to, so a
/// row keeps its name, and the most.
const WIDE: f32 = 248.0;
const NARROWEST: f32 = 200.0;
const WIDEST: f32 = 520.0;
/// The least the rail leaves the page beside it.
const PAGE: f32 = 280.0;

/// The most that left a rail shows.
const LEFT_SHOWN: usize = 6;

/// The most letters of a line a row holds; the rail's width cuts it
/// shorter.
const LINE_SHOWN: usize = 90;

/// The most letters of a row's second line, which wraps rather than
/// being cut at the rail's edge.
const TITLE_SHOWN: usize = 56;

/// The least height of a row, so that a row of one line is as easy
/// to press as one of two.
const ROW: f32 = 32.0;

/// The height of a worker's row and the side of its icon, smaller
/// than a hire's, so the hires stand out of their workers.
const WORKER_ROW: f32 = 26.0;
const WORKER_ICON: f32 = 12.0;

/// How opaque the icon of one that left is.
const DIMMED: f32 = 0.6;

/// The least height of `line`'s row.
fn row_height(line: &Line) -> f32 {
    if line.worker { WORKER_ROW } else { ROW }
}

/// The column at the rail's edge that holds every chevron.
const CHEVRON: f32 = 14.0;

/// How far each step down a tree is set in: a chevron's room and a
/// little more, so a guide under a chevron leads to the next one.
const STEP_IN: f32 = 18.0;

/// The deepest a row is set in; deeper rows line up with it.
const DEEPEST: u8 = 6;

/// The room between an elbow's end and the row it leads to.
const GAP: f32 = 4.0;

/// The room inside a row's ends, and over and under its words.
const ROW_INSET: f32 = 8.0;
const ROW_DOWN: f32 = 6.0;

/// The room over a section's header inside its block.
const BLOCK_PAD: f32 = 4.0;

/// The space over each part of the rail: what waits, the tabs and
/// each hamlet, so that the parts stand apart and evenly.
const PART_GAP: f32 = 16.0;

/// The space over the first part under the tabs, less than
/// [`PART_GAP`] by what the search row already leaves under it.
const UNDER_SEARCH: f32 = 12.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Rail>()
        .init_resource::<Tab>()
        .add_observer(grab)
        .add_observer(drag)
        .add_observer(drop)
        .add_systems(Update, (read_search, tabs::mark::<Tab>));
}

/// The rail's search, what is folded, and its width.
#[derive(Resource, PartialEq)]
pub struct Rail {
    search: String,
    /// The parts folded to their header. A hamlet with something
    /// waiting opens whatever this says.
    folded: Vec<Group>,
    /// The groups of workers opened; each is folded until it is.
    opened: Vec<Group>,
    /// How wide the chief dragged it.
    width: f32,
    /// The hamlets left out of it; one with something waiting is
    /// shown whatever this says.
    hidden: Vec<Entity>,
    /// Whether the list of hamlets to show is open.
    filtering: bool,
}

/// The part of the realm the rail lists under the realm's own rows
/// and what waits, one at a time.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    /// The hamlets, each the tree under its mayor, and the hires
    /// that left.
    #[default]
    Hamlets,
    /// The profiles, those at the harbor and those that left.
    City,
}

impl Default for Rail {
    fn default() -> Self {
        Self {
            search: String::new(),
            folded: Vec::new(),
            opened: Vec::new(),
            width: WIDE,
            hidden: Vec::new(),
            filtering: false,
        }
    }
}

impl Rail {
    /// Shows the rail's rows at its first width.
    pub fn show_rows(&mut self) {
        self.width = WIDE;
    }

    /// Folds `group` to its header, or opens it.
    fn toggle(&mut self, group: Group) {
        // Workers are kept in what is opened, as they start folded.
        let kept = match group {
            Group::Workers(_) => &mut self.opened,
            _ => &mut self.folded,
        };
        match kept.iter().position(|g| *g == group) {
            Some(at) => {
                kept.remove(at);
            }
            None => kept.push(group),
        }
    }
}

/// The root of the Hub, whose width bounds the rail's.
#[derive(Component)]
pub(super) struct Frame;

/// The line between the rail and the page, dragged to size the rail.
#[derive(Component)]
pub(super) struct Divider;

/// The rail itself, as laid out.
#[derive(Component)]
struct Body;

/// The line between the rail and the page: dragged, it sizes the
/// rail, between [`NARROWEST`] and [`WIDEST`].
pub(super) fn divider() -> impl View {
    views::divider(Axis::Vertical)
        .rules(revealed_line)
        .tagged(Divider)
}

/// The divider is a hairline, in the accent while held or hovered.
fn revealed_line(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| f.fill(t.palette.gold_6))
        .hovered(|s| s.fill(|t: &HamletTheme| t.palette.gold_4))
        .when::<Dragging>(|s| {
            s.fill(|t: &HamletTheme| t.palette.gold_4)
        })
        .transition(HamletTheme::interact)
        .apply(cx);
}

fn grab(
    start: On<PointerDragStart>,
    dividers: Query<(), With<Divider>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if dividers.contains(start.entity) {
        cursor.0 = Some(SystemCursorIcon::EwResize);
    }
}

/// Sizes the rail so that the line is where the pointer is, however
/// fast it moves. Every desk has a Hub of its own, so the rail
/// measured is the one beside the line dragged.
fn drag(
    drag: On<PointerDrag>,
    dividers: Query<&UiGlobalTransform, With<Divider>>,
    frames: Query<(&ComputedNode, &UiGlobalTransform), With<Frame>>,
    bodies: Query<(&ComputedNode, &UiGlobalTransform), With<Body>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    mut rail: ResMut<Rail>,
) {
    let Ok(line) = dividers.get(drag.entity) else {
        return;
    };
    let at = drag.pointer.position;
    let Some(frame) = parents
        .iter_ancestors(drag.entity)
        .find(|above| frames.contains(*above))
    else {
        return;
    };
    // The Hub starts where the rail does, in logical pixels.
    let Ok((node, place)) = frames.get(frame) else {
        return;
    };
    let Some((body, body_at)) = children
        .iter_descendants(frame)
        .find_map(|kid| bodies.get(kid).ok())
    else {
        return;
    };
    let scale = node.inverse_scale_factor();
    let left = (place.translation.x - node.size().x * 0.5) * scale;
    // The line stands the gap past the rail's edge, as last laid out.
    let edge = body_at.translation.x + body.size().x * 0.5;
    let past = (line.translation.x - edge) * scale;
    let most =
        (node.size().x * scale - PAGE).clamp(NARROWEST, WIDEST);
    rail.width = (at.x - left - past).clamp(NARROWEST, most);
}

fn drop(
    end: On<PointerDragEnd>,
    dividers: Query<(), With<Divider>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if dividers.contains(end.entity) {
        cursor.0 = None;
    }
}

/// The root of the search field.
#[derive(Component)]
struct Search;

fn read_search(
    root: Query<&Children, With<Search>>,
    texts: Query<&EditableText>,
    mut rail: ResMut<Rail>,
) {
    let Some(text) = root
        .iter()
        .flat_map(|children| children.iter().copied())
        .find_map(|child| texts.get(child).ok())
    else {
        return;
    };
    if text.value() != rail.search.as_str() {
        rail.search = text.value().to_string();
    }
}

/// The tabs between the hamlets and the city.
const TABS: [(Tab, Glyph, &str); 2] = [
    (Tab::Hamlets, Glyph::Hamlet, "Hamlets"),
    (Tab::City, Glyph::Profile, "City"),
];

/// An icon button of the search row.
fn tool(
    which: Glyph,
    says: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    button(glyph(which, ICON))
        .padding(UiRect::all(px(4.0)))
        .rules(views::tint)
        .tooltip(move || note(says))
        .on_activate(act)
        .boxed()
}

/// The search over the rail's rows, with a word in it while it is
/// empty, and at its end the filter of the hamlets shown and a way to
/// make what the tab lists.
fn search_row() -> impl View {
    let field = search(
        text_field(String::new(), |_, _| {})
            .rules(text_input)
            .tagged(Search),
        "Search",
    );
    // Every tab's tools are built once and only shown on their tab,
    // so that a change of tab does not make them anew.
    let on = |shown: Tab, view: AnyView| {
        column((view,)).display(resource::<Tab, _>(move |tab| {
            if *tab == shown {
                Display::Flex
            } else {
                Display::None
            }
        }))
    };
    let tools = row((
        on(
            Tab::Hamlets,
            tool(
                Glyph::Sliders,
                "Choose which hamlets are shown",
                |w| {
                    let mut rail = w.resource_mut::<Rail>();
                    rail.filtering = !rail.filtering;
                },
            ),
        ),
        on(
            Tab::Hamlets,
            tool(Glyph::Plus, "Create a hamlet", |w| {
                crate::world_menu::ask_found(w);
            }),
        ),
        on(
            Tab::City,
            tool(
                Glyph::Plus,
                "Make a profile",
                crate::world_menu::ask_new_profile,
            ),
        ),
    ))
    .gap(2.0);
    with_cx(move |cx| {
        row((field, tools))
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center)
    })
}

/// A switch for each hamlet that shows or hides it, while the filter
/// is open on the hamlets' tab.
fn filter_list() -> impl View {
    keyed(
        derived(|world: &World| {
            let rail = world.resource::<Rail>();
            let open = rail.filtering
                && world.get_resource::<Tab>().copied()
                    == Some(Tab::Hamlets);
            (open, rail.hidden.clone(), hamlet_names(world))
        })
        .when(every_frame()),
        |(open, hidden, names)| {
            if !*open {
                return frame().boxed();
            }
            column(
                names
                    .iter()
                    .cloned()
                    .map(|(hamlet, name)| {
                        labelled_switch(
                            name,
                            !hidden.contains(&hamlet),
                            move |world, show| {
                                let hidden = &mut world
                                    .resource_mut::<Rail>()
                                    .hidden;
                                hidden.retain(|h| *h != hamlet);
                                if !show {
                                    hidden.push(hamlet);
                                }
                            },
                        )
                        .boxed()
                    })
                    .collect::<Vec<_>>(),
            )
            .gap(6.0)
            .padding(UiRect::axes(px(CHEVRON), px(4.0)))
            .boxed()
        },
    )
    .within(hamlet_ui::styles::bare())
}

/// The tabs between the hamlets and the city, in the style of the
/// panels' tabs, the search under them, and the hamlets to show.
fn tabs_and_search() -> impl View {
    with_cx(|cx| {
        column((
            tabs::tipped_strip(
                &TABS,
                &[words::HAMLET, words::CITY],
                Spread::Shared,
                |world, tab| {
                    *world.resource_mut::<Tab>() = tab;
                },
            ),
            search_row(),
            filter_list(),
        ))
        .gap(cx.theme().space.gap)
    })
}

/// What pressing a row does.
fn go(world: &mut World, target: Target) {
    match target {
        Target::Realm => show(world, Showing::Realm),
        Target::Hand => talk_to_hand(world),
        // Its thread, and its house brought into view.
        Target::Agent(agent) => {
            talk_to(world, agent);
            if world.contains_resource::<Selected>() {
                let home = crate::convo::home_of(world, agent);
                bring_into_view(world, home);
            }
        }
        Target::Hamlet(hamlet) => {
            show(world, Showing::Hamlet(hamlet));
            look_at(world, Thing::Common(hamlet));
        }
        // A profile's house, or its place on the pier, and the
        // library are selected and brought into view in the world
        // too.
        Target::Profile(profile) => {
            show(world, Showing::Profile(profile));
            look_at(world, Thing::Profile(profile));
        }
        Target::Library => {
            show(world, Showing::Library);
            look_at(world, Thing::Library);
        }
        Target::Gone(agent) => show(world, Showing::Gone(agent)),
        Target::Fold(group) => {
            world.resource_mut::<Rail>().toggle(group);
        }
    }
}

/// Chooses `thing` in the world and brings it into view, where there
/// is a world.
fn look_at(world: &mut World, thing: Thing) {
    if world.contains_resource::<Selected>() {
        select(world, Some(thing));
        bring_into_view(world, thing);
    }
}

/// A section's header, set apart from the rows under it in the
/// display face: a chevron that folds it, then its title, its count
/// and its state, pressed whole to open its page where it has one.
fn header(section: &Section, above: f32) -> AnyView {
    let group = section.group;
    let fold = match group {
        Group::Needs => {
            frame().width(px(CHEVRON)).shrink(0.0).boxed()
        }
        _ => chevron(group, section.folded, "Fold or open"),
    };
    // Those that left are headed in grey, not in the gold of what
    // can be pressed.
    let tone = if section.current {
        Tone::Accent
    } else if group == Group::Left {
        Tone::Dim
    } else {
        Tone::Faint
    };
    let mut words = vec![
        crate::overview::part_title(section.title.clone(), tone)
            .boxed(),
        label(section.count.to_string()).tone(Tone::Dim).boxed(),
    ];
    if matches!(group, Group::Hamlet(_)) {
        words.push(frame().grow(1.0).boxed());
        words.push(mark_icon(section.mark));
    }
    let words = row(words).gap(6.0).align(AlignItems::Center);
    let words = match group_meaning(group) {
        Some(meaning) => {
            row((words.tooltip(move || note(meaning)).boxed(),))
        }
        None => row((words.boxed(),)),
    };
    match section.opens {
        // Filled under the pointer as a row is, outside the chevron's
        // press so that folding does not open the page; its mark in
        // the column of the rows' marks.
        Some(target) => row((
            fold,
            washed(
                {
                    let pressed = button(words.grow(1.0))
                        .grow(1.0)
                        .min_width(px(0.0))
                        .rules(row_rules)
                        .tagged(Variant::Default)
                        .on_activate(move |world| go(world, target));
                    match target.thing() {
                        Some(thing) => {
                            pressed.tagged(Asks(thing)).boxed()
                        }
                        None => pressed.boxed(),
                    }
                },
                section.current
                    && matches!(section.group, Group::Hamlet(_)),
            ),
        ))
        .gap(0.0)
        .align(AlignItems::Center)
        .padding(UiRect::top(px((above - ROW_DOWN).max(0.0))))
        .width(percent(100.0))
        .boxed(),
        None => row((fold, words))
            .gap(6.0)
            .align(AlignItems::Center)
            .padding(UiRect::new(
                px(0.0),
                px(ROW_INSET),
                px(above),
                px(ROW_DOWN),
            ))
            .width(percent(100.0))
            .boxed(),
    }
}

/// `sections` one under another, the first header `first_above` under
/// what is over it.
fn sections_view(sections: &[Section], first_above: f32) -> AnyView {
    let mut parts = Vec::new();
    for section in sections {
        let mut block = Vec::new();
        // The room over a header is kept outside its block, so a lit
        // block starts at its header.
        let mut above = 0.0;
        if !section.title.is_empty() {
            above = if parts.is_empty() {
                first_above
            } else {
                PART_GAP
            };
            // The title of the hamlet whose page is shown is washed
            // as a selected row is.
            block.push(header(section, BLOCK_PAD));
            above = (above - BLOCK_PAD).max(0.0);
        }
        let lines = if section.folded {
            &[][..]
        } else {
            &section.lines[..]
        };
        let dimmed = section.group == Group::Left;
        block.extend(lines.iter().map(|line| expanded(line, dimmed)));
        parts.push(
            with_cx(move |cx| {
                column(block)
                    .gap(0.0)
                    .margin(UiRect::top(px(above)))
                    .radius(cx.theme().space.round)
            })
            .boxed(),
        );
    }
    column(parts).gap(0.0).boxed()
}

/// Whether `section` is of the realm's own, over the tabs: the realm,
/// the Hand, the library and what waits.
fn is_over_tabs(section: &Section) -> bool {
    matches!(section.group, Group::Top | Group::Needs)
}

/// The rail down the Hub's left side: the realm's own rows and what
/// waits, then the tabs and the search, then the part of the realm
/// the tab chooses. The rows are built anew as they change; the tabs
/// and the search between them are not, so typing keeps its place.
pub(super) fn view() -> impl View {
    let over = keyed(
        derived(|world: &World| {
            // The realm's page lists what waits in full.
            let on_realm = world.get_resource::<Showing>()
                == Some(&Showing::Realm);
            sections(world)
                .into_iter()
                .filter(is_over_tabs)
                .filter(|section| {
                    !(on_realm && section.group == Group::Needs)
                })
                .collect::<Vec<_>>()
        })
        .when(every_frame()),
        |sections| sections_view(sections, PART_GAP),
    )
    .within(hamlet_ui::styles::bare());
    let under = keyed(
        derived(|world: &World| {
            sections(world)
                .into_iter()
                .filter(|section| !is_over_tabs(section))
                .collect::<Vec<_>>()
        })
        .when(every_frame()),
        |sections| {
            // Those that left, at the foot of the rail however short
            // the rest is.
            let (left, rest): (Vec<_>, Vec<_>) = sections
                .iter()
                .cloned()
                .partition(|section| section.group == Group::Left);
            column((
                sections_view(&rest, UNDER_SEARCH),
                frame().grow(1.0),
                sections_view(&left, PART_GAP),
            ))
            .gap(0.0)
            .grow(1.0)
            .boxed()
        },
    )
    .within(hamlet_ui::styles::bare().grow(1.0));
    let body = column((
        over,
        column((tabs_and_search(),))
            .padding(UiRect::top(px(PART_GAP))),
        under,
    ))
    .gap(0.0)
    .grow(1.0);
    with_cx(move |cx| {
        column((scroll_area((body,)),))
            .gap(cx.theme().space.gap)
            .width(resource::<Rail, _>(|rail| px(rail.width)))
            .min_width(px(NARROWEST))
            // Its width is its own: it neither grows into the page's
            // room nor shrinks, and only stretches down.
            .shrink(0.0)
            .height(percent(100.0))
            .min_height(px(0.0))
            .tagged(Body)
    })
}

#[cfg(test)]
mod tests;
