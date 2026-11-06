//! The preview: what the graph puts down, as boxes in the world's
//! colours, drawn by a camera of its own into the panel. The camera,
//! its post processing, the hour's lights and the orbit by hand are
//! the realm's, on a render layer of their own. F, Shift+R, B and L
//! turn it to look at the front, the right, the back or the left, and
//! T and Shift+T down from above or up from below.

use core::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::app::{App, Startup, Update};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::camera::visibility::{RenderLayers, Visibility};
use bevy::camera::{Camera, ImageRenderTarget, RenderTarget};
use bevy::color::Alpha;
use bevy::dev_tools::infinite_grid::{
    InfiniteGrid, InfiniteGridPlugin, InfiniteGridSettings,
};
use bevy::ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, If, Local, Query, Res, ResMut};
use bevy::ecs::world::{FromWorld, World};
use bevy::image::Image;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::light::DirectionalLight;
use bevy::math::{Quat, Rect, Vec3};
use bevy::mesh::{Mesh, Mesh3d};
use bevy::pbr::{DistanceFog, MeshMaterial3d, StandardMaterial};
use bevy::picking::events::{PointerDrag, PointerScroll};
use bevy::picking::pointer::PointerButton;
use bevy::render::render_resource::{
    TextureDimension, TextureFormat, TextureUsages,
};
use bevy::time::Time;
use bevy::transform::components::Transform;
use bevy::ui::widget::ViewportNode;
use bevy::ui::{
    AlignItems, ComputedNode, Display, Overflow, PositionType,
    UiRect, Val, percent, px,
};
use bevy::ui_widgets::popover::{
    PopoverAlign, PopoverPlacement, PopoverSide,
};
use bevy::window::{PrimaryWindow, RequestRedraw, Window};
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, Invoke, Layer, Mods, Scope,
    ScopeId, ScopeSpec, ShortcutAppExt,
};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    label, overlay, popup, row,
};
use bevy_fynix::{
    ScopedExt, Theme, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_blueprint::{Frame, Object};
use hamlet_graph::{Asset, BODY, Catalogue, Paint, Shape};
use hamlet_graph_editor::Editor;
use hamlet_graph_editor::library::Library;
use hamlet_realm::generator::house::slope;
use hamlet_realm::meshes::{Meshing, Rng};
use hamlet_realm::palette::{Palette as Colours, TownStyle};
use hamlet_realm::pick::{
    Orbit, Pace, Steer, backward, scrolled_lines,
};
use hamlet_realm::sky::{
    self, CameraSettings, Hourly, Looks, North, OwnSky, Sky,
    SkySettings,
};
use hamlet_ui::styles::{
    self, Fills, ICON, checked_when, description, note, toggle_button,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::dial::{Face, dial};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View};

use super::Blueprint;
use super::assets::{Meshes, Model, palette_colour};
use super::blueprint::{Got, roll_hamlet, roll_item};
use super::gizmo::{self, Dragging, GizmoMode};
use crate::roll::moved;

/// The asset name a transform is shown as when nothing is put there.
pub(super) const MARKER: &str = "marker";

/// The render layer only the preview's camera, lights and pieces are
/// on. Not 7, which the realm's debug overlay puts its camera on.
pub(super) const LAYER: usize = 8;

/// The camera's yaw before a hand has turned it, in radians.
const YAW: f32 = 0.8;

/// The camera's pitch before a hand has turned it, in radians.
const PITCH: f32 = 0.55;

/// The lowest a hand turns the camera's pitch, in radians, as near
/// level as it looks.
const LOWEST: f32 = 0.1;

/// The highest a hand turns the camera's pitch, in radians. Only the
/// views from above and below go past it, to straight down or up.
const HIGHEST: f32 = 1.45;

/// The scope of the preview's commands, in force while the pointer is
/// over it.
const SCOPE: ScopeId = ScopeId("preview");

pub(super) fn plugin(app: &mut App) {
    if !app.is_plugin_added::<InfiniteGridPlugin>() {
        app.add_plugins(InfiniteGridPlugin);
    }
    app.add_plugins(keys)
        .init_resource::<Seat>()
        .init_resource::<Framing>()
        .init_resource::<Backdrop>()
        .init_resource::<Daytime>()
        .init_resource::<DialOpen>()
        .add_systems(Startup, spawn)
        .add_systems(
            Update,
            (open, rebuild, grid, daylight, glide, aim).chain(),
        );
}

/// The views from the four sides: F the front, Shift+R the right, B
/// the back and L the left, as Blender's; and T from the top and
/// Shift+T from the bottom. R alone is the gizmo's Rotate.
fn keys(app: &mut App) {
    let command = |id, label, run| CommandSpec {
        id: CommandId(id),
        label,
        scope: SCOPE,
        run,
        enabled: |_| true,
        repeat: false,
    };
    app.add_scope(ScopeSpec {
        id: SCOPE,
        label: "Preview",
        layer: Layer::Panel,
        active: None,
    })
    .add_command(
        command("preview.front", "Front View", |world, _| {
            face(world, 0.0);
        }),
        &[Chord::key(KeyCode::KeyF)],
    )
    .add_command(
        command("preview.right", "Right View", |world, _| {
            face(world, FRAC_PI_2);
        }),
        &[Chord {
            key: KeyCode::KeyR,
            mods: Mods::SHIFT,
        }],
    )
    .add_command(
        command("preview.back", "Back View", |world, _| {
            face(world, PI);
        }),
        &[Chord::key(KeyCode::KeyB)],
    )
    .add_command(
        command("preview.left", "Left View", |world, _| {
            face(world, -FRAC_PI_2);
        }),
        &[Chord::key(KeyCode::KeyL)],
    )
    .add_command(
        command("preview.top", "Top View", |world, _| {
            tilt(world, FRAC_PI_2);
        }),
        &[Chord::key(KeyCode::KeyT)],
    )
    .add_command(
        command("preview.bottom", "Bottom View", |world, _| {
            tilt(world, -FRAC_PI_2);
        }),
        &[Chord {
            key: KeyCode::KeyT,
            mods: Mods::SHIFT,
        }],
    );
}

/// Turns the camera, easing, to look level at what is drawn from
/// `yaw` round from its front (+z), keeping its distance and what it
/// looks at.
fn face(world: &mut World, yaw: f32) {
    let mut seat = world.resource_mut::<Seat>();
    // The short way round from where it is turned to.
    let turn = yaw - YAW - seat.want.yaw;
    seat.want.yaw += (turn + PI).rem_euclid(TAU) - PI;
    seat.want.pitch = LOWEST - PITCH;
}

/// Turns the camera, easing, to look straight down at what is drawn
/// for a `pitch` of a quarter turn, or straight up for minus one,
/// keeping which way round it is turned, its distance and what it
/// looks at.
fn tilt(world: &mut World, pitch: f32) {
    world.resource_mut::<Seat>().want.pitch = pitch - PITCH;
}

/// The hour the preview's lighting is set for, apart from the
/// world's: midday until another is dialled.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
struct Daytime(f32);

impl FromWorld for Daytime {
    fn from_world(world: &mut World) -> Self {
        let set = world
            .get_resource::<SkySettings>()
            .cloned()
            .unwrap_or_default();
        Self(set.held(false))
    }
}

/// Whether the dial of the preview's hour is open.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
struct DialOpen(bool);

/// What is drawn behind what the graph puts down: a ground under it,
/// and a grid on the ground plane.
#[derive(Resource)]
struct Backdrop {
    ground: bool,
    grid: bool,
}

impl Default for Backdrop {
    fn default() -> Self {
        Self {
            ground: false,
            grid: true,
        }
    }
}

/// The preview's grid, seen only by its camera.
#[derive(Component)]
struct Grid;

/// Marks the frame the preview is drawn in.
#[derive(Component)]
pub(super) struct Shown;

/// What the graph put down, drawn.
#[derive(Component)]
struct Piece;

/// The camera that draws the preview, and the material of its pieces.
#[derive(Resource)]
pub(super) struct Preview {
    pub(super) camera: Entity,
    key: Entity,
    fill: Entity,
    material: Handle<StandardMaterial>,
}

/// The camera's orbit as a hand asked for it, and as it is shown,
/// easing there.
#[derive(Resource, Default)]
struct Seat {
    want: Orbit,
    shown: Orbit,
    /// The camera's distance from what it looks at and its angle
    /// below the horizon, as last aimed.
    stance: (f32, f32),
}

/// The middle and the reach of what is drawn.
#[derive(Resource, Default)]
pub(super) struct Framing {
    pub(super) middle: Vec3,
    pub(super) reach: f32,
}

/// What the last drawing was made from.
struct Drawn {
    objects: Vec<Object>,
    assets: Catalogue,
    library: u64,
}

pub(super) fn view() -> impl View {
    AnyView::new(|cx: &mut Cx| {
        let theme = cx.theme();
        let panel =
            column((error(), modes(), switches(), time_dial()))
                .width(percent(100.0))
                .fills_height()
                .overflow(Overflow::clip())
                .fill(theme.palette.input)
                .tagged(Shown)
                .tagged(Scope(SCOPE));
        match cx
            .world
            .get_resource::<Preview>()
            .map(|preview| preview.camera)
        {
            Some(camera) => {
                cx.build(panel.seeded(ViewportNode::new(camera)))
            }
            None => cx.build(panel),
        }
    })
}

/// Why the graph does not run, over the preview's corner, while it
/// shows the last drawing that did.
fn error() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let theme = cx.theme();
        // The run writes the editor's errors as it writes what it
        // got, so a change of the one is a change of both.
        let note = |world: &World| {
            let errors =
                &world.resource::<Editor<Blueprint>>().errors;
            match (&world.resource::<Got>().outcome, errors.first()) {
                (Err(error), _) => format!("Not drawn: {error}"),
                (Ok(_), Some(error)) => {
                    format!("Not all drawn: {error}")
                }
                (Ok(_), None) => String::new(),
            }
        };
        let shown = derived(move |world| {
            if note(world).is_empty() {
                Display::None
            } else {
                Display::Flex
            }
        })
        .when(moved::<Got>());
        cx.build(
            column(
                (description(derived(note).when(moved::<Got>())),),
            )
            .position(PositionType::Absolute)
            .inset(UiRect {
                left: px(BAR_INSET),
                right: px(MODES_WIDTH),
                top: px(BAR_INSET),
                bottom: Val::Auto,
            })
            .padding(UiRect::axes(px(8.0), px(4.0)))
            .fill(theme.palette.card)
            .radius(theme.space.round)
            .display(shown),
        )
    })
}

