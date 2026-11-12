//! The Terminal panels: each shows the screen of one terminal, and
//! beside it the terminals by hamlet, each agent's commands first,
//! with the press that opens another. A terminal dragged from a list
//! onto the dock opens in a panel of its own, and a terminal shows in
//! only one panel.

mod carry;
mod input;
pub mod keys;
mod list;
mod mouse;
mod pick;
mod screen;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use bevy::app::{App, Update};
use bevy::color::{Alpha, Color};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::Pickable;
use bevy::ui::widget::Text;
use bevy::ui::{
    AlignItems, Node, Outline, Overflow, PositionType, UiRect,
    percent, px,
};
use bevy_fynix::dock::{DockTab, DockTree, DockWindowKind};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame, row,
};
use bevy_fynix::{
    ScopedExt, Theme, ViewExt, derived, every_frame, keyed,
};
use hamlet_client::outbox::RequestId;
use hamlet_client::terminals::{
    self, TerminalId, TerminalOpened, Watched,
};
use hamlet_ui::styles::{
    Fills, ICON, caption, description, link_row, note,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, View, with_cx};

use self::list::Listed;
pub use self::pick::{open_menu, pin, show_menu};
pub use self::screen::Canvas;
use crate::dock::{
    PanelAppExt, flush_panel, number, on_a_desk, open_another,
    show_tab,
};

/// The size a terminal opens at before its canvas has been measured.
const FIRST_SIZE: (u16, u16) = (80, 24);

/// The id of the first Terminal panel. The others are `terminal-2`,
/// `terminal-3` and so on.
pub const PANEL: &str = "terminal";

pub fn plugin(app: &mut App) {
    app.add_numbered_panel(PANEL, window)
        .init_resource::<Panels>()
        .init_resource::<Handing>()
        .init_resource::<Opening>()
        .init_resource::<Shown>()
        .init_resource::<Restoring>()
        .init_resource::<Folds>()
        .init_resource::<Pins>()
        .add_observer(opened)
        .add_systems(
            Update,
            (
                (
                    forget_closed
                        .run_if(resource_changed::<DockTree>)
                        .after(crate::desk::capture),
                    hand_keys,
                    keep_watching,
                    screen::draw,
                    screen::fit,
                )
                    .chain(),
                light_edge,
                name_tabs,
            ),
        )
        .add_plugins((
            input::plugin,
            list::plugin,
            carry::plugin,
            pick::plugin,
        ));
}

/// The dock window of the Terminal panel `id`. Its tab is named after
/// the terminal it shows by [`name_tabs`].
fn window(id: &str) -> DockWindowKind<HamletTheme> {
    let panel = PanelId(id.to_owned());
    DockWindowKind::new(TITLE, move || flush_panel(contents(&panel)))
        .icon(Glyph::Terminal.image(ICON))
}

/// The name of a Terminal panel, and of its tab while it shows none.
const TITLE: &str = "TERMINAL";

/// The letters a Terminal panel's tab holds: its name is cut to them
/// with an ellipsis, and the tab is as wide as them whatever it
/// shows, so it does not jump as commands of other lengths run.
const TAB_LETTERS: usize = 32;

/// The width of a letter of a tab's name, in ems of the body size:
/// a little over the display face's capitals, on average.
const TAB_LETTER_EM: f32 = 0.58;

/// Whether the dock window `id` is a Terminal panel.
fn is_panel(id: &str) -> bool {
    id == PANEL || number(PANEL, id).is_some()
}

/// The Terminal panels the dock holds, each once, in a lasting order.
fn in_dock(world: &World) -> Vec<PanelId> {
    let Some((tree, root)) = world
        .get_resource::<DockTree>()
        .and_then(|tree| Some((tree, tree.root?)))
    else {
        return Vec::new();
    };
    let mut found = tree
        .leaves_under(root)
        .into_iter()
        .flat_map(|(_, leaf)| {
            leaf.tabs().map(|(id, _)| id.to_owned())
        })
        .filter(|id| is_panel(id))
        .map(PanelId)
        .collect::<Vec<_>>();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found.dedup();
    found
}

