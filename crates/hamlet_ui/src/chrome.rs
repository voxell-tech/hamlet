//! The window's own frame: it has no title bar from the system, so
//! the top bar moves it, three buttons minimise, maximise and close
//! it, and its edges resize it.

use bevy::app::{App, Startup, Update};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Or, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Local, Query, Res, ResMut, Single};
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::input::ButtonInput;
use bevy::input::mouse::MouseButton;
use bevy::math::{CompassOctant, Vec2};
use bevy::picking::events::PointerPress;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerButton, PointerId};
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
use bevy::text::EditableText;
use bevy::time::Time;
use bevy::ui::{AlignItems, px};
use bevy::ui_widgets::Button as ButtonBehavior;
use bevy::window::{
    PrimaryWindow, SystemCursorIcon, Window, WindowCloseRequested,
};
use bevy_fynix::tokens::Motion;
use bevy_fynix::views::{BehaviorExt, FrameProps, button, icon, row};
use bevy_fynix::{OverrideCursor, ScopedExt, style};
use hamlet_desktop::title_bar::TitleBarDrag;

use crate::theme::{HamletTheme, Palette};
use crate::{Cx, View, built};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<ChromeSettings>()
        .init_resource::<Maximised>()
        .add_observer(grab)
        .add_systems(Startup, draw_marks)
        .add_systems(Update, (resize_by_edge, hold_title_bar));
}

/// A window with no title bar from the system. On macOS the system
/// keeps the frame and hides its bar and buttons. Elsewhere the
/// window has no frame at all.
pub fn bare() -> Window {
    Window {
        decorations: cfg!(target_os = "macos"),
        titlebar_transparent: true,
        titlebar_show_title: false,
        titlebar_show_buttons: false,
        fullsize_content_view: true,
        ..Window::default()
    }
}

/// A part of the window that moves it when dragged, and maximises it
/// when pressed twice.
#[derive(Component)]
pub(crate) struct Grip;

/// The tuning of the window's frame.
#[derive(Resource, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChromeSettings {
    /// The distance from an edge within which the pointer resizes
    /// the window, in pixels.
    pub edge: f32,
    /// Seconds within which two presses are one double press.
    pub double: f32,
    /// The side of a window button, in pixels.
    pub button: f32,
}

/// The marks on the window's buttons.
#[derive(Resource)]
pub(crate) struct Marks {
    minimise: Handle<Image>,
    maximise: Handle<Image>,
    close: Handle<Image>,
}

#[derive(Clone, Copy)]
enum Mark {
    /// A line along the foot.
    Line,
    /// A square outline.
    Square,
    Cross,
}

