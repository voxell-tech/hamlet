//! The workspace switcher: the workspaces as segments of one track
//! in the bar, the one shown raised and underlined, a "+" that adds
//! one or one from a layout a plugin offers, and a chevron listing
//! them all while they do not fit. A tab is renamed in place on a
//! double-click, closed by its "x", dragged to another place, and
//! offers the rest on a right-click.

mod motion;

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{OverflowAxis, OverflowClipMargin, UiTransform};
use bevy_fynix::scroll::{
    OwnScrollbar, ScrollGoal, ScrollbarStyle, ScrollbarVisibility,
};
use bevy_fynix::shortcut::{Chord, Mods};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, TooltipExt, button,
    column, frame, label, menu_item, overlay, popup, row, submenu,
    text_field,
};
use bevy_fynix::{
    Hovered, ModifierExt, ScopedExt, ViewExt, keyed, resource, style,
};
use hamlet_engine_api::CommandId;

use crate::command::{CTRL, RunCommand, ShellCommandsExt, engine_id};
use crate::fade::{FadeSurface, ScrollFade};
use crate::fonts::DisplayText;
use crate::icons::{Glyph, glyph};
use crate::keymap::Bindings;
use crate::sounds::{Cue, InterfaceSound};
use crate::style::{
    Active, ButtonStyle, Glow, action, description, field_rules,
    note, section,
};
use crate::switcher::motion::TabDrag;
use crate::theme::EngineTheme;
use crate::workspace::{
    SuggestedWorkspace, WorkspaceTuning, Workspaces,
};
use crate::{AnyView, Cx, ShellWorldExt, View, themed};

/// The keys that show the first nine workspaces, with the primary
/// key.
const DIGITS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(motion::plugin)
        .init_resource::<Tabs>()
        .init_resource::<NewMenu>()
        .init_resource::<Renaming>()
        .init_resource::<Opened>()
        .init_resource::<LastClick>()
        .add_observer(click)
        .add_observer(press_close)
        .add_observer(click_close)
        .add_observer(press_outside)
        .add_systems(Startup, add_commands)
        .add_systems(
            Update,
            (
                follow,
                mark_active,
                show_close,
                (focus_field, end_rename).chain(),
                tap_on_switch,
            ),
        );
}

/// What the tabs are built from: each workspace's name and the key
/// that shows it, and the one being renamed.
#[derive(Resource, Clone, PartialEq, Default)]
struct Tabs {
    list: Vec<(String, Option<String>)>,
    renaming: Option<usize>,
}

/// What "+" is built from: the key that adds a workspace, and the
/// layouts plugins offer, each a name and the command that takes it.
#[derive(Resource, Clone, PartialEq, Default)]
struct NewMenu {
    keys: Option<String>,
    layouts: Vec<(String, CommandId)>,
}

/// The workspace being renamed, and its name.
#[derive(Resource, Clone, PartialEq, Default)]
pub(crate) struct Renaming(pub(crate) Option<(usize, String)>);

/// The popup open from the switcher, and the rect it hangs from.
#[derive(Resource, Clone, Copy, PartialEq, Default)]
enum Opened {
    #[default]
    Nothing,
    /// Every workspace.
    All { anchor: Rect },
    /// The ways to add a workspace.
    New { anchor: Rect },
    /// The question of closing the workspace at `at`.
    Close { at: usize, anchor: Rect },
}

/// The tab last clicked and when, in seconds.
#[derive(Resource, Default)]
struct LastClick(Option<(usize, f32)>);

/// On a workspace's tab: the workspace's index.
#[derive(Component, Clone, Copy)]
pub(crate) struct WorkspaceTab(pub(crate) usize);

/// On the track's scroll area.
#[derive(Component, Clone, Copy)]
pub(crate) struct TabStrip;

/// On the row of tabs in the scroll area.
#[derive(Component, Clone, Copy)]
pub(crate) struct TabRow;

/// On the raised segment under the shown tab.
#[derive(Component, Clone, Copy)]
pub(crate) struct Indicator;

/// On the chevron that lists every workspace.
#[derive(Component, Clone, Copy)]
pub(crate) struct MoreButton;

/// On the "x" that closes the workspace at its index.
#[derive(Component, Clone, Copy)]
struct CloseMark(usize);

