//! The desk switcher in the top bar: the desks as segments of one
//! sunken track, the one shown raised and underlined, then "+",
//! which adds one, and a chevron listing them all. A tab is renamed
//! in place on a double-click, closed by its "x" once asked, dragged
//! to another place, and offers the rest on a right-click.

mod motion;

use bevy::app::{App, Update};
use bevy::camera::visibility::Visibility;
use bevy::color::{Alpha, Color};
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::math::{Rect, Rot2};
use bevy::picking::Pickable;
use bevy::picking::events::{PointerClick, PointerPress};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::text::EditableText;
use bevy::ui::{
    AlignItems, BackgroundGradient, BoxShadow, ColorStop, Display,
    FlexDirection, LinearGradient, Overflow, OverflowAxis,
    PositionType, ShadowStyle, UiRect, UiTransform, Val, percent, px,
};
use bevy_fynix::scroll::{
    OwnScrollbar, ScrollbarStyle, ScrollbarVisibility,
};
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, GLOBAL, Invoke, Mods,
    ShortcutAppExt,
};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, TooltipExt, button,
    column, frame, label, menu_item, overlay, popup, row, scroll,
    text_field,
};
use bevy_fynix::{
    ModifierExt, ScopedExt, ViewExt, keyed, resource, style,
};

use self::motion::TabDrag;
use crate::desk::Desks;
use crate::fade::{FadeSurface, ScrollFade};
use crate::feel::FeelSettings;
use crate::icons::{Glyph, glyph};
use crate::looks::{
    ICON, action, bad_action, description, note, section, well,
};
use crate::menu::rect_of;
use crate::platform::DisplayText;
use crate::theme::HamletTheme;
use crate::{AnyView, Cx, View, with_cx};

/// Ctrl, on macOS too, where the primary key is Command.
const CTRL: Mods = if cfg!(target_os = "macos") {
    Mods::CTRL
} else {
    Mods::PRIMARY
};

/// The key that adds a desk.
const NEW: Chord = Chord {
    key: KeyCode::KeyT,
    mods: Mods::PRIMARY,
};

/// The keys that show the first nine desks, with the primary key,
/// and the commands they run.
const SHOWN_BY_DIGIT: [(
    &str,
    &str,
    KeyCode,
    fn(&mut World, Invoke),
); 9] = [
    ("desk.show_1", "Desk 1", KeyCode::Digit1, show_at::<0>),
    ("desk.show_2", "Desk 2", KeyCode::Digit2, show_at::<1>),
    ("desk.show_3", "Desk 3", KeyCode::Digit3, show_at::<2>),
    ("desk.show_4", "Desk 4", KeyCode::Digit4, show_at::<3>),
    ("desk.show_5", "Desk 5", KeyCode::Digit5, show_at::<4>),
    ("desk.show_6", "Desk 6", KeyCode::Digit6, show_at::<5>),
    ("desk.show_7", "Desk 7", KeyCode::Digit7, show_at::<6>),
    ("desk.show_8", "Desk 8", KeyCode::Digit8, show_at::<7>),
    ("desk.show_9", "Desk 9", KeyCode::Digit9, show_at::<8>),
];

/// The strength of the glow about the shown desk's underline, and
/// its blur in logical pixels.
const HALO: f32 = 0.75;
const HALO_BLUR: f32 = 4.8;

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(motion::plugin)
        .init_resource::<Tabs>()
        .init_resource::<Renaming>()
        .init_resource::<Opened>()
        .add_observer(click)
        .add_observer(press_close)
        .add_observer(click_close)
        .add_observer(press_outside)
        .add_systems(
            Update,
            (
                follow,
                mark_shown,
                show_close,
                (focus_field, end_rename).chain(),
            ),
        );
    add_commands(app);
}

/// What the tabs are built from: each desk's name, and the one
/// being renamed.
#[derive(Resource, Clone, PartialEq, Default)]
struct Tabs {
    names: Vec<String>,
    renaming: Option<usize>,
}

/// The desk being renamed.
#[derive(Resource, Clone, Copy, PartialEq, Default)]
struct Renaming(Option<usize>);