/// The switches for the ground and the grid, and the dice that roll
/// the random values picked per hamlet and per item, in the preview's
/// lower corner.
fn switches() -> AnyView {
    let dice = |word: &'static str,
                tip: &'static str,
                roll: fn(&mut Got)| {
        button(row((glyph(Glyph::Dice, ICON), label(word))).gap(4.0))
            .padding(UiRect::axes(px(8.0), px(2.0)))
            .tooltip(move || note(tip))
            .rules(styles::button)
            .tagged(Variant::Default)
            .on_activate(move |world| {
                roll(&mut world.resource_mut::<Got>());
            })
    };
    // A part of the backdrop, checked while it is shown.
    let switch = |word: &'static str,
                  shown: fn(&Backdrop) -> bool,
                  flip: fn(&mut Backdrop)| {
        checked_when(
            button(label(word))
                .padding(UiRect::axes(px(8.0), px(2.0)))
                .rules(toggle_button)
                .tagged(Variant::Default)
                .on_activate(move |world| {
                    flip(&mut world.resource_mut::<Backdrop>());
                }),
            resource::<Backdrop, _>(shown),
        )
    };
    row((
        time(),
        switch(
            "Grid",
            |backdrop| backdrop.grid,
            |backdrop| backdrop.grid = !backdrop.grid,
        ),
        switch(
            "Ground",
            |backdrop| backdrop.ground,
            |backdrop| backdrop.ground = !backdrop.ground,
        ),
        dice(
            "Hamlet",
            "Roll again what is picked once per hamlet",
            roll_hamlet,
        ),
        dice(
            "Item",
            "Roll again what is picked once per item",
            roll_item,
        ),
    ))
    .gap(4.0)
    .position(PositionType::Absolute)
    .inset(UiRect {
        left: px(BAR_INSET),
        bottom: px(BAR_INSET),
        right: Val::Auto,
        top: Val::Auto,
    })
    .boxed()
}