/// On the field a workspace is renamed in.
#[derive(Component, Clone, Copy)]
struct RenameField;

/// The switcher, for the bar: a divider, then the track.
pub fn view() -> impl View {
    themed(|cx| {
        let theme = cx.theme();
        let (space, colors) = (&theme.space, &theme.colors);
        let divider = frame()
            .width(px(space.divider))
            .height(px(space.track_divider))
            .shrink(0.0)
            .fill(*colors.border);
        let hidden = ScrollbarStyle {
            visibility: ScrollbarVisibility::Hidden,
            floating: true,
            ..ScrollbarStyle::default()
        };
        let strip =
            keyed(resource::<Tabs, _>(Clone::clone), tabs_view)
                .within(
                row(())
                    .height(percent(100.0))
                    .min_width(px(0.0))
                    .shrink(1.0)
                    .overflow(Overflow {
                        x: OverflowAxis::Scroll,
                        y: OverflowAxis::Visible,
                    })
                    .clip_margin(OverflowClipMargin::content_box())
                    .with((
                        ScrollGoal::default(),
                        TabStrip,
                        OwnScrollbar(hidden),
                        ScrollFade {
                            surface: FadeSurface::Sunken,
                        },
                    )),
            );
        let new =
            keyed(resource::<NewMenu, _>(Clone::clone), new_view)
                .within(row(()).height(percent(100.0)).shrink(0.0));
        let track = row((strip, more_view(), new))
            .height(px(space.bar - 2.0 * space.track_inset))
            .min_width(px(0.0))
            .shrink(1.0)
            .padding(UiRect::all(px(space.track_padding)))
            .gap(space.track_padding)
            .fill(*colors.surface_sunken)
            .border(space.border)
            .border_color(*colors.border)
            .radius(space.track_radius)
            .align(AlignItems::Stretch);
        row((divider, track))
            .gap(space.section)
            .grow(1.0)
            .min_width(px(0.0))
            .height(percent(100.0))
            .align(AlignItems::Center)
    })
}

/// The popups of the switcher, over everything, while one is open.
pub fn popup_view() -> AnyView {
    keyed(resource::<Opened, _>(|opened| *opened), |opened| {
        let opened = *opened;
        AnyView::new(move |cx: &mut Cx| {
            let (anchor, content) = match opened {
                Opened::Nothing => return cx.build(frame()),
                Opened::All { anchor } => {
                    let tabs = cx.world.resource::<Tabs>().clone();
                    let shown =
                        cx.world.resource::<Workspaces>().shown();
                    (anchor, all_view(&tabs, shown))
                }
                Opened::New { anchor } => {
                    let menu = cx.world.resource::<NewMenu>().clone();
                    (anchor, column(new_items(&menu, false)).boxed())
                }
                Opened::Close { at, anchor } => {
                    (anchor, close_view(at))
                }
            };
            let width = cx.theme().space.menu_width;
            cx.build(
                popup(anchor, (content,))
                    .on_dismiss(|world, _| {
                        *world.resource_mut::<Opened>() =
                            Opened::Nothing;
                    })
                    .min_width(px(width)),
            )
        })
    })
    .within(overlay(()))
    .boxed()
}

/// The row of tabs, over the raised segment under the shown one.
fn tabs_view(tabs: &Tabs) -> AnyView {
    let tabs = tabs.clone();
    themed(move |cx| {
        let shown = cx.world.resource::<Workspaces>().shown();
        let count = tabs.list.len();
        let mut views = vec![indicator().boxed()];
        for (at, (name, keys)) in tabs.list.iter().enumerate() {
            views.push(if tabs.renaming == Some(at) {
                renaming_tab(at, name).boxed()
            } else {
                tab(at, name, keys.clone(), at == shown, count)
                    .boxed()
            });
        }
        let gap = cx.theme().space.track_padding;
        row(views)
            .gap(gap)
            .height(percent(100.0))
            .shrink(0.0)
            .align(AlignItems::Stretch)
            .tagged(TabRow)
    })
    .boxed()
}

