//! The preview: what the graph puts down, as boxes in the world's
//! colours, drawn by a camera of its own into the panel. The camera,
//! its post processing, the hour's lights and the orbit by hand are
//! the realm's, on a render layer of their own.

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
use bevy::input::mouse::MouseButton;
use bevy::light::DirectionalLight;
use bevy::math::{Quat, Vec3};
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
use bevy::window::{PrimaryWindow, RequestRedraw, Window};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, label, row,
    slider,
};
use bevy_fynix::{ScopedExt, Theme, ViewExt, resource};
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
use hamlet_ui::styles::{self, Fills, ICON, description, note};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View};

use super::Blueprint;
use super::assets::{Meshes, Model, palette_colour};
use super::blueprint::{roll_hamlet, roll_item};
use super::gizmo::{self, Dragging, GizmoMode};

/// The asset name a transform is shown as when nothing is put there.
pub(super) const MARKER: &str = "marker";

/// The render layer only the preview's camera, lights and pieces are
/// on. Not 7, which the realm's debug overlay puts its camera on.
pub(super) const LAYER: usize = 8;

/// The camera's yaw before a hand has turned it, in radians.
const YAW: f32 = 0.8;

/// The camera's pitch before a hand has turned it, in radians.
const PITCH: f32 = 0.55;

pub(super) fn plugin(app: &mut App) {
    if !app.is_plugin_added::<InfiniteGridPlugin>() {
        app.add_plugins(InfiniteGridPlugin);
    }
    app.init_resource::<Seat>()
        .init_resource::<Framing>()
        .init_resource::<Backdrop>()
        .init_resource::<Daytime>()
        .add_systems(Startup, spawn)
        .add_systems(
            Update,
            (open, rebuild, grid, daylight, glide, aim).chain(),
        );
}

/// The hour the preview is lit at, apart from the world's, and its
/// name when it was chosen by one: midday until another is chosen.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
struct Daytime {
    hour: f32,
    name: Option<&'static str>,
}

impl FromWorld for Daytime {
    fn from_world(world: &mut World) -> Self {
        let set = world
            .get_resource::<SkySettings>()
            .cloned()
            .unwrap_or_default();
        let (name, hour) = named_hours(&set)[0];
        Self {
            hour,
            name: Some(name),
        }
    }
}