/// The popup open from the switcher, and the rect it hangs from.
#[derive(Resource, Clone, Copy, PartialEq, Default)]
enum Opened {
    #[default]
    Nothing,
    /// Every desk.
    All { anchor: Rect },
    /// The question of closing the desk at `at`.
    Close { at: usize, anchor: Rect },
}

/// On a desk's tab: the desk's index.
#[derive(Component, Clone, Copy)]
struct DeskTab(usize);

/// On the tab of the desk shown.
#[derive(Component)]
struct ShownDesk;

/// On the track's row of tabs, which scrolls across.
#[derive(Component, Clone, Copy)]
struct TabStrip;

/// On the row of tabs in the strip.
#[derive(Component, Clone, Copy)]
struct TabRow;

/// On the raised segment under the shown tab.
#[derive(Component, Clone, Copy)]
struct Indicator;

/// On the "x" that closes the desk at its index.
#[derive(Component, Clone, Copy)]
struct CloseMark(usize);

/// On the field a desk is renamed in.
#[derive(Component, Clone, Copy)]
struct RenameField;

/// The switcher, for the top bar: a divider, then the track.
pub(crate) fn switcher() -> impl View {
    with_cx(|cx| {
        let theme = cx.theme();
        let (space, palette) = (&theme.space, &theme.palette);
        let divider = frame()
            .width(px(space.divider))
            .height(px(space.track_divider))
            .shrink(0.0)
            .fill(palette.brass_6);
        let hidden = ScrollbarStyle {
            visibility: ScrollbarVisibility::Hidden,
            floating: true,
            ..ScrollbarStyle::default()
        };
        let tabs =
            keyed(resource::<Tabs, _>(Clone::clone), tabs_view)
                .within(row(()).height(percent(100.0)).shrink(0.0));
        let strip = scroll((tabs,))
            .direction(FlexDirection::Row)
            // Across only, so that the glow under the shown desk is
            // not cut at the track's foot.
            .overflow(Overflow {
                x: OverflowAxis::Scroll,
                y: OverflowAxis::Visible,
            })
            .height(percent(100.0))
            .shrink(1.0)
            .tagged(OwnScrollbar(hidden))
            .tagged(ScrollFade {
                surface: FadeSurface::Well,
            })
            .tagged(TabStrip);
        let new = segment(glyph(Glyph::Plus, ICON), space.gap)
            .on_activate(new_desk)
            .tooltip(|| note(format!("New desk ({NEW})")));
        let all = segment(
            row((glyph(Glyph::Chevron, ICON),)).tagged(UiTransform {
                rotation: Rot2::degrees(180.0),
                ..UiTransform::IDENTITY
            }),
            space.gap / 2.0,
        )
        .on_activate_with(|world, node| {
            let anchor = rect_of(world, node).unwrap_or_default();
            *world.resource_mut::<Opened>() = Opened::All { anchor };
        })
        .tooltip(|| note("All desks"));
        let track = row((strip, new, all))
            .height(px(space.bar - 2.0 * space.track_inset))
            .min_width(px(0.0))
            .shrink(1.0)
            .padding(UiRect::all(px(space.track_padding)))
            .gap(space.track_padding)
            .fill(palette.well)
            .border(space.border)
            .border_color(palette.brass_6)
            .radius(space.track_radius)
            .align(AlignItems::Stretch);
        row((divider, track))
            .gap(space.section)
            .padding(UiRect::left(px(space.section)))
            .min_width(px(0.0))
            .shrink(1.0)
            .height(percent(100.0))
            .align(AlignItems::Center)
    })
}

/// The switcher's popups, over everything, while one is open.
pub(crate) fn popups() -> impl View {
    keyed(resource::<Opened, _>(|opened| *opened), |opened| {
        let (anchor, content) = match *opened {
            Opened::Nothing => return frame().boxed(),
            Opened::All { anchor } => (anchor, all_view()),
            Opened::Close { at, anchor } => (anchor, close_view(at)),
        };
        with_cx(move |cx| {
            popup(anchor, (content,))
                .on_dismiss(|world, _| {
                    *world.resource_mut::<Opened>() = Opened::Nothing;
                })
                .min_width(px(cx.theme().space.menu_width))
        })
        .boxed()
    })
    .within(overlay(()))
}