/// The raised segment under the shown tab, lit inside from below
/// and underlined with a glowing line, placed by `motion::glide`.
fn indicator() -> impl View {
    themed(|cx| {
        let theme = cx.theme();
        let (space, colors, glow) =
            (&theme.space, &theme.colors, &theme.glow);
        let radius =
            (space.track_radius - space.track_padding).max(0.0);
        let light = colors.active.with_alpha(glow.bottom);
        let inner =
            BackgroundGradient::from(LinearGradient::to_top(vec![
                ColorStop::new(light, percent(0.0)),
                ColorStop::new(light.with_alpha(0.0), percent(100.0)),
            ]));
        let halo = BoxShadow(vec![ShadowStyle {
            color: colors.active.with_alpha(glow.primary),
            x_offset: px(0.0),
            y_offset: px(0.0),
            spread_radius: px(0.0),
            blur_radius: px(glow.blur * glow.core),
        }]);
        let line = frame()
            .position(PositionType::Absolute)
            .inset(UiRect {
                left: px(radius),
                right: px(radius),
                top: Val::Auto,
                bottom: px(0.0),
            })
            .height(px(space.underline))
            .fill(*colors.accent)
            .tagged(halo);
        column((line,))
            .position(PositionType::Absolute)
            .inset(UiRect {
                left: px(0.0),
                right: Val::Auto,
                top: px(0.0),
                bottom: px(0.0),
            })
            .display(Display::None)
            .fill(*colors.fill)
            .radius(radius)
            .tagged(inner)
            .tagged(Indicator)
            .tagged(Pickable::IGNORE)
    })
}

/// The rules of a segment of the track: no background of its own,
/// dim until the pointer is on it or it is shown.
fn segment_rules() -> impl FnOnce(&mut Cx) + Send + Sync + 'static {
    style::<EngineTheme>()
        .frame(|f, t| {
            f.fill(Color::NONE)
                .radius(
                    (t.space.track_radius - t.space.track_padding)
                        .max(0.0),
                )
                .padding(UiRect::all(px(0.0)))
                .min_height(px(0.0))
                .height(percent(100.0))
                .shrink(0.0)
        })
        .label(|l, t| l.size(t.text.heading).bold(true))
        .icon(|i, t| i.size(t.space.icon))
        .tone(Tone::Dim)
        .hovered(|s| s.fill(|_| Color::NONE).tone(Tone::Body))
        .pressed(|s| s.fill(|_| Color::NONE).tone(Tone::Faint))
        .when::<Active>(|s| s.tone(Tone::Body))
        .transition(Motion::Interact)
        .bundle()
}

/// `content` as a segment of the track, `pad` either side.
fn segment(content: impl crate::View, pad: f32) -> impl View {
    button(
        row((content,))
            .height(percent(100.0))
            .padding(UiRect::axes(px(pad), px(0.0)))
            .align(AlignItems::Center),
    )
    .rules(segment_rules())
    .tagged(Glow::Inner)
}

/// The tab of the workspace at `at`, one of `count`.
fn tab(
    at: usize,
    name: &str,
    keys: Option<String>,
    shown: bool,
    count: usize,
) -> impl View {
    let name = name.to_uppercase();
    themed(move |cx| {
        let (pad, gap) =
            (cx.theme().space.tab_padding, cx.theme().space.icon_gap);
        let words = label(name).wrap(false).tagged(DisplayText);
        let content = if count > 1 {
            row((words, close_mark(at)))
                .gap(gap)
                .align(AlignItems::Center)
                .boxed()
        } else {
            words.boxed()
        };
        let tab = segment(content, pad)
            .tagged(WorkspaceTab(at))
            .on_activate(move |world| {
                world.resource_mut::<Workspaces>().show(at);
            });
        let tab = if shown {
            tab.seeded(Active).boxed()
        } else {
            tab.boxed()
        };
        let tab = match keys {
            Some(keys) => {
                tab.tooltip(move || note(keys.clone())).boxed()
            }
            None => tab,
        };
        tab.context_menu(move || tab_items(at, count))
    })
}

/// What a right-click on the tab at `at`, one of `count`, offers.
fn tab_items(at: usize, count: usize) -> Vec<AnyView> {
    let item = |text: &str, run: fn(&mut World, usize)| {
        menu_item(label(text.to_owned()))
            .on_activate(move |world| run(world, at))
            .boxed()
    };
    let mut items = vec![
        item("Rename", |world, at| world.start_rename(at)),
        item("Duplicate", |world, at| {
            world.resource_mut::<Workspaces>().duplicate(at);
        }),
    ];
    if at > 0 {
        items.push(item("Move Left", |world, at| {
            world.resource_mut::<Workspaces>().move_to(at, at - 1);
        }));
    }
    if at + 1 < count {
        items.push(item("Move Right", |world, at| {
            world.resource_mut::<Workspaces>().move_to(at, at + 1);
        }));
    }
    if count > 1 {
        items.push(
            menu_item(label("Close").tone(Tone::Critical))
                .on_activate(move |world| ask_close(world, at))
                .boxed(),
        );
    }
    items
}