/// The hours the Time button steps through, by name.
fn named_hours(set: &SkySettings) -> [(&'static str, f32); 4] {
    [
        ("Day", set.held(false)),
        ("Dusk", set.sunset),
        ("Night", set.held(true)),
        ("Dawn", set.sunrise),
    ]
}

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
        let panel = column((error(), switches()))
            .width(percent(100.0))
            .fills_height()
            .overflow(Overflow::clip())
            .fill(theme.palette.input)
            .tagged(Shown);
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
        let shown = resource::<Editor<Blueprint>, _>(|editor| {
            if editor.got.outcome.is_ok() && editor.errors.is_empty()
            {
                Display::None
            } else {
                Display::Flex
            }
        });
        cx.build(
            column((description(resource::<Editor<Blueprint>, _>(
                |editor| match (
                    &editor.got.outcome,
                    editor.errors.first(),
                ) {
                    (Err(error), _) => {
                        format!("Not drawn: {error}")
                    }
                    (Ok(_), Some(error)) => {
                        format!("Not all drawn: {error}")
                    }
                    (Ok(_), None) => String::new(),
                },
            )),))
            .position(PositionType::Absolute)
            .inset(UiRect {
                left: px(8.0),
                right: px(8.0),
                top: px(8.0),
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
    // What a transform's gizmo edits, the one it edits now marked.
    let mode =
        |word: &'static str, key: &'static str, picked: GizmoMode| {
            button(label(resource::<GizmoMode, _>(move |mode| {
                if *mode == picked {
                    format!("• {word}")
                } else {
                    word.to_owned()
                }
            })))
            .padding(UiRect::axes(px(8.0), px(2.0)))
            .tooltip(move || {
                note(format!("What a transform's gizmo does ({key})"))
            })
            .rules(styles::button)
            .tagged(Variant::Default)
            .on_activate(move |world| {
                world.insert_resource(picked);
            })
        };
    let dice = |word: &'static str,
                tip: &'static str,
                roll: fn(&mut Editor<Blueprint>)| {
        button(row((glyph(Glyph::Dice, ICON), label(word))).gap(4.0))
            .padding(UiRect::axes(px(8.0), px(2.0)))
            .tooltip(move || note(tip))
            .rules(styles::button)
            .tagged(Variant::Default)
            .on_activate(move |world| {
                roll(&mut world.resource_mut::<Editor<Blueprint>>());
            })
    };
    let switch = |word: &'static str, flip: fn(&mut Backdrop)| {
        button(label(resource::<Backdrop, _>(move |backdrop| {
            let on = match word {
                "Ground" => backdrop.ground,
                _ => backdrop.grid,
            };
            format!("{word}: {}", if on { "on" } else { "off" })
        })))
        .padding(UiRect::axes(px(8.0), px(2.0)))
        .rules(styles::button)
        .tagged(Variant::Default)
        .on_activate(move |world| {
            flip(&mut world.resource_mut::<Backdrop>());
        })
    };
    row((
        time(),
        switch("Grid", |backdrop| backdrop.grid = !backdrop.grid),
        switch("Ground", |backdrop| {
            backdrop.ground = !backdrop.ground;
        }),
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
        mode("Move", "W", GizmoMode::Translate),
        mode("Turn", "E", GizmoMode::Rotate),
        mode("Scale", "R", GizmoMode::Scale),
    ))
    .gap(4.0)
    .position(PositionType::Absolute)
    .inset(UiRect {
        left: px(8.0),
        bottom: px(8.0),
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
    let own = OwnSky(Sky::at(&looks, &set, daytime.hour, north));
    for entity in [preview.camera, preview.key, preview.fill] {
        commands.entity(entity).insert(own.clone());
    }
}

/// The Time button, which steps through the named hours, and the
/// slider for any hour between.
fn time() -> AnyView {
    let named =
        row((button(label(resource::<Daytime, _>(|daytime| {
            format!("Time: {}", hour_name(daytime))
        })))
        .padding(UiRect::axes(px(8.0), px(2.0)))
        .tooltip(|| note("The hour the preview is lit at"))
        .rules(styles::button)
        .tagged(Variant::Default)
        .on_activate(|world| {
            let set = world
                .get_resource::<SkySettings>()
                .cloned()
                .unwrap_or_default();
            let hours = named_hours(&set);
            let now = world.resource::<Daytime>().name;
            let next = hours
                .iter()
                .position(|(name, _)| Some(*name) == now)
                .map_or(0, |at| (at + 1) % hours.len());
            let (name, hour) = hours[next];
            *world.resource_mut::<Daytime>() = Daytime {
                hour,
                name: Some(name),
            };
        }),));
    row((
        named,
        slider(resource::<Daytime, _>(|daytime| daytime.hour))
            .range(0.0, 24.0)
            .on_change(|world, hour| {
                *world.resource_mut::<Daytime>() =
                    Daytime { hour, name: None };
            })
            .width(px(TIME_SLIDER)),
    ))
    .gap(6.0)
    .align(AlignItems::Center)
    .boxed()
}

/// The width of the slider for the preview's hour.
const TIME_SLIDER: f32 = 96.0;

/// The name `daytime` was chosen by, else its hour and minute.
fn hour_name(daytime: &Daytime) -> String {
    if let Some(name) = daytime.name {
        return name.to_owned();
    }
    let minutes = (daytime.hour * 60.0).round() as u32 % (24 * 60);
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

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
        && !backdrop.is_changed()
        && drawn.is_some()
    {
        return;
    }
    // A graph that fails, as one with an input not yet wired, leaves
    // the last drawing as it was.
    let Ok(objects) = &editor.got.outcome else {
        return;
    };
    // The editor changes with a selection and a dragged card too,
    // which draw the same.
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
        let size = object.frame.scale;
        let asset = editor.rules.assets.get(&object.asset);
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
    let size = frame.scale;
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
    let pitch = (PITCH + seat.shown.pitch).clamp(0.1, 1.45);
    let toward = backward(yaw, pitch);
    let distance = (reach * 2.2 + 2.0) * seat.shown.zoom;
    let slid = seat.shown.pan;
    let focus = middle + Vec3::new(slid.x, 0.0, slid.y);
    *transform =
        Transform::from_translation(focus + toward * distance)
            .looking_at(focus, Vec3::Y);
    seat.stance = (distance, pitch);
    fog.falloff = sky::fog_for(&set, distance);
    commands
        .entity(preview.key)
        .try_insert(sky::shadows_for(&set, distance));
}