/// The row of tabs, over the raised segment under the shown one.
fn tabs_view(tabs: &Tabs) -> AnyView {
    let tabs = tabs.clone();
    with_cx(move |cx| {
        let count = tabs.names.len();
        let mut views = vec![indicator().boxed()];
        views.extend(tabs.names.iter().enumerate().map(
            |(at, name)| {
                if tabs.renaming == Some(at) {
                    renaming_tab(at, name).boxed()
                } else {
                    tab(at, name, count)
                }
            },
        ));
        row(views)
            .gap(cx.theme().space.track_padding)
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
    with_cx(|cx| {
        let theme = cx.theme();
        let (space, palette) = (&theme.space, &theme.palette);
        let foot = cx.world.resource::<FeelSettings>().foot;
        let radius =
            (space.track_radius - space.track_padding).max(0.0);
        let light = palette.lamp.with_alpha(foot);
        let inner =
            BackgroundGradient::from(LinearGradient::to_top(vec![
                ColorStop::new(light, percent(0.0)),
                ColorStop::new(light.with_alpha(0.0), percent(100.0)),
            ]));
        let halo = BoxShadow(vec![ShadowStyle {
            color: palette.lamp.with_alpha(HALO),
            x_offset: px(0.0),
            y_offset: px(0.0),
            spread_radius: px(0.0),
            blur_radius: px(HALO_BLUR),
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
            .fill(palette.brass_4)
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
            .fill(palette.grey_4)
            .radius(radius)
            .tagged(inner)
            .tagged(Indicator)
            .tagged(Pickable::IGNORE)
    })
}

/// A segment of the track: no plate of its own, dim until the
/// pointer is on it or it is shown.
fn segment_look(cx: &mut Cx) {
    style::<HamletTheme>()
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
        .tone(Tone::Dim)
        .hovered(|s| s.fill(|_| Color::NONE).tone(Tone::Body))
        .pressed(|s| s.fill(|_| Color::NONE).tone(Tone::Faint))
        .when::<ShownDesk>(|s| s.tone(Tone::Body))
        .transition(Motion::Interact)
        .apply(cx);
}

/// `content` as a segment of the track, `pad` either side.
fn segment(content: impl View, pad: f32) -> impl View {
    button(
        row((content,))
            .height(percent(100.0))
            .padding(UiRect::axes(px(pad), px(0.0)))
            .align(AlignItems::Center),
    )
    .rules(segment_look)
}

/// The tab of the desk at `at`, one of `count`.
fn tab(at: usize, name: &str, count: usize) -> AnyView {
    let words =
        label(name.to_uppercase()).wrap(false).tagged(DisplayText);
    with_cx(move |cx| {
        let space = &cx.theme().space;
        let content = if count > 1 {
            row((words, close_mark(at)))
                .gap(space.gap / 2.0)
                .align(AlignItems::Center)
                .boxed()
        } else {
            words.boxed()
        };
        segment(content, space.tab_padding)
            .tagged(DeskTab(at))
            .on_activate(move |world| {
                // The release that ends a drag is no press.
                if !world.resource::<TabDrag>().has_dropped() {
                    world.resource_mut::<Desks>().show(at);
                }
            })
            .context_menu(move || tab_items(at, count))
    })
    .boxed()
}

/// What a right-click on the tab at `at`, one of `count`, offers.
fn tab_items(at: usize, count: usize) -> Vec<AnyView> {
    let item = |text: &'static str, run: fn(&mut World, usize)| {
        menu_item(label(text))
            .on_activate(move |world| run(world, at))
            .boxed()
    };
    let mut items = vec![
        item("Rename", start_rename),
        item("Duplicate", |world, at| {
            world.resource_mut::<Desks>().duplicate(at);
        }),
    ];
    if at > 0 {
        items.push(item("Move Left", |world, at| {
            world.resource_mut::<Desks>().move_to(at, at - 1);
        }));
    }
    if at + 1 < count {
        items.push(item("Move Right", |world, at| {
            world.resource_mut::<Desks>().move_to(at, at + 1);
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

/// Every desk, the one shown in the accent and each with the key
/// that shows it, then a way to add one.
fn all_view() -> AnyView {
    with_cx(|cx| {
        let desks = cx.world.resource::<Desks>();
        let gap = cx.theme().space.menu_gap;
        let mut rows = desks
            .list()
            .iter()
            .enumerate()
            .map(|(at, desk)| {
                let tone = if at == desks.shown() {
                    Tone::Accent
                } else {
                    Tone::Body
                };
                let keys = SHOWN_BY_DIGIT
                    .get(at)
                    .map(|&(_, _, key, _)| {
                        Chord {
                            key,
                            mods: Mods::PRIMARY,
                        }
                        .to_string()
                    })
                    .unwrap_or_default();
                menu_item(
                    row((
                        label(desk.name.clone())
                            .wrap(false)
                            .tone(tone)
                            .grown(1.0),
                        label(keys).wrap(false).tone(Tone::Dim),
                    ))
                    .gap(gap)
                    .grow(1.0)
                    .align(AlignItems::Center),
                )
                .on_activate(move |world| {
                    *world.resource_mut::<Opened>() = Opened::Nothing;
                    world.resource_mut::<Desks>().show(at);
                })
                .boxed()
            })
            .collect::<Vec<_>>();
        rows.push(section("NEW").boxed());
        rows.push(
            menu_item(label("New Desk"))
                .on_activate(|world| {
                    *world.resource_mut::<Opened>() = Opened::Nothing;
                    new_desk(world);
                })
                .boxed(),
        );
        column(rows).gap(0.0)
    })
    .boxed()
}

/// The "x" that closes a tab's desk, after its name, shown while the
/// pointer is on the tab.
fn close_mark(at: usize) -> impl View {
    row((glyph(Glyph::Cross, ICON),))
        .align(AlignItems::Center)
        .rules(|cx: &mut Cx| {
            style::<HamletTheme>()
                .tone(Tone::Dim)
                .hovered(|s| s.tone(Tone::Accent))
                .transition(Motion::Interact)
                .apply(cx);
        })
        .tagged(CloseMark(at))
        .tagged(Visibility::Hidden)
}

/// Asks whether to close the desk at `at`, under its tab, unless it
/// is the last one.
fn ask_close(world: &mut World, at: usize) {
    if world.resource::<Desks>().list().len() < 2 {
        return;
    }
    let anchor = tab_rect(world, at).unwrap_or_default();
    *world.resource_mut::<Opened>() = Opened::Close { at, anchor };
}

/// The question of closing the desk at `at`.
fn close_view(at: usize) -> AnyView {
    let answered = |world: &mut World| {
        *world.resource_mut::<Opened>() = Opened::Nothing;
    };
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        column((
            description("Close this desk?"),
            row((
                bad_action(Glyph::Cross, "CLOSE", move |world| {
                    answered(world);
                    world.resource_mut::<Desks>().close(at);
                }),
                action(Glyph::Back, "CANCEL", answered),
            ))
            .gap(gap),
        ))
        .gap(gap)
        .padding(UiRect::all(px(gap)))
    })
    .boxed()
}

/// The tab of the desk at `at` while it is renamed: a field over it,
/// the tab as wide as before under it.
fn renaming_tab(at: usize, name: &str) -> impl View {
    let (name, shown) = (name.to_owned(), name.to_uppercase());
    with_cx(move |cx| {
        let theme = cx.theme();
        let (pad, size) =
            (theme.space.tab_padding, theme.text.heading);
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
            world.resource_mut::<Desks>().rename(at, &name);
            world.resource_mut::<Renaming>().0 = None;
        })
        .rules(well)
        .rules(|cx: &mut Cx| {
            // As wide and as tall as the tab it covers.
            style::<HamletTheme>()
                .frame(|f, _| {
                    f.width(percent(100.0))
                        .height(percent(100.0))
                        .min_width(px(0.0))
                        .min_height(px(0.0))
                })
                .apply(cx);
        })
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
        .tagged(DeskTab(at))
    })
}

/// Adds a desk and opens its name to be typed.
fn new_desk(world: &mut World) {
    let at = world.resource_mut::<Desks>().add();
    start_rename(world, at);
}

fn start_rename(world: &mut World, at: usize) {
    if at < world.resource::<Desks>().list().len() {
        world.resource_mut::<Renaming>().0 = Some(at);
    }
}

/// The rect of the tab of the desk at `at`.
fn tab_rect(world: &mut World, at: usize) -> Option<Rect> {
    let node = world
        .try_query::<(Entity, &DeskTab)>()?
        .iter(world)
        .find(|(_, tab)| tab.0 == at)
        .map(|(node, _)| node)?;
    rect_of(world, node)
}

/// Keeps what the tabs are built from up to date with the desks.
fn follow(
    desks: Res<Desks>,
    renaming: Res<Renaming>,
    mut tabs: ResMut<Tabs>,
) {
    let now = Tabs {
        names: desks
            .list()
            .iter()
            .map(|desk| desk.name.clone())
            .collect(),
        renaming: renaming.0,
    };
    if *tabs != now {
        *tabs = now;
    }
}

/// Marks the tab of the desk shown, and only it.
fn mark_shown(
    desks: Res<Desks>,
    tabs: Query<(Entity, &DeskTab, Has<ShownDesk>)>,
    mut commands: Commands,
) {
    for (node, tab, marked) in &tabs {
        let shown = tab.0 == desks.shown();
        if shown && !marked {
            commands.entity(node).try_insert(ShownDesk);
        } else if !shown && marked {
            commands.entity(node).try_remove::<ShownDesk>();
        }
    }
}

/// Shows each tab's "x" while the pointer is on the tab and no tab is
/// dragged.
fn show_close(
    dragged: Res<TabDrag>,
    tabs: Query<(&DeskTab, &Hovered)>,
    mut marks: Query<(&CloseMark, &mut Visibility)>,
) {
    let over = |at: usize| {
        !dragged.is_moving()
            && tabs
                .iter()
                .any(|(tab, hovered)| tab.0 == at && hovered.0)
    };
    for (mark, mut visibility) in &mut marks {
        visibility.set_if_neq(if over(mark.0) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// Renames the desk of a tab clicked twice in quick succession.
fn click(
    click: On<PointerClick>,
    tabs: Query<&DeskTab>,
    mut commands: Commands,
) {
    let Ok(tab) = tabs.get(click.event_target()) else {
        return;
    };
    if click.button == PointerButton::Primary && click.count == 2 {
        let at = tab.0;
        commands
            .queue(move |world: &mut World| start_rename(world, at));
    }
}

/// Keeps a press on a tab's "x" from pressing the tab.
fn press_close(
    mut press: On<PointerPress>,
    marks: Query<(), With<CloseMark>>,
) {
    if marks.contains(press.event_target()) {
        press.propagate(false);
    }
}

/// Asks to close the desk of the "x" clicked.
fn click_close(
    mut click: On<PointerClick>,
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
    press: On<PointerPress>,
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
        if let Some(&text) =
            children.iter().find(|&&kid| texts.contains(kid))
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

fn show_at<const AT: usize>(world: &mut World, _: Invoke) {
    world.resource_mut::<Desks>().show(AT);
}

/// The desks' commands: the primary key with T for a new one,
/// Ctrl+Tab and Ctrl+Shift+Tab to go round, and the primary key with
/// 1 to 9 to show one.
fn add_commands(app: &mut App) {
    let command = |id, label, run| CommandSpec {
        id: CommandId(id),
        label,
        scope: GLOBAL,
        run,
        enabled: |_| true,
        repeat: false,
    };
    let tab = |mods| Chord {
        key: KeyCode::Tab,
        mods,
    };
    app.add_command(
        command("desk.new", "New Desk", |world, _| new_desk(world)),
        &[NEW],
    )
    .add_command(
        command("desk.next", "Next Desk", |world, _| {
            world.resource_mut::<Desks>().cycle(false);
        }),
        &[tab(CTRL)],
    )
    .add_command(
        command("desk.previous", "Previous Desk", |world, _| {
            world.resource_mut::<Desks>().cycle(true);
        }),
        &[tab(CTRL.with(Mods::SHIFT))],
    );
    for (id, label, key, run) in SHOWN_BY_DIGIT {
        app.add_command(
            command(id, label, run),
            &[Chord {
                key,
                mods: Mods::PRIMARY,
            }],
        );
    }
}