/// Asks whether to close the workspace at `at`, under its tab,
/// unless it is the last one.
fn ask_close(world: &mut World, at: usize) {
    if world.resource::<Workspaces>().list().len() < 2 {
        return;
    }
    let anchor = tab_rect(world, at).unwrap_or_default();
    *world.resource_mut::<Opened>() = Opened::Close { at, anchor };
}

/// The question of closing the workspace at `at`, called `name`.
fn close_view(at: usize) -> AnyView {
    themed(move |cx| {
        let gap = cx.theme().space.gap;
        column((
            description("Close this workspace?"),
            row((
                action(ButtonStyle::Critical, "CLOSE").on_activate(
                    move |world| {
                        *world.resource_mut::<Opened>() =
                            Opened::Nothing;
                        world.resource_mut::<Workspaces>().delete(at);
                    },
                ),
                action(ButtonStyle::Quiet, "CANCEL").on_activate(
                    |world| {
                        *world.resource_mut::<Opened>() =
                            Opened::Nothing;
                    },
                ),
            )),
        ))
        .gap(gap)
        .padding(UiRect::all(px(gap)))
    })
    .boxed()
}

/// The "x" that closes a tab's workspace, after its name, shown while
/// the pointer is on the tab.
fn close_mark(at: usize) -> impl View {
    themed(move |cx| {
        let size = cx.theme().space.icon;
        row((glyph(Glyph::Cross, size),))
            .align(AlignItems::Center)
            .rules(
                style::<EngineTheme>()
                    .tone(Tone::Dim)
                    .hovered(|s| s.tone(Tone::Accent))
                    .transition(Motion::Interact)
                    .bundle(),
            )
            .tagged(CloseMark(at))
            .tagged(Visibility::Hidden)
    })
}

/// The tab of the workspace at `at` while it is renamed: a field
/// over it, the tab as wide as before under it.
fn renaming_tab(at: usize, name: &str) -> impl View {
    let (name, shown) = (name.to_owned(), name.to_uppercase());
    themed(move |cx| {
        let theme = cx.theme();
        let (pad, size, inset) = (
            theme.space.tab_padding,
            theme.text.heading,
            theme.space.gap / 2.0,
        );
        let field = text_field(name.clone(), move |world, name| {
            // Enter keeps the field focused while it commits. Focus
            // leaving first, as on a press outside, drops the name.
            let entered = world
                .resource::<InputFocus>()
                .get()
                .and_then(|input| world.get::<ChildOf>(input))
                .is_some_and(|parent| {
                    world
                        .get::<RenameField>(parent.parent())
                        .is_some()
                });
            if !entered {
                return;
            }
            world.resource_mut::<Workspaces>().rename(at, &name);
            world.resource_mut::<Renaming>().0 = None;
        })
        .rules(field_rules())
        .rules(
            style::<EngineTheme>()
                .frame(move |f, _| {
                    f.width(percent(100.0))
                        .height(percent(100.0))
                        .min_height(px(0.0))
                        .padding(UiRect::axes(px(inset), px(0.0)))
                })
                .bundle(),
        )
        .tagged(RenameField);
        let room = label(shown.clone())
            .size(size)
            .bold(true)
            .wrap(false)
            .tagged(DisplayText);
        row((
            row((room,)).tagged(Visibility::Hidden),
            row((field,))
                .position(PositionType::Absolute)
                .inset(UiRect::all(px(0.0))),
        ))
        .height(percent(100.0))
        .shrink(0.0)
        .padding(UiRect::axes(px(pad), px(0.0)))
        .align(AlignItems::Center)
        .tagged(WorkspaceTab(at))
    })
}