/// A mark drawn in white, to be tinted.
fn mark(mark: Mark) -> Image {
    let side = 32usize;
    let mut data = vec![255u8; side * side * 4];
    for y in 0..side {
        for x in 0..side {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5)
                - Vec2::splat(side as f32 / 2.0);
            // Distance to the mark's strokes, in pixels.
            let away = match mark {
                Mark::Line if p.x.abs() <= 8.0 => (p.y - 6.0).abs(),
                Mark::Square => {
                    (p.x.abs().max(p.y.abs()) - 7.5).abs()
                }
                Mark::Cross if p.x.abs().max(p.y.abs()) <= 8.0 => {
                    (p.x - p.y).abs().min((p.x + p.y).abs())
                        * std::f32::consts::FRAC_1_SQRT_2
                }
                Mark::Line | Mark::Cross => f32::MAX,
            };
            let alpha = 1.0 - (away - 1.1).clamp(0.0, 1.0);
            data[(y * side + x) * 4 + 3] = (alpha * 255.0) as u8;
        }
    }
    Image::new(
        Extent3d {
            width: side as u32,
            height: side as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Draws the marks of the window's buttons.
pub fn draw_marks(world: &mut World) {
    let mut images = world.resource_mut::<Assets<Image>>();
    let marks = Marks {
        minimise: images.add(mark(Mark::Line)),
        maximise: images.add(mark(Mark::Square)),
        close: images.add(mark(Mark::Cross)),
    };
    world.insert_resource(marks);
}

/// A window button: bare until the pointer is on it, then a plate of
/// `under`.
fn plate(under: fn(&Palette) -> Color) -> impl FnOnce(&mut Cx) {
    move |cx| {
        style::<HamletTheme>()
            .frame(|f, _| f.fill(Color::NONE).radius(0.0))
            .hovered(move |s| s.fill(move |t| under(&t.palette)))
            .pressed(|s| s.fill(|t| t.palette.black))
            .transition(Motion::Interact)
            .apply(cx);
    }
}

fn minimise(world: &mut World) {
    for mut window in world
        .query_filtered::<&mut Window, With<PrimaryWindow>>()
        .iter_mut(world)
    {
        window.set_minimized(true);
    }
}

/// The window was last asked to fill the screen.
#[derive(Resource, Default)]
pub(crate) struct Maximised(bool);

fn maximise(world: &mut World) {
    let now = !world.resource::<Maximised>().0;
    world.resource_mut::<Maximised>().0 = now;
    for mut window in world
        .query_filtered::<&mut Window, With<PrimaryWindow>>()
        .iter_mut(world)
    {
        window.set_maximized(now);
    }
}

/// Asks to close the window, as the system's own button would.
fn close(world: &mut World) {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .iter(world)
        .next();
    if let Some(window) = window {
        world.write_message(WindowCloseRequested { window });
    }
}

/// The three buttons of the window, for the end of the top bar.
pub fn buttons() -> impl View {
    built(|cx| {
        let side = cx.world.resource::<ChromeSettings>().button;
        let marks = cx.world.resource::<Marks>();
        let (small, large, shut) = (
            marks.minimise.clone(),
            marks.maximise.clone(),
            marks.close.clone(),
        );
        let one = move |image: Handle<Image>,
                        under: fn(&Palette) -> Color,
                        act: fn(&mut World)| {
            button(icon(image).size(12.0))
                .width(px(side + 8.0))
                .height(px(side))
                .rules(plate(under))
                .on_activate(move |world| {
                    crate::sounds::window_button(world);
                    act(world);
                })
        };
        cx.build(
            row((
                one(small, |p| p.brass_hover, minimise),
                one(large, |p| p.brass_hover, maximise),
                one(shut, |p| p.red, close),
            ))
            .gap(2.0)
            .align(AlignItems::Center),
        )
    })
}

/// A press on a handle moves the window, and a second soon after
/// maximises it or puts it back.
pub(crate) fn grab(
    mut press: On<PointerPress>,
    handles: Query<(), With<Grip>>,
    parents: Query<&ChildOf>,
    pressable: Query<(), With<ButtonBehavior>>,
    time: Res<Time>,
    set: Res<ChromeSettings>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut maximised: ResMut<Maximised>,
    mut last: Local<Option<f32>>,
) {
    let (grip, pressed) =
        (press.event_target(), press.original_event_target());
    // A press on a button in the handle is the button's.
    let on_button = core::iter::once(pressed)
        .chain(parents.iter_ancestors(pressed))
        .take_while(|node| *node != grip)
        .any(|node| pressable.contains(node));
    if press.button != PointerButton::Primary
        || !handles.contains(grip)
        || on_button
    {
        return;
    }
    // The press has been dealt with by the innermost handle.
    press.propagate(false);
    let now = time.elapsed_secs();
    if last.is_some_and(|at| now - at < set.double) {
        *last = None;
        maximised.0 = !maximised.0;
        window.set_maximized(maximised.0);
        return;
    }
    *last = Some(now);
    window.start_drag_move();
}

/// Leaves a drag that starts on a control in the top bar, a button
/// or a field, to that control rather than to the system, which
/// moves the window by a drag anywhere in its title bar.
fn hold_title_bar(
    hover: Option<Res<HoverMap>>,
    controls: Query<
        (),
        Or<(With<ButtonBehavior>, With<EditableText>)>,
    >,
    handles: Query<(), With<Grip>>,
    parents: Query<&ChildOf>,
    drag: Option<ResMut<TitleBarDrag>>,
) {
    let Some(mut drag) = drag else {
        return;
    };
    let on_control = hover
        .as_deref()
        .and_then(|hover| hover.get(&PointerId::Mouse))
        .is_some_and(|hits| {
            hits.keys().any(|&hit| {
                let mut up = core::iter::once(hit)
                    .chain(parents.iter_ancestors(hit));
                up.any(|node| controls.contains(node))
                    && parents
                        .iter_ancestors(hit)
                        .any(|node| handles.contains(node))
            })
        });
    if drag.0 == on_control {
        drag.0 = !on_control;
    }
}

/// The edge or corner of a window `size` across that the pointer at
/// `at` is within `near` of.
fn edge(at: Vec2, size: Vec2, near: f32) -> Option<CompassOctant> {
    let (west, east) = (at.x < near, at.x > size.x - near);
    let (north, south) = (at.y < near, at.y > size.y - near);
    Some(match (north, south, west, east) {
        (true, _, true, _) => CompassOctant::NorthWest,
        (true, _, _, true) => CompassOctant::NorthEast,
        (_, true, true, _) => CompassOctant::SouthWest,
        (_, true, _, true) => CompassOctant::SouthEast,
        (true, ..) => CompassOctant::North,
        (_, true, ..) => CompassOctant::South,
        (_, _, true, _) => CompassOctant::West,
        (_, _, _, true) => CompassOctant::East,
        _ => return None,
    })
}

fn cursor(edge: CompassOctant) -> SystemCursorIcon {
    match edge {
        CompassOctant::North | CompassOctant::South => {
            SystemCursorIcon::NsResize
        }
        CompassOctant::East | CompassOctant::West => {
            SystemCursorIcon::EwResize
        }
        CompassOctant::NorthWest | CompassOctant::SouthEast => {
            SystemCursorIcon::NwseResize
        }
        CompassOctant::NorthEast | CompassOctant::SouthWest => {
            SystemCursorIcon::NeswResize
        }
    }
}

/// Resizes a window that the system gives no edges to: the pointer
/// shows which way at an edge, and a press there takes hold of it.
pub(crate) fn resize_by_edge(
    set: Res<ChromeSettings>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut shown: ResMut<OverrideCursor>,
    mut ours: Local<bool>,
) {
    if window.decorations {
        return;
    }
    let size = Vec2::new(window.width(), window.height());
    let at = window
        .cursor_position()
        .and_then(|at| edge(at, size, set.edge));
    match at {
        Some(at) => {
            shown.0 = Some(cursor(at));
            *ours = true;
            if buttons.just_pressed(MouseButton::Left) {
                window.start_drag_resize(at);
            }
        }
        // Only a cursor this set is put back.
        None if *ours => {
            shown.0 = None;
            *ours = false;
        }
        None => {}
    }
}