/// Lights the preview's camera and lights at its own hour.
fn daylight(
    mut commands: Commands,
    preview: Option<Res<Preview>>,
    daytime: Res<Daytime>,
    looks: Option<Res<Looks>>,
    set: Option<Res<SkySettings>>,
    north: Option<Res<North>>,
) {
    let (Some(preview), Some(looks), Some(set)) =
        (preview, looks, set)
    else {
        return;
    };
    let north = north.as_deref().copied().unwrap_or_default();
    if !(preview.is_added()
        || daytime.is_changed()
        || looks.is_changed()
        || set.is_changed())
    {
        return;
    }
    let own = OwnSky(Sky::at(&looks, &set, daytime.0, north));
    for entity in [preview.camera, preview.key, preview.fill] {
        commands.entity(entity).insert(own.clone());
    }
}

/// The sun or the moon of the preview's hour, read from the world.
fn sun_or_moon(world: &World) -> Glyph {
    let hour = world.resource::<Daytime>().0;
    let night = world
        .get_resource::<SkySettings>()
        .is_some_and(|set| set.is_night(hour));
    if night { Glyph::Moon } else { Glyph::Sun }
}

/// How far the preview's bar is in from the panel's lower left
/// corner.
const BAR_INSET: f32 = 8.0;

/// The side of a button of the preview's bar that holds only an icon.
const BAR_BUTTON: f32 = 28.0;