/// "+", which adds a blank workspace, and the chevron beside it,
/// which offers a layout a plugin suggests too, as a right-click on
/// "+" does.
fn new_view(menu: &NewMenu) -> AnyView {
    let menu = menu.clone();
    themed(move |cx| {
        let theme = cx.theme();
        let (icon, small, pad) =
            (theme.space.icon, theme.text.small, theme.space.gap);
        let tip = match &menu.keys {
            Some(keys) => format!("New workspace ({keys})"),
            None => "New workspace".to_owned(),
        };
        let items = menu.clone();
        let plus = segment(glyph(Glyph::Plus, icon), pad)
            .on_activate(new_workspace)
            .tooltip(move || note(tip.clone()))
            .context_menu(move || new_items(&items, true));
        let more = segment(
            row((glyph(Glyph::ChevronUp, small),)).tagged(
                UiTransform {
                    rotation: Rot2::degrees(180.0),
                    ..UiTransform::IDENTITY
                },
            ),
            pad / 4.0,
        )
        .on_activate_with(|world, node| {
            let anchor = world.node_rect(node).unwrap_or_default();
            *world.resource_mut::<Opened>() = Opened::New { anchor };
        })
        .tooltip(|| note("New from layout"));
        row((plus, more)).gap(0.0).height(percent(100.0))
    })
    .boxed()
}

/// The ways to add a workspace: blank, or from each layout in
/// `menu`, under a submenu when `nested`.
fn new_items(menu: &NewMenu, nested: bool) -> Vec<AnyView> {
    let blank = menu_item(label("New workspace"))
        .on_activate(|world| {
            *world.resource_mut::<Opened>() = Opened::Nothing;
            new_workspace(world);
        })
        .boxed();
    if nested {
        let menu = menu.clone();
        return vec![
            blank,
            submenu(label("New from layout..."), move || {
                layout_items(&menu)
            })
            .boxed(),
        ];
    }
    let mut items = vec![blank, section("NEW FROM LAYOUT").boxed()];
    items.extend(layout_items(menu));
    items
}

/// A row for each layout in `menu`, which adds a workspace from it.
fn layout_items(menu: &NewMenu) -> Vec<AnyView> {
    if menu.layouts.is_empty() {
        return vec![note("No plugin offers a layout").boxed()];
    }
    menu.layouts
        .iter()
        .map(|(name, take)| {
            let take = take.clone();
            menu_item(label(name.clone()))
                .on_activate(move |world| {
                    *world.resource_mut::<Opened>() = Opened::Nothing;
                    world.run_command(&take);
                })
                .boxed()
        })
        .collect()
}

/// The chevron at the end of the track that lists every workspace,
/// shown while they do not fit.
fn more_view() -> impl View {
    themed(|cx| {
        let (small, pad) =
            (cx.theme().text.small, cx.theme().space.gap / 2.0);
        segment(glyph(Glyph::ChevronRight, small), pad)
            .tagged(MoreButton)
            .on_activate_with(|world, node| {
                let anchor =
                    world.node_rect(node).unwrap_or_default();
                *world.resource_mut::<Opened>() =
                    Opened::All { anchor };
            })
            .tooltip(|| note("All workspaces"))
    })
}

/// Every workspace in `tabs`, as a menu, the one `shown` in the
/// accent.
fn all_view(tabs: &Tabs, shown: usize) -> AnyView {
    let rows = tabs
        .list
        .iter()
        .enumerate()
        .map(|(at, (name, keys))| {
            let tone = if at == shown {
                Tone::Accent
            } else {
                Tone::Body
            };
            let (name, keys) =
                (name.clone(), keys.clone().unwrap_or_default());
            themed(move |cx| {
                let gap = cx.theme().space.menu_gap;
                menu_item(
                    row((
                        label(name).wrap(false).tone(tone).grown(1.0),
                        label(keys).wrap(false).tone(Tone::Dim),
                    ))
                    .gap(gap)
                    .grow(1.0)
                    .align(AlignItems::Center),
                )
                .on_activate(move |world| {
                    *world.resource_mut::<Opened>() = Opened::Nothing;
                    world.resource_mut::<Workspaces>().show(at);
                })
            })
        })
        .collect::<Vec<_>>();
    column(rows).gap(0.0).boxed()
}

/// Adds a blank workspace and opens its name to be typed.
fn new_workspace(world: &mut World) {
    let at = world.resource_mut::<Workspaces>().add();
    world.start_rename(at);
}