/// What each Terminal panel shows, by panel id and terminal id, as
/// the view keeps it.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct Shown(pub BTreeMap<String, String>);

/// What each Terminal panel's list has folded away, by panel id: a
/// hamlet's group, the Hand's, the workers' or the unbounded ones,
/// or an agent with what is under it, as `agent#<id>`.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct Folds(pub BTreeMap<String, BTreeSet<String>>);

/// The agents whose bash logs the lists show, as `agent#<id>`.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct Pins(pub BTreeSet<String>);

/// What Terminal panels showed and are to show again once they are in
/// the dock and their terminals are there: as the view kept it, or as
/// a panel on another desk left it.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct Restoring(pub BTreeMap<String, String>);

/// Names each Terminal panel's tab after the terminal it shows.
fn name_tabs(
    tabs: Query<(Entity, &DockTab)>,
    tree: Option<Res<DockTree>>,
    panels: Res<Panels>,
    listed: Res<Listed>,
    theme: Option<Res<Theme<HamletTheme>>>,
    children: Query<&Children>,
    mut texts: Query<(&mut Text, &mut Node)>,
) {
    let (Some(tree), Some(theme)) = (tree, theme) else {
        return;
    };
    let width =
        px(TAB_LETTERS as f32 * TAB_LETTER_EM * theme.0.text.body);
    for (node, tab) in &tabs {
        let Some(window) = tree.leaf(tab.leaf).and_then(|leaf| {
            leaf.tabs()
                .find(|(_, id)| *id == tab.tab)
                .map(|(window, _)| window)
        }) else {
            continue;
        };
        if !is_panel(window) {
            continue;
        }
        let item = panels
            .active(&PanelId(window.to_owned()))
            .and_then(|terminal| listed.find(terminal));
        let name = tab_name(item);
        let Some(label) = children
            .iter_descendants_depth_first(node)
            .find(|kid| texts.contains(*kid))
        else {
            continue;
        };
        let Ok((mut text, mut node)) = texts.get_mut(label) else {
            continue;
        };
        if text.0 != name {
            text.0 = name;
        }
        if node.width != width {
            node.width = width;
            node.overflow = Overflow::clip();
        }
    }
}

/// The name of the tab of a panel showing `item`, cut to
/// [`TAB_LETTERS`].
fn tab_name(item: Option<&list::Item>) -> String {
    let name = match item {
        None => TITLE.to_owned(),
        Some(item) => match &item.last {
            Some(last) => format!("{TITLE} ({}): {last}", item.name),
            None => format!("{TITLE} ({})", item.name),
        },
    };
    list::elided(&name, TAB_LETTERS)
}