/// The button that opens the dial of the preview's hour, showing the
/// sun or the moon of it.
fn time() -> AnyView {
    keyed(derived(sun_or_moon).when(every_frame()), |which| {
        button(glyph(*which, ICON))
            .width(px(BAR_BUTTON))
            .height(px(BAR_BUTTON))
            .tooltip(|| note("The hour of the preview's lighting"))
            .rules(styles::button)
            .tagged(Variant::Default)
            .on_activate(|world| {
                let mut open = world.resource_mut::<DialOpen>();
                open.0 = !open.0;
            })
            .boxed()
    })
    .within(row(()).gap(0.0))
    .boxed()
}

/// The dial of the preview's hour, in a popup over the Time button
/// while it is open.
fn time_dial() -> AnyView {
    keyed(resource::<DialOpen, _>(|open| open.0), |open| {
        if !*open {
            return frame().boxed();
        }
        AnyView::new(|cx: &mut Cx| {
            // The button is the first of the bar, in the panel's
            // lower left corner.
            let height = cx
                .world
                .query_filtered::<&ComputedNode, With<Shown>>()
                .iter(cx.world)
                .next()
                .map_or(0.0, |node| {
                    node.size().y * node.inverse_scale_factor()
                });
            let bottom = height - BAR_INSET;
            let anchor = Rect::new(
                BAR_INSET,
                bottom - BAR_BUTTON,
                BAR_INSET + BAR_BUTTON,
                bottom,
            );
            let gap = cx.theme().space.gap;
            cx.build(
                popup(
                    anchor,
                    (column((
                        dial(
                            Face::HOURS,
                            derived(sun_or_moon).when(every_frame()),
                            resource::<Daytime, _>(|daytime| {
                                daytime.0
                            }),
                            resource::<Daytime, _>(|daytime| {
                                daytime.0
                            }),
                            |world, hour| {
                                world.resource_mut::<Daytime>().0 =
                                    hour;
                            },
                        ),
                        description("Drag to hold an hour."),
                    ))
                    .gap(gap)
                    .align(AlignItems::Center),),
                )
                .placements(vec![PopoverPlacement {
                    side: PopoverSide::Top,
                    align: PopoverAlign::Start,
                    gap,
                }])
                .on_dismiss(|world, _| {
                    world.resource_mut::<DialOpen>().0 = false;
                }),
            )
        })
    })
    .within(overlay(()))
    .boxed()
}

/// What a transform's gizmo does, each an icon, the one it does now
/// checked, in the preview's upper right corner.
fn modes() -> AnyView {
    const MODES: [(Glyph, &str, GizmoMode); 3] = [
        (Glyph::Move, "Move (G)", GizmoMode::Translate),
        (Glyph::Restore, "Rotate (R)", GizmoMode::Rotate),
        (Glyph::Scale, "Scale (S)", GizmoMode::Scale),
    ];
    let buttons = MODES
        .into_iter()
        .map(|(which, tip, picked)| {
            checked_when(
                button(glyph(which, ICON))
                    .width(px(BAR_BUTTON))
                    .height(px(BAR_BUTTON))
                    .tooltip(move || note(tip))
                    .rules(toggle_button)
                    .tagged(Variant::Default)
                    .on_activate(move |world| {
                        world.insert_resource(picked);
                    }),
                resource::<GizmoMode, _>(move |mode| *mode == picked),
            )
        })
        .collect::<Vec<_>>();
    row(buttons)
        .gap(MODE_GAP)
        .position(PositionType::Absolute)
        .inset(UiRect {
            top: px(BAR_INSET),
            right: px(BAR_INSET),
            left: Val::Auto,
            bottom: Val::Auto,
        })
        .boxed()
}