/// The rect of the tab of the workspace at `at`.
fn tab_rect(world: &mut World, at: usize) -> Option<Rect> {
    let node = world
        .try_query::<(Entity, &WorkspaceTab)>()?
        .iter(world)
        .find(|(_, tab)| tab.0 == at)
        .map(|(node, _)| node)?;
    world.node_rect(node)
}

/// Keeps what the tabs and "+" are built from up to date with the
/// workspaces, the keys bound to them and the layouts offered.
fn follow(
    workspaces: Res<Workspaces>,
    renaming: Res<Renaming>,
    bindings: Res<Bindings>,
    suggested: Query<&SuggestedWorkspace>,
    mut tabs: ResMut<Tabs>,
    mut menu: ResMut<NewMenu>,
) {
    let keys = |id: &str| bindings.label(&engine_id(id));
    let now = Tabs {
        list: workspaces
            .list()
            .iter()
            .enumerate()
            .map(|(at, workspace)| {
                (
                    workspace.name.clone(),
                    keys(&format!("workspace.show_{}", at + 1)),
                )
            })
            .collect(),
        renaming: renaming.0.as_ref().map(|(at, _)| *at),
    };
    if *tabs != now {
        *tabs = now;
    }
    let mut layouts = suggested
        .iter()
        .map(|offer| (offer.name.clone(), offer.take.clone()))
        .collect::<Vec<_>>();
    layouts.sort();
    let now = NewMenu {
        keys: keys("workspace.new"),
        layouts,
    };
    if *menu != now {
        *menu = now;
    }
}

/// Marks the tab of the workspace shown, and only it.
fn mark_active(
    workspaces: Res<Workspaces>,
    tabs: Query<(Entity, &WorkspaceTab, Has<Active>)>,
    mut commands: Commands,
) {
    for (node, tab, active) in &tabs {
        let want = tab.0 == workspaces.shown();
        if want && !active {
            commands.entity(node).try_insert(Active);
        } else if !want && active {
            commands.entity(node).try_remove::<Active>();
        }
    }
}