/// Forgets the panels the dock no longer holds. One a desk still
/// holds is to show its terminal again when that desk is shown.
fn forget_closed(world: &mut World) {
    let docked = in_dock(world);
    let gone = world
        .resource::<Panels>()
        .0
        .keys()
        .filter(|panel| !docked.contains(panel))
        .cloned()
        .collect::<Vec<_>>();
    for panel in gone {
        let shown = world
            .resource_mut::<Panels>()
            .0
            .remove(&panel)
            .and_then(|kept| kept.active)
            .and_then(|terminal| world.get::<TerminalId>(terminal))
            .map(|id| id.0.clone());
        if let Some(terminal) = shown
            && on_a_desk(world, &panel.0)
        {
            world
                .resource_mut::<Restoring>()
                .0
                .insert(panel.0, terminal);
        }
    }
    let kept = world
        .resource::<Restoring>()
        .0
        .keys()
        .filter(|panel| {
            docked.contains(&PanelId((*panel).clone()))
                || on_a_desk(world, panel)
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    world
        .resource_mut::<Restoring>()
        .0
        .retain(|panel, _| kept.contains(panel));
    let folded = world
        .resource::<Folds>()
        .0
        .keys()
        .filter(|panel| {
            docked.contains(&PanelId((*panel).clone()))
                || on_a_desk(world, panel)
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    if world.resource::<Folds>().0.len() != folded.len() {
        world
            .resource_mut::<Folds>()
            .0
            .retain(|panel, _| folded.contains(panel));
    }
}

/// A Terminal panel's dock window id.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PanelId(pub String);

impl PanelId {
    /// The first Terminal panel.
    pub fn first() -> Self {
        Self(PANEL.to_owned())
    }
}

/// What each Terminal panel shows and keeps.
#[derive(Resource, Default)]
pub struct Panels(HashMap<PanelId, Panel>);

#[derive(Clone, Default)]
pub(crate) struct Panel {
    /// The terminal it shows.
    pub(crate) active: Option<Entity>,
    /// The columns and rows its screen has room for, for a terminal
    /// still to be opened.
    pub(crate) room: Option<(u16, u16)>,
    /// Its list is out of sight.
    pub(crate) list_hidden: bool,
}

impl Panels {
    /// The terminal `panel` shows.
    pub fn active(&self, panel: &PanelId) -> Option<Entity> {
        self.0.get(panel).and_then(|panel| panel.active)
    }

    pub(crate) fn get(&self, panel: &PanelId) -> Option<&Panel> {
        self.0.get(panel)
    }

    pub(crate) fn edit(&mut self, panel: &PanelId) -> &mut Panel {
        self.0.entry(panel.clone()).or_default()
    }

    /// Hides the list of `panel`.
    pub fn hide_list(&mut self, panel: &PanelId) {
        self.edit(panel).list_hidden = true;
    }
}

/// A faint line just inside the panel's edge, lit while its screen
/// has the keys. It is the panel's last child, so drawn over the
/// rest.
#[derive(Component)]
#[require(Pickable = Pickable::IGNORE)]
struct Edge(PanelId);

/// How strongly the [`Edge`] shows the accent.
const EDGE_ALPHA: f32 = 0.45;

/// Lights a panel's [`Edge`] while its screen has the keys.
fn light_edge(
    focus: Option<Res<InputFocus>>,
    canvases: Query<&Canvas>,
    theme: Option<Res<Theme<HamletTheme>>>,
    edges: Query<(Entity, &Edge, Option<&Outline>)>,
    mut commands: Commands,
) {
    let focused = focus
        .and_then(|focus| focus.get())
        .and_then(|node| canvases.get(node).ok());
    let lit = theme.map_or(Color::NONE, |theme| {
        theme.0.palette.gold_4.with_alpha(EDGE_ALPHA)
    });
    for (edge, Edge(panel), outline) in &edges {
        let color = if focused.is_some_and(|c| c.0 == *panel) {
            lit
        } else {
            Color::NONE
        };
        if outline.is_none_or(|outline| outline.color != color) {
            commands.entity(edge).try_insert(Outline::new(
                px(1.0),
                px(-1.0),
                color,
            ));
        }
    }
}

/// The panel whose screen is to be given the keys once it is there.
#[derive(Resource, Default)]
struct Handing(Option<PanelId>);

/// The open a panel waits on, to show what it opens.
#[derive(Resource, Default)]
struct Opening(Option<(RequestId, PanelId)>);

/// What each of `panels` shows: the terminal it had while that is
/// there and no panel before it has it, else the one it is to show
/// again, else none. A panel never takes a terminal of itself.
fn claims(
    panels: &[PanelId],
    had: impl Fn(&PanelId) -> Option<Entity>,
    there: impl Fn(Entity) -> bool,
    again: impl Fn(&PanelId) -> Option<Entity>,
) -> Vec<(PanelId, Option<Entity>)> {
    let mut taken = Vec::new();
    let mut kept = panels
        .iter()
        .map(|panel| {
            let active = had(panel)
                .filter(|terminal| there(*terminal))
                .filter(|terminal| !taken.contains(terminal));
            taken.extend(active);
            (panel.clone(), active)
        })
        .collect::<Vec<_>>();
    for (panel, active) in &mut kept {
        if active.is_none() {
            *active = again(panel)
                .filter(|terminal| there(*terminal))
                .filter(|terminal| !taken.contains(terminal));
            taken.extend(*active);
        }
    }
    kept
}

/// Keeps what each panel in the dock shows, puts back what a panel is
/// to show again, watches those terminals and no other, and notes
/// what each shows for the view.
fn keep_watching(world: &mut World) {
    let panels = in_dock(world);
    let again = world
        .resource::<Restoring>()
        .0
        .iter()
        .filter_map(|(panel, id)| {
            terminals::find(world, id)
                .map(|terminal| (PanelId(panel.clone()), terminal))
        })
        .collect::<HashMap<_, _>>();
    let shown = {
        let kept = world.resource::<Panels>();
        claims(
            &panels,
            |panel| kept.active(panel),
            |terminal| world.get::<TerminalId>(terminal).is_some(),
            |panel| again.get(panel).copied(),
        )
    };
    for (panel, active) in &shown {
        if world.resource::<Panels>().active(panel) != *active {
            world.resource_mut::<Panels>().edit(panel).active =
                *active;
        }
        if active.is_some()
            && world.resource::<Restoring>().0.contains_key(&panel.0)
        {
            world.resource_mut::<Restoring>().0.remove(&panel.0);
        }
    }
    let keep = shown
        .iter()
        .filter_map(|(_, active)| *active)
        .collect::<Vec<_>>();
    let watched = world
        .try_query_filtered::<Entity, With<Watched>>()
        .map(|mut query| query.iter(world).collect::<Vec<_>>())
        .unwrap_or_default();
    for terminal in watched {
        if !keep.contains(&terminal) {
            terminals::unwatch(world, terminal);
        }
    }
    for terminal in &keep {
        terminals::watch(world, *terminal);
    }
    let mut noted = world.resource::<Restoring>().0.clone();
    for (panel, active) in shown {
        if let Some(id) = active
            .and_then(|terminal| world.get::<TerminalId>(terminal))
        {
            noted.insert(panel.0, id.0.clone());
        }
    }
    if world.resource::<Shown>().0 != noted {
        world.resource_mut::<Shown>().0 = noted;
    }
}

/// Shows a terminal a panel opened, and gives it the keys.
fn opened(opened: On<TerminalOpened>, mut commands: Commands) {
    let (request, terminal) =
        (opened.request.clone(), opened.terminal);
    commands.queue(move |world: &mut World| {
        let panel = match &world.resource::<Opening>().0 {
            Some((waited, panel)) if *waited == request => {
                panel.clone()
            }
            _ => return,
        };
        world.resource_mut::<Opening>().0 = None;
        show_tab(world, &panel.0);
        activate(world, &panel, terminal);
    });
}

/// Gives the screen of the panel handed to the keys once it is
/// there: the first terminal's canvas is built only after the
/// terminal arrives.
fn hand_keys(world: &mut World) {
    let Some(panel) = world.resource::<Handing>().0.clone() else {
        return;
    };
    let canvas = world.try_query::<(Entity, &Canvas)>().and_then(
        |mut query| {
            query
                .iter(world)
                .find(|(_, canvas)| canvas.0 == panel)
                .map(|(entity, _)| entity)
        },
    );
    if let Some(canvas) = canvas {
        world.resource_mut::<Handing>().0 = None;
        if let Some(mut focus) =
            world.get_resource_mut::<InputFocus>()
        {
            focus.set(canvas, FocusCause::Pressed);
        }
    }
}

/// Opens the chief's own shell at the size the screen of `panel` has
/// room for.
fn open(world: &mut World, panel: &PanelId) {
    let (cols, rows) = world
        .resource::<Panels>()
        .get(panel)
        .and_then(|panel| panel.room)
        .unwrap_or(FIRST_SIZE);
    let request = terminals::open(world, cols, rows);
    world.resource_mut::<Opening>().0 =
        Some((request, panel.clone()));
}

/// Shows `terminal` in `panel` and gives it the keys, as a press on
/// its screen would. One another panel shows is brought forward
/// there instead.
fn activate(world: &mut World, panel: &PanelId, terminal: Entity) {
    let elsewhere = in_dock(world).into_iter().find(|other| {
        other != panel
            && world.resource::<Panels>().active(other)
                == Some(terminal)
    });
    let panel = match elsewhere {
        Some(other) => {
            show_tab(world, &other.0);
            other
        }
        None => {
            world.resource_mut::<Panels>().edit(panel).active =
                Some(terminal);
            panel.clone()
        }
    };
    world.resource_mut::<Handing>().0 = Some(panel);
}

/// Shows `terminal` in `panel`, without giving it the keys.
pub fn show(world: &mut World, panel: &PanelId, terminal: Entity) {
    world.resource_mut::<Panels>().edit(panel).active =
        Some(terminal);
}

/// The contents of `panel`.
fn contents(panel: &PanelId) -> impl View + use<> {
    let panel = panel.clone();
    with_cx(move |cx| {
        let look = &cx.theme().terminal;
        row((
            column((heading(&panel), body(&panel)))
                .grow(1.0)
                .min_width(px(0.0))
                .fills_height()
                .gap(0.0)
                .fill(look.background),
            side(&panel),
            frame()
                .position(PositionType::Absolute)
                .inset(UiRect::all(px(0.0)))
                .tagged(Edge(panel.clone())),
        ))
        .fills_width()
        .fills_height()
        .gap(0.0)
    })
}

/// The list beside the screen, behind its line, unless hidden.
fn side(panel: &PanelId) -> impl View + use<> {
    let shown = panel.clone();
    let panel = panel.clone();
    keyed(
        derived(move |world: &World| {
            world
                .resource::<Panels>()
                .get(&shown)
                .is_some_and(|shown| shown.list_hidden)
        })
        .when(every_frame()),
        move |hidden| {
            if *hidden {
                frame().boxed()
            } else {
                let panel = panel.clone();
                with_cx(move |cx| {
                    row((
                        frame()
                            .width(px(1.0))
                            .fill(cx.theme().palette.border),
                        list::side(&panel),
                    ))
                    .gap(0.0)
                    .height(percent(100.0))
                })
                .boxed()
            }
        },
    )
    // Not `fills_height`, whose grow in a row would take width the
    // screen is to have.
    .within(row(()).gap(0.0).shrink(0.0).height(percent(100.0)))
}

/// The shown terminal's name over its screen, whether it takes keys,
/// and the press that hides or shows the list.
fn heading(panel: &PanelId) -> impl View + use<> {
    let shown = panel.clone();
    let panel = panel.clone();
    keyed(
        derived(move |world: &World| {
            let panels = world.resource::<Panels>();
            let hidden = panels
                .get(&shown)
                .is_some_and(|shown| shown.list_hidden);
            let listed = world.resource::<Listed>();
            let item = panels
                .active(&shown)
                .and_then(|active| listed.find(active).cloned());
            (item, hidden)
        })
        .when(every_frame()),
        move |(item, hidden)| {
            let (item, hidden) = (item.clone(), *hidden);
            let panel = panel.clone();
            with_cx(move |cx| {
                let space = &cx.theme().space;
                let mut parts = Vec::<AnyView>::new();
                match &item {
                    Some(item) => {
                        parts.push(
                            glyph(Glyph::Terminal, ICON)
                                .tone(Tone::Dim)
                                .boxed(),
                        );
                        parts.push(
                            row((list::crumbs(item, 0, true),))
                                .min_width(px(0.0))
                                .overflow(Overflow::clip())
                                .boxed(),
                        );
                        parts.push(frame().grow(1.0).boxed());
                        parts.push(
                            row(hints(item))
                                .gap(space.gap)
                                .min_width(px(0.0))
                                .overflow(Overflow::clip())
                                .boxed(),
                        );
                    }
                    None => {
                        parts.push(frame().grow(1.0).boxed());
                    }
                }
                parts.push(
                    row(actions(&panel, hidden))
                        .gap(0.0)
                        .shrink(0.0)
                        .boxed(),
                );
                row(parts)
                    .gap(space.gap)
                    .align(AlignItems::Center)
                    .padding(UiRect::new(
                        px(space.section),
                        px(space.gap),
                        px(4.0),
                        px(4.0),
                    ))
            })
            .boxed()
        },
    )
    .within(column(()).gap(0.0))
}

/// What the heading says of `item`: read-only or ended.
fn hints(item: &list::Item) -> Vec<AnyView> {
    let mut hints = Vec::<AnyView>::new();
    if item.commands {
        hints.push(badge("READ-ONLY", Tone::Accent).boxed());
    }
    if let Some(code) = item.exit {
        hints.push(badge(format!("EXIT {code}"), Tone::Dim).boxed());
    }
    hints
}

/// The heading's presses: open a terminal while the list is hidden,
/// and hide or show the list.
fn actions(panel: &PanelId, hidden: bool) -> Vec<AnyView> {
    let mut presses = Vec::<AnyView>::new();
    if hidden {
        presses.push(press(
            Glyph::Plus,
            "New terminal or pin a bash",
            {
                let panel = panel.clone();
                move |world, node| pick::toggle(world, node, &panel)
            },
        ));
    }
    let words = if hidden {
        "Show the list"
    } else {
        "Hide the list"
    };
    presses.push(press(Glyph::Sidebar, words, {
        let panel = panel.clone();
        move |world, _| {
            let mut panels = world.resource_mut::<Panels>();
            let shown = panels.edit(&panel);
            shown.list_hidden = !shown.list_hidden;
        }
    }));
    presses
}

/// Opens another Terminal panel beside `beside`, its screen to have
/// the keys.
pub(crate) fn new_panel(world: &mut World, beside: &PanelId) {
    if let Some(id) = open_another(world, PANEL, &beside.0) {
        world.resource_mut::<Handing>().0 = Some(PanelId(id));
    }
}

/// A small press of `which`, named by a tooltip.
fn press(
    which: Glyph,
    words: &'static str,
    act: impl Fn(&mut World, Entity) + Send + Sync + 'static,
) -> AnyView {
    button(glyph(which, ICON).tone(Tone::Dim))
        .padding(UiRect::all(px(4.0)))
        .rules(link_row)
        .on_activate_with(act)
        .tooltip(move || note(words))
        .boxed()
}

/// Small capitals in `tone`.
fn badge(text: impl Into<String>, tone: Tone) -> impl View {
    caption(text.into(), tone)
}

/// What a panel's body holds.
#[derive(Clone, Copy, PartialEq)]
enum Body {
    /// There are no terminals.
    Empty,
    /// The panel shows none of them.
    Unshown,
    Screen,
}

/// The screen, or the words of a panel that shows no terminal.
fn body(panel: &PanelId) -> impl View + use<> {
    let shown = panel.clone();
    let panel = panel.clone();
    keyed(
        derived(move |world: &World| {
            if world.resource::<Listed>().0.is_empty() {
                Body::Empty
            } else if world
                .resource::<Panels>()
                .active(&shown)
                .is_none()
            {
                Body::Unshown
            } else {
                Body::Screen
            }
        })
        .when(every_frame()),
        move |body| {
            let words = match body {
                Body::Empty => Some(
                    "No terminals yet. Press + to open a shell in a \
                     hamlet, or an unbounded one. An agent's \
                     commands show here as they run.",
                ),
                Body::Unshown => Some(
                    "Choose a terminal in the list to show it here, \
                     or press + to open one. Drag one from the list \
                     onto the dock to open it in a panel of its own.",
                ),
                Body::Screen => None,
            };
            if let Some(words) = words {
                with_cx(move |cx| {
                    let section = cx.theme().space.section;
                    column((description(words),))
                        .padding(UiRect::all(px(section * 2.0)))
                        .fills_width()
                })
                .boxed()
            } else {
                frame()
                    .grow(1.0)
                    .fills_width()
                    .min_height(px(0.0))
                    .overflow(Overflow::clip())
                    .tagged(Canvas(panel.clone()))
                    .boxed()
            }
        },
    )
    .within(
        column(())
            .gap(0.0)
            .grow(1.0)
            .fills_width()
            .min_height(px(0.0)),
    )
}

#[cfg(test)]
mod tests {
    use hamlet_protocol::client as protocol;
    use serde_json::json;

    use super::*;

    fn view(
        id: &str,
        hamlet: Option<&str>,
        agent: Option<u64>,
        kind: &str,
    ) -> terminals::TerminalView {
        serde_json::from_value(json!({
            "agent": agent,
            "cols": 80, "rows": 24, "cwd": "/", "exit": null,
            "hamlet": hamlet, "id": id, "kind": kind, "name": id,
            "title": ""
        }))
        .unwrap()
    }

    #[test]
    fn a_terminal_shown_from_the_list_or_opened_gets_the_keys_once_its_screen_is_there()
     {
        let mut app = App::new();
        app.add_plugins(bevy::input_focus::InputFocusPlugin)
            .init_resource::<Panels>()
            .init_resource::<Handing>()
            .add_systems(Update, hand_keys);
        let panel = PanelId::first();
        let terminal = app.world_mut().spawn_empty().id();
        activate(app.world_mut(), &panel, terminal);
        app.update();
        assert!(app.world().resource::<Handing>().0.is_some());
        app.world_mut().spawn(Canvas(PanelId("terminal-2".into())));
        let canvas = app.world_mut().spawn(Canvas(panel)).id();
        app.update();
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(canvas)
        );
        assert!(app.world().resource::<Handing>().0.is_none());
    }

    /// A dock of one area holding the Terminal panels `ids`.
    fn dock(app: &mut App, ids: &[&str]) {
        use bevy_fynix::dock::{DockAreaStyle, DockLeaf};

        app.world_mut().resource_mut::<DockTree>().set_root_leaf(
            DockLeaf::new("low", DockAreaStyle::TabBar).with_windows(
                ids.iter().map(|id| (*id).to_owned()).collect(),
            ),
        );
    }

    #[test]
    fn a_panel_shows_what_it_had_or_is_to_show_again_and_never_takes_one_of_itself()
     {
        let mut app = crate::testing::panel_app();
        let terminals = |ids: &[&str]| {
            let mut state = hamlet_testing::realm::state();
            state.revision = 1;
            state.terminals = ids
                .iter()
                .map(|id| view(id, None, None, "shell"))
                .collect();
            protocol::ServerMessage::State(state)
        };
        hamlet_client::receive(
            app.world_mut(),
            terminals(&["t1", "t2", "t3"]),
        );
        app.init_resource::<Panels>()
            .init_resource::<Shown>()
            .init_resource::<Folds>()
            .init_resource::<Pins>()
            .insert_resource(Restoring(BTreeMap::from([
                ("terminal-2".to_owned(), "t2".to_owned()),
                ("terminal-3".to_owned(), "t9".to_owned()),
            ])));
        dock(&mut app, &["terminal", "terminal-2", "terminal-3"]);
        let find = |app: &App, id| terminals::find(app.world(), id);
        let [first, second, third] =
            ["terminal", "terminal-2", "terminal-3"]
                .map(|id| PanelId(id.to_owned()));
        // The first had t1, and the second had t1 as well, which the
        // first keeps.
        for panel in [&first, &second] {
            app.world_mut()
                .resource_mut::<Panels>()
                .edit(panel)
                .active = find(&app, "t1");
        }
        keep_watching(app.world_mut());
        let active = |app: &App, panel| {
            app.world().resource::<Panels>().active(panel)
        };
        assert_eq!(active(&app, &first), find(&app, "t1"));
        assert_eq!(active(&app, &second), find(&app, "t2"));
        // t3 is free, but the third is to show t9, which is not
        // there.
        assert_eq!(active(&app, &third), None);
        let noted =
            |app: &App| app.world().resource::<Shown>().0.clone();
        let pairs = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(panel, id)| {
                    ((*panel).to_owned(), (*id).to_owned())
                })
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(
            noted(&app),
            pairs(&[
                ("terminal", "t1"),
                ("terminal-2", "t2"),
                ("terminal-3", "t9")
            ])
        );
        // A terminal closed leaves its panel showing none.
        hamlet_client::receive(
            app.world_mut(),
            terminals(&["t1", "t3"]),
        );
        keep_watching(app.world_mut());
        assert_eq!(active(&app, &second), None);
        // A panel closed, which no desk holds, is forgotten.
        dock(&mut app, &["terminal", "terminal-2"]);
        forget_closed(app.world_mut());
        keep_watching(app.world_mut());
        assert_eq!(noted(&app), pairs(&[("terminal", "t1")]));
    }

    #[test]
    fn a_tab_is_named_after_its_terminal_and_its_last_command() {
        let item = |last: Option<&str>| list::Item {
            terminal: Entity::PLACEHOLDER,
            name: "zsh 3".into(),
            owner: "chief".into(),
            exit: None,
            commands: false,
            key: None,
            last: last.map(str::to_owned),
            running: false,
            crumbs: Vec::new(),
        };
        assert_eq!(tab_name(None), "TERMINAL");
        assert_eq!(tab_name(Some(&item(None))), "TERMINAL (zsh 3)");
        assert_eq!(
            tab_name(Some(&item(Some(
                "cat notes/today.md | grep -n pears"
            )))),
            "TERMINAL (zsh 3): cat notes/t..."
        );
    }

    #[test]
    fn choosing_a_terminal_another_panel_shows_hands_the_keys_there()
    {
        let mut app = App::new();
        app.init_resource::<Panels>()
            .init_resource::<Handing>()
            .init_resource::<DockTree>();
        dock(&mut app, &["terminal", "terminal-2"]);
        let (first, second) =
            (PanelId::first(), PanelId("terminal-2".into()));
        let terminal = app.world_mut().spawn_empty().id();
        app.world_mut()
            .resource_mut::<Panels>()
            .edit(&second)
            .active = Some(terminal);
        activate(app.world_mut(), &first, terminal);
        let panels = app.world().resource::<Panels>();
        assert_eq!(panels.active(&first), None);
        assert_eq!(panels.active(&second), Some(terminal));
        assert_eq!(app.world().resource::<Handing>().0, Some(second));
    }

    #[test]
    fn a_press_on_the_screen_keeps_the_keys_there_and_lights_the_edge()
     {
        use bevy::ecs::hierarchy::ChildOf;
        use bevy::input_focus::pointer_focus::PointerFocusPlugin;
        use bevy::input_focus::{AcquireFocus, InputFocusPlugin};
        use bevy::window::{PrimaryWindow, Window};

        let mut app = App::new();
        app.add_plugins((
            bevy::input::InputPlugin,
            InputFocusPlugin,
            PointerFocusPlugin,
        ))
        .add_systems(Update, light_edge);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        app.update();
        let panel = app.world_mut().spawn(ChildOf(window)).id();
        let canvas = app
            .world_mut()
            .spawn((Canvas(PanelId::first()), ChildOf(panel)))
            .id();
        let edge = app
            .world_mut()
            .spawn((Edge(PanelId::first()), ChildOf(panel)))
            .id();
        app.world_mut().trigger(AcquireFocus {
            focused_entity: canvas,
            window,
        });
        app.update();
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            Some(canvas)
        );
        app.world_mut()
            .insert_resource(Theme(HamletTheme::designed()));
        app.update();
        let lit = |app: &App| {
            app.world()
                .get::<Outline>(edge)
                .is_some_and(|outline| outline.color != Color::NONE)
        };
        assert!(lit(&app));
        app.world_mut().trigger(AcquireFocus {
            focused_entity: panel,
            window,
        });
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), None);
        assert!(!lit(&app));
    }
}