/// The space between the buttons of the gizmo's modes.
const MODE_GAP: f32 = 4.0;

/// The width the buttons of the gizmo's modes take from the preview's
/// right edge, which the error beside them keeps clear of.
const MODES_WIDTH: f32 =
    BAR_INSET + 3.0 * BAR_BUTTON + 2.0 * MODE_GAP + BAR_INSET;

/// Shows or hides the grid, spawning it once the theme gives its
/// colours.
fn grid(
    mut commands: Commands,
    backdrop: Res<Backdrop>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    mut grids: Query<&mut Visibility, With<Grid>>,
) {
    let shown = if backdrop.grid {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if grids.is_empty() {
        let palette = &theme.0.palette;
        commands.spawn((
            Grid,
            InfiniteGrid,
            InfiniteGridSettings {
                x_axis_color: palette.red,
                z_axis_color: palette.agent,
                minor_line_color: palette.border,
                major_line_color: palette.grey_1.with_alpha(0.5),
                ..InfiniteGridSettings::default()
            },
            shown,
            RenderLayers::layer(LAYER),
        ));
        return;
    }
    if backdrop.is_changed() {
        for mut visibility in &mut grids {
            *visibility = shown;
        }
    }
}

fn spawn(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    window: Query<&Window, With<PrimaryWindow>>,
    settings: Option<Res<CameraSettings>>,
) {
    let mut image = Image::new_uninit(
        Default::default(),
        TextureDimension::D2,
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::all(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::RENDER_ATTACHMENT;
    let handle = images.add(image);
    let scale =
        window.iter().next().map_or(1.0, Window::scale_factor);
    let settings = settings.as_deref().cloned().unwrap_or_default();
    let camera = commands
        .spawn((
            sky::view(&settings),
            Camera {
                // Before the world's camera and the window's.
                order: -2,
                is_active: false,
                ..Camera::default()
            },
            RenderTarget::Image(ImageRenderTarget {
                handle,
                scale_factor: scale,
            }),
            RenderLayers::layer(LAYER),
            Transform::from_xyz(8.0, 6.0, 8.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id();
    // The hour's lights, which the realm sets from the preview's own
    // hour.
    let key = commands
        .spawn((
            Hourly::Key,
            DirectionalLight {
                shadow_maps_enabled: true,
                ..DirectionalLight::default()
            },
            Transform::default(),
            RenderLayers::layer(LAYER),
        ))
        .id();
    let fill = commands
        .spawn((
            Hourly::Fill,
            DirectionalLight::default(),
            Transform::default(),
            RenderLayers::layer(LAYER),
        ))
        .id();
    let material = materials.add(StandardMaterial {
        perceptual_roughness: 0.9,
        reflectance: 0.2,
        ..StandardMaterial::default()
    });
    commands.insert_resource(Preview {
        camera,
        key,
        fill,
        material,
    });
}

/// Draws the preview while its frame is in the panel, and lets it be
/// turned by dragging.
fn open(
    mut commands: Commands,
    preview: Option<Res<Preview>>,
    shown: Query<Entity, With<Shown>>,
    mut cameras: Query<&mut Camera>,
    mut watched: Local<Option<Entity>>,
) {
    let Some(preview) = preview else {
        return;
    };
    let frame = shown.iter().next();
    if let Some(frame) = frame
        && *watched != Some(frame)
    {
        commands
            .entity(frame)
            .observe(gizmo::grab)
            .observe(gizmo::steer)
            .observe(gizmo::release)
            .observe(gizmo::press)
            .observe(gizmo::click)
            .observe(turn)
            .observe(zoom);
        *watched = Some(frame);
    }
    if let Ok(mut camera) = cameras.get_mut(preview.camera)
        && camera.is_active != frame.is_some()
    {
        camera.is_active = frame.is_some();
    }
}

/// The left button slides the ground, the right button turns and
/// tilts, as in the world.
fn turn(
    drag: On<PointerDrag>,
    frames: Query<&ComputedNode, With<Shown>>,
    set: Res<CameraSettings>,
    framing: Res<Framing>,
    mut steer: ResMut<Steer>,
    mut seat: ResMut<Seat>,
    held: Res<Dragging>,
) {
    let Ok(node) = frames.get(drag.event_target()) else {
        return;
    };
    // A drag that took hold of a handle moves the handle instead.
    if held.is_under_way() {
        return;
    }
    match drag.button {
        PointerButton::Primary => {
            steer.set_if_neq(Steer::Slide);
            let height = (node.size().y
                * node.inverse_scale_factor())
            .max(1.0);
            let scale = seat.want.slide_scale(
                height,
                Some(seat.stance),
                &set,
            );
            let pan = seat.want.pan
                + Orbit::slid(
                    drag.delta,
                    scale,
                    YAW + seat.shown.yaw,
                );
            seat.want.pan =
                pan.clamp_length_max(framing.reach * 1.5 + 4.0);
        }
        PointerButton::Secondary => {
            steer.set_if_neq(Steer::Turn);
            seat.want = seat.want.turned(drag.delta, &set);
            // A hand turns it back between level and steep, from
            // straight above or below too.
            seat.want.pitch = seat
                .want
                .pitch
                .clamp(LOWEST - PITCH, HIGHEST - PITCH);
        }
        PointerButton::Middle => {}
    }
}

fn zoom(
    scroll: On<PointerScroll>,
    set: Res<CameraSettings>,
    mut seat: ResMut<Seat>,
) {
    let lines = scrolled_lines(scroll.unit, scroll.y, &set);
    seat.want = seat.want.zoomed(lines, &set, set.zoom);
}

/// Eases the orbit that is shown toward the one asked for, as the
/// realm's camera does.
fn glide(
    time: Res<Time>,
    set: Res<CameraSettings>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    mut seat: ResMut<Seat>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if seat.shown == seat.want {
        return;
    }
    let pace =
        Pace::of(buttons.as_deref().is_some_and(|buttons| {
            buttons.pressed(MouseButton::Left)
        }));
    seat.shown =
        seat.shown.eased(seat.want, time.delta_secs(), &set, pace);
    redraw.write(RequestRedraw);
}

/// Draws what the graph put down again when it changes.
fn rebuild(
    mut commands: Commands,
    editor: Res<Editor<Blueprint>>,
    got: Res<Got>,
    preview: Option<Res<Preview>>,
    colours: Option<Res<Colours>>,
    mut library: ResMut<Library>,
    mut models: ResMut<Meshes>,
    pieces: Query<Entity, With<Piece>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut framing: ResMut<Framing>,
    backdrop: Res<Backdrop>,
    mut drawn: Local<Option<Drawn>>,
) {
    let Some(preview) = preview else {
        return;
    };
    if !editor.is_changed()
        && !got.is_changed()
        && !backdrop.is_changed()
        && drawn.is_some()
    {
        return;
    }
    // A graph that fails, as one with an input not yet wired, leaves
    // the last drawing as it was.
    let Ok(objects) = &got.outcome else {
        return;
    };
    // The editor changes with a selection and a dragged card too, and
    // the graph runs again as the selection changes, which draw the
    // same.
    if !backdrop.is_changed()
        && drawn.as_ref().is_some_and(|drawn| {
            drawn.objects == *objects
                && drawn.assets == editor.rules.assets
                && drawn.library == library.revision
        })
    {
        return;
    }
    *drawn = Some(Drawn {
        objects: objects.clone(),
        assets: editor.rules.assets.clone(),
        library: library.revision,
    });
    for piece in &pieces {
        commands.entity(piece).despawn();
    }
    let mut meshing = Meshing::new();
    let (mut low, mut high) =
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for object in objects {
        let asset = editor.rules.assets.get(&object.asset);
        let size = drawn_size(&object.frame, asset.as_ref());
        // A mesh that cannot be read is drawn as a box, and said so.
        let mut failed = None;
        let model = match (
            asset.as_ref().map(|asset| &asset.shape),
            library.root(),
        ) {
            (Some(Shape::Mesh(path)), Some(root)) => {
                match models.get(root, path) {
                    Ok(model) => Some(model),
                    Err(error) => {
                        failed =
                            Some(format!("Could not read {error:#}"));
                        None
                    }
                }
            }
            _ => None,
        };
        if let Some(error) = failed
            && library.note != error
        {
            library.note = error;
        }
        let middle = stand(
            &mut meshing,
            &object.frame,
            &object.asset,
            asset.as_ref(),
            model,
            colours.as_deref(),
        );
        low = low.min(middle - size);
        high = high.max(middle + size);
    }
    if objects.is_empty() {
        (low, high) = (Vec3::splat(-2.0), Vec3::splat(2.0));
    }
    let ground = colours
        .as_deref()
        .map_or(Vec3::splat(0.3), |palette| palette.grass);
    let span = (high - low).max(Vec3::splat(1.0));
    if backdrop.ground {
        meshing.rbox(
            Vec3::new(
                (low.x + high.x) * 0.5,
                -0.05,
                (low.z + high.z) * 0.5,
            ),
            Vec3::new(span.x * 0.5 + 0.5, 0.05, span.z * 0.5 + 0.5),
            0.02,
            Quat::IDENTITY,
            ground,
            0.0,
        );
    }
    framing.middle = (low + high) * 0.5;
    framing.reach = (high - low).length() * 0.5;
    commands.spawn((
        Piece,
        Mesh3d(meshes.add(meshing.build())),
        MeshMaterial3d(preview.material.clone()),
        RenderLayers::layer(LAYER),
    ));
}

/// The size an asset is drawn at on `frame`: its own size times the
/// frame's scale, a metre on each side where it has none.
fn drawn_size(frame: &Frame, asset: Option<&Asset>) -> Vec3 {
    frame.scale * asset.map_or(Vec3::ONE, |asset| asset.size)
}

/// The asset `name`, with the catalogue entry `asset` and the
/// `model` its file was read as, drawn standing on `frame`: a box
/// where it has no shape of its own. Gives the middle of its box.
pub(super) fn stand(
    meshing: &mut Meshing,
    frame: &Frame,
    name: &str,
    asset: Option<&Asset>,
    model: Option<&Model>,
    colours: Option<&Colours>,
) -> Vec3 {
    let size = drawn_size(frame, asset);
    let marker = name == MARKER;
    // An asset stands on its transform; a marker sits on it.
    let lift = if marker { 0.0 } else { size.y * 0.5 };
    let middle = frame.translation
        + frame.rotation * Vec3::new(0.0, lift, 0.0);
    let colour = match colours {
        Some(palette) if marker => palette.ochre,
        _ => paint(colours, asset, BODY, Vec3::splat(0.7)),
    };
    let shape = asset.map(|asset| &asset.shape);
    match (model, shape, colours) {
        (Some(model), ..) => {
            model.draw(
                meshing,
                middle,
                size,
                frame.rotation,
                |slot, own| paint(colours, asset, slot, own),
            );
        }
        (None, Some(Shape::Wedge { peak }), _) => {
            wedge(meshing, frame, size, *peak, colour);
        }
        (None, Some(Shape::Tiles { taper }), Some(palette)) => {
            tiles(meshing, frame, size, *taper, colour, palette);
        }
        _ => meshing.rbox(
            middle,
            size * 0.5,
            0.04,
            frame.rotation,
            colour,
            if marker { 0.0 } else { 0.25 },
        ),
    }
    middle
}

/// A triangle across x on `frame`, `size.y` high at `peak` along x
/// and `size.z` thick: a gable's end.
fn wedge(
    meshing: &mut Meshing,
    frame: &Frame,
    size: Vec3,
    peak: f32,
    colour: Vec3,
) {
    let at = |x: f32, y: f32, z: f32| {
        frame.translation
            + frame.rotation * (Vec3::new(x, y, z) * size)
    };
    let turn = |v: Vec3| frame.rotation * v;
    let (front, back) = (0.5, -0.5);
    let corner = |x: f32, z: f32| at(x, 0.0, z);
    let top = |z: f32| at(peak, 1.0, z);
    meshing.face(
        [
            corner(-0.5, front),
            corner(0.5, front),
            top(front),
            top(front),
        ],
        turn(Vec3::Z),
        colour,
    );
    meshing.face(
        [corner(0.5, back), corner(-0.5, back), top(back), top(back)],
        turn(-Vec3::Z),
        colour,
    );
    // Each sloping side faces out and up, steeper the nearer the
    // peak.
    let sides = [
        (-0.5, Vec3::new(-size.y, (peak + 0.5) * size.x, 0.0)),
        (0.5, Vec3::new(size.y, (0.5 - peak) * size.x, 0.0)),
    ];
    for (x, toward) in sides {
        meshing.face(
            [
                corner(x, back),
                corner(x, front),
                top(front),
                top(back),
            ],
            turn(toward),
            colour * 0.95,
        );
    }
}

/// Lapped tiles over `frame` as the game lays a roof's slope, from
/// the eave at +z to the ridge at -z, `taper` times as wide. The
/// roof's own colour drifts as the hamlet's roofs do.
fn tiles(
    meshing: &mut Meshing,
    frame: &Frame,
    size: Vec3,
    taper: f32,
    colour: Vec3,
    colours: &Colours,
) {
    // A slope flat along x or z has no eave or no run to lay on, and
    // `slope` would give its vertices NaN.
    if size.x.abs() < 1e-4 || size.z.abs() < 1e-4 {
        return;
    }
    let at = |x: f32, z: f32| {
        frame.translation
            + frame.rotation
                * (Vec3::new(x * size.x, 0.0, z * size.z))
    };
    let eave = [at(-0.5, 0.5), at(0.5, 0.5)];
    let ridge = [at(-0.5 * taper, -0.5), at(0.5 * taper, -0.5)];
    let town = &colours.towns[0];
    let style = TownStyle {
        roof: colour,
        drift: if colour == town.roof {
            town.drift
        } else {
            colour * 1.15
        },
        banner: town.banner,
    };
    // Seeded by where it is, so a slope keeps its tiles from run to
    // run.
    let mut rng = Rng::seeded(&format!("{:?}", frame.translation));
    slope(meshing, &mut rng, eave, ridge, &style, colours);
}

/// The colour of an asset's material slot, as its catalogue entry
/// paints it: a palette colour, or `own`, the one its file gives.
fn paint(
    colours: Option<&Colours>,
    asset: Option<&Asset>,
    slot: &str,
    own: Vec3,
) -> Vec3 {
    let Some(colours) = colours else {
        return Vec3::splat(0.7);
    };
    match asset.and_then(|asset| asset.slots.get(slot)) {
        Some(Paint::Palette(name)) => {
            palette_colour(colours, name).unwrap_or(colours.plaster)
        }
        Some(Paint::Own) => own,
        None => colours.plaster,
    }
}

/// Points the camera at what is drawn, from the way it is turned,
/// with the world's fog and shadows fitted to its distance.
fn aim(
    mut commands: Commands,
    preview: Option<Res<Preview>>,
    editor: Res<Editor<Blueprint>>,
    mut seat: ResMut<Seat>,
    framing: Res<Framing>,
    set: Res<CameraSettings>,
    mut cameras: Query<(&mut Transform, &mut DistanceFog)>,
    mut anchor: Local<Option<(String, Vec3, f32)>>,
) {
    let Some(preview) = preview else {
        return;
    };
    // Framed once for each graph opened, then moved only by the hand,
    // so what is drawn and the grid under it keep their size while
    // the graph is edited.
    let framed = anchor
        .as_ref()
        .is_some_and(|(name, ..)| *name == editor.name);
    if !framed && framing.reach > 0.0 {
        *anchor = Some((
            editor.name.clone(),
            framing.middle,
            framing.reach,
        ));
        seat.want = Orbit::default();
        seat.shown = Orbit::default();
    } else if !seat.is_changed() {
        return;
    }
    let Some((_, middle, reach)) = anchor.clone() else {
        return;
    };
    let Ok((mut transform, mut fog)) =
        cameras.get_mut(preview.camera)
    else {
        return;
    };
    let yaw = YAW + seat.shown.yaw;
    let pitch = (PITCH + seat.shown.pitch).clamp(-FRAC_PI_2, FRAC_PI_2);
    let toward = backward(yaw, pitch);
    // Up across the view, square to where it looks, so that it holds
    // looking straight down or up too.
    let up = Vec3::new(
        -yaw.sin() * pitch.sin(),
        pitch.cos(),
        -yaw.cos() * pitch.sin(),
    );
    let distance = (reach * 2.2 + 2.0) * seat.shown.zoom;
    let slid = seat.shown.pan;
    let focus = middle + Vec3::new(slid.x, 0.0, slid.y);
    *transform =
        Transform::from_translation(focus + toward * distance)
            .looking_at(focus, up);
    seat.stance = (distance, pitch);
    fog.falloff = sky::fog_for(&set, distance);
    commands
        .entity(preview.key)
        .try_insert(sky::shadows_for(&set, distance));
}