/// Shows each tab's "x" while the pointer is on the tab and no tab is
/// dragged.
fn show_close(
    dragged: Res<TabDrag>,
    tabs: Query<(&WorkspaceTab, Has<Hovered>)>,
    mut marks: Query<(&CloseMark, &mut Visibility)>,
) {
    let over = |at: usize| {
        !dragged.moving()
            && tabs
                .iter()
                .any(|(tab, hovered)| tab.0 == at && hovered)
    };
    for (mark, mut visibility) in &mut marks {
        visibility.set_if_neq(if over(mark.0) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// Renames the workspace of a tab clicked twice in quick succession.
fn click(
    click: On<Pointer<Click>>,
    tabs: Query<&WorkspaceTab>,
    time: Res<Time>,
    tuning: Res<WorkspaceTuning>,
    dragged: Res<TabDrag>,
    mut last: ResMut<LastClick>,
    mut commands: Commands,
) {
    let Ok(tab) = tabs.get(click.event_target()) else {
        return;
    };
    if click.button != PointerButton::Primary || dragged.moving() {
        return;
    }
    let (at, now) = (tab.0, time.elapsed_secs());
    let twice = last.0.is_some_and(|(was, when)| {
        was == at && now - when <= tuning.double_click
    });
    if twice {
        last.0 = None;
        commands
            .queue(move |world: &mut World| world.start_rename(at));
    } else {
        last.0 = Some((at, now));
    }
}

/// Keeps a press on a tab's "x" from pressing the tab.
fn press_close(
    mut press: On<Pointer<Press>>,
    marks: Query<(), With<CloseMark>>,
) {
    if marks.contains(press.event_target()) {
        press.propagate(false);
    }
}

/// Closes the workspace of the "x" clicked.
fn click_close(
    mut click: On<Pointer<Click>>,
    marks: Query<&CloseMark>,
    mut commands: Commands,
) {
    if let Ok(mark) = marks.get(click.event_target()) {
        click.propagate(false);
        if click.button == PointerButton::Primary {
            let at = mark.0;
            commands
                .queue(move |world: &mut World| ask_close(world, at));
        }
    }
}

/// Leaves the name being typed on a press outside its field.
fn press_outside(
    press: On<Pointer<Press>>,
    fields: Query<(), With<RenameField>>,
    parents: Query<&ChildOf>,
    mut renaming: ResMut<Renaming>,
) {
    let hit = press.original_event_target();
    if renaming.0.is_none() || press.event_target() != hit {
        return;
    }
    let inside = fields.contains(hit)
        || parents
            .iter_ancestors(hit)
            .any(|above| fields.contains(above));
    if !inside {
        renaming.0 = None;
    }
}

/// Puts the keyboard in a new rename field.
fn focus_field(
    fields: Query<&Children, Added<RenameField>>,
    texts: Query<(), With<EditableText>>,
    mut focus: ResMut<InputFocus>,
) {
    for children in &fields {
        if let Some(text) =
            children.iter().find(|&kid| texts.contains(kid))
        {
            focus.set(text, FocusCause::Navigated);
        }
    }
}

/// Leaves the name being typed on Escape, or once the keyboard is
/// elsewhere.
fn end_rename(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    fields: Query<Entity, With<RenameField>>,
    parents: Query<&ChildOf>,
    mut renaming: ResMut<Renaming>,
) {
    if renaming.0.is_none() {
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
        renaming.0 = None;
    }
}

/// Taps as another workspace is shown by a key; a press on a tab has
/// tapped already.
fn tap_on_switch(
    workspaces: Res<Workspaces>,
    tabs: Query<(), (With<WorkspaceTab>, With<Hovered>)>,
    mut last: Local<Option<usize>>,
    mut cues: MessageWriter<Cue>,
) {
    let shown = workspaces.shown();
    if last.replace(shown).is_some_and(|was| was != shown)
        && tabs.is_empty()
    {
        cues.write(Cue(InterfaceSound::Tap));
    }
}

/// The workspace commands, in no menu of the bar: new on the primary
/// key with T, rename, duplicate and close the shown workspace,
/// Ctrl+Tab and Ctrl+Shift+Tab to go round, and the primary key with
/// 1 to 9 to show one.
fn add_commands(mut commands: Commands) {
    let key = |key, mods| Chord { key, mods };
    commands
        .spawn_command(
            engine_id("workspace.new"),
            "New workspace",
            None,
            &[key(KeyCode::KeyT, Mods::PRIMARY)],
        )
        .observe(|_: On<RunCommand>, mut commands: Commands| {
            commands.queue(new_workspace);
        });
    let on_shown = |run: fn(&mut World, usize)| {
        move |_: On<RunCommand>, mut commands: Commands| {
            commands.queue(move |world: &mut World| {
                let shown = world.resource::<Workspaces>().shown();
                run(world, shown);
            });
        }
    };
    commands
        .spawn_command(
            engine_id("workspace.rename"),
            "Rename workspace",
            None,
            &[],
        )
        .observe(on_shown(|world, at| world.start_rename(at)));
    commands
        .spawn_command(
            engine_id("workspace.duplicate"),
            "Duplicate workspace",
            None,
            &[],
        )
        .observe(on_shown(|world, at| {
            world.resource_mut::<Workspaces>().duplicate(at);
        }));
    commands
        .spawn_command(
            engine_id("workspace.delete"),
            "Close workspace",
            None,
            &[],
        )
        .observe(on_shown(ask_close));
    commands
        .spawn_command(
            engine_id("workspace.next"),
            "Next workspace",
            None,
            &[key(KeyCode::Tab, CTRL)],
        )
        .observe(
            |_: On<RunCommand>,
             mut workspaces: ResMut<Workspaces>| {
                workspaces.cycle(false);
            },
        );
    commands
        .spawn_command(
            engine_id("workspace.previous"),
            "Previous workspace",
            None,
            &[key(KeyCode::Tab, CTRL.with(Mods::SHIFT))],
        )
        .observe(
            |_: On<RunCommand>,
             mut workspaces: ResMut<Workspaces>| {
                workspaces.cycle(true);
            },
        );
    for (at, digit) in DIGITS.into_iter().enumerate() {
        commands
            .spawn_command(
                engine_id(&format!("workspace.show_{}", at + 1)),
                &format!("Workspace {}", at + 1),
                None,
                &[key(digit, Mods::PRIMARY)],
            )
            .observe(
                move |_: On<RunCommand>,
                      mut workspaces: ResMut<Workspaces>| {
                    workspaces.show(at);
                },
            );
    }
}
