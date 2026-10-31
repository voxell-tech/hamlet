//! The preview: what the graph puts down, as boxes in the world's
//! colours, drawn by a camera of its own into the panel. The camera,
//! its post processing, the hour's lights and the orbit by hand are
//! the realm's, on a render layer of their own.

use bevy::app::{App, Startup, Update};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Camera, ImageRenderTarget, RenderTarget};
use bevy::ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
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
use bevy::ui::{ComputedNode, Overflow, percent};
use bevy::window::{PrimaryWindow, RequestRedraw, Window};
use bevy_fynix::views::{BehaviorExt, frame};
use hamlet_blueprint::{Asset, BODY, Frame, Paint, Shape};
use hamlet_client::ViewFolder;
use hamlet_realm::generator::house::slope;
use hamlet_realm::meshes::{Meshing, Rng};
use hamlet_realm::palette::{Palette as Colours, TownStyle};
use hamlet_realm::pick::{Orbit, Steer, backward, scrolled_lines};
use hamlet_realm::sky::{self, CameraSettings, Hourly};
use hamlet_ui::styles::Fills;
use hamlet_ui::{AnyView, Cx, View};

use super::assets::{Meshes, palette_colour};
use super::{Editor, files};

/// The asset name a placement is shown as when nothing is put there.
pub(super) const MARKER: &str = "marker";

/// The render layer only the preview's camera, lights and pieces are
/// on. Not 7, which the realm's debug overlay puts its camera on.
pub(super) const LAYER: usize = 8;

/// The way the camera looks from before a hand has turned it, in
/// radians.
const YAW: f32 = 0.8;
const PITCH: f32 = 0.55;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Seat>()
        .init_resource::<Framing>()
        .add_systems(Startup, spawn)
        .add_systems(Update, (open, rebuild, glide, aim).chain());
}

/// Marks the frame the preview is drawn in.
#[derive(Component)]
struct Shown;

/// What the graph put down, drawn.
#[derive(Component)]
struct Piece;

/// The camera that draws the preview, and the material of its pieces.
#[derive(Resource)]
struct Preview {
    camera: Entity,
    key: Entity,
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

pub(super) fn view() -> impl View {
    AnyView::new(|cx: &mut Cx| {
        let theme = cx.theme();
        let panel = frame()
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
    // The hour's lights, which the realm sets for the world's and
    // for these alike.
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
    commands.spawn((
        Hourly::Fill,
        DirectionalLight::default(),
        Transform::default(),
        RenderLayers::layer(LAYER),
    ));
    let material = materials.add(StandardMaterial {
        perceptual_roughness: 0.9,
        reflectance: 0.2,
        ..StandardMaterial::default()
    });
    commands.insert_resource(Preview {
        camera,
        key,
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
        commands.entity(frame).observe(turn).observe(zoom);
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
) {
    let Ok(node) = frames.get(drag.event_target()) else {
        return;
    };
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
    // A hand on the ground is followed closely, but not in steps.
    let held = buttons
        .as_deref()
        .is_some_and(|buttons| buttons.pressed(MouseButton::Left));
    seat.shown =
        seat.shown.eased(seat.want, time.delta_secs(), &set, held);
    redraw.write(RequestRedraw);
}

/// Draws what the graph put down again when it changes.
fn rebuild(
    mut commands: Commands,
    editor: Res<Editor>,
    preview: Option<Res<Preview>>,
    colours: Option<Res<Colours>>,
    folder: Option<Res<ViewFolder>>,
    mut models: ResMut<Meshes>,
    pieces: Query<Entity, With<Piece>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut framing: ResMut<Framing>,
    mut drawn: Local<bool>,
) {
    let Some(preview) = preview else {
        return;
    };
    if !editor.is_changed() && *drawn {
        return;
    }
    *drawn = true;
    for piece in &pieces {
        commands.entity(piece).despawn();
    }
    let Ok(instances) = &editor.outcome else {
        return;
    };
    let mut meshing = Meshing::new();
    let (mut low, mut high) =
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for instance in instances {
        let size = instance.size * instance.frame.scale;
        let marker = instance.asset == MARKER;
        // An asset stands on its placement; a marker sits on it.
        let lift = if marker { 0.0 } else { size.y * 0.5 };
        let middle = instance.frame.translation
            + instance.frame.rotation * Vec3::new(0.0, lift, 0.0);
        let asset = editor.rules.assets.get(&instance.asset);
        let model =
            match (asset.as_ref().map(|asset| &asset.shape), &folder)
            {
                (Some(Shape::Mesh(path)), Some(folder)) => models
                    .get(&files::book_folder(&folder.0), path)
                    .ok(),
                _ => None,
            };
        let colours = colours.as_deref();
        let colour = match colours {
            Some(c) if marker => c.ochre,
            _ => {
                paint(colours, asset.as_ref(), BODY, Vec3::splat(0.7))
            }
        };
        let shape = asset.as_ref().map(|asset| &asset.shape);
        match (model, shape, colours) {
            (Some(model), ..) => model.draw(
                &mut meshing,
                middle,
                size,
                instance.frame.rotation,
                |slot, own| paint(colours, asset.as_ref(), slot, own),
            ),
            (None, Some(Shape::Wedge { peak }), _) => {
                wedge(
                    &mut meshing,
                    &instance.frame,
                    size,
                    *peak,
                    colour,
                );
            }
            (None, Some(Shape::Tiles { taper }), Some(c)) => tiles(
                &mut meshing,
                &instance.frame,
                size,
                *taper,
                colour,
                c,
            ),
            _ => meshing.rbox(
                middle,
                size * 0.5,
                0.04,
                instance.frame.rotation,
                colour,
                if marker { 0.0 } else { 0.25 },
            ),
        }
        low = low.min(middle - size);
        high = high.max(middle + size);
    }
    if instances.is_empty() {
        (low, high) = (Vec3::splat(-2.0), Vec3::splat(2.0));
    }
    let ground =
        colours.as_deref().map_or(Vec3::splat(0.3), |c| c.grass);
    let span = (high - low).max(Vec3::splat(1.0));
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
    framing.middle = (low + high) * 0.5;
    framing.reach = (high - low).length() * 0.5;
    commands.spawn((
        Piece,
        Mesh3d(meshes.add(meshing.build())),
        MeshMaterial3d(preview.material.clone()),
        RenderLayers::layer(LAYER),
    ));
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
    c: &Colours,
) {
    let at = |x: f32, z: f32| {
        frame.translation
            + frame.rotation
                * (Vec3::new(x * size.x, 0.0, z * size.z))
    };
    let eave = [at(-0.5, 0.5), at(0.5, 0.5)];
    let ridge = [at(-0.5 * taper, -0.5), at(0.5 * taper, -0.5)];
    let town = &c.towns[0];
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
    slope(meshing, &mut rng, eave, ridge, &style, c);
}

/// The colour of an asset's material slot, as its catalogue entry
/// paints it: a palette colour, or `own`, the one its file gives.
fn paint(
    colours: Option<&Colours>,
    asset: Option<&Asset>,
    slot: &str,
    own: Vec3,
) -> Vec3 {
    let Some(c) = colours else {
        return Vec3::splat(0.7);
    };
    match asset.and_then(|asset| asset.slots.get(slot)) {
        Some(Paint::Palette(name)) => {
            palette_colour(c, name).unwrap_or(c.plaster)
        }
        Some(Paint::Own) => own,
        None => c.plaster,
    }
}

/// Points the camera at what is drawn, from the way it is turned,
/// with the world's fog and shadows fitted to its distance.
fn aim(
    mut commands: Commands,
    preview: Option<Res<Preview>>,
    mut seat: ResMut<Seat>,
    framing: Res<Framing>,
    set: Res<CameraSettings>,
    mut cameras: Query<(&mut Transform, &mut DistanceFog)>,
) {
    let Some(preview) = preview else {
        return;
    };
    if !seat.is_changed() && !framing.is_changed() {
        return;
    }
    let Ok((mut transform, mut fog)) =
        cameras.get_mut(preview.camera)
    else {
        return;
    };
    let yaw = YAW + seat.shown.yaw;
    let pitch = (PITCH + seat.shown.pitch).clamp(0.1, 1.45);
    let toward = backward(yaw, pitch);
    let distance = (framing.reach * 2.2 + 2.0) * seat.shown.zoom;
    let slid = seat.shown.pan;
    let focus = framing.middle + Vec3::new(slid.x, 0.0, slid.y);
    *transform =
        Transform::from_translation(focus + toward * distance)
            .looking_at(focus, Vec3::Y);
    seat.stance = (distance, pitch);
    fog.falloff = sky::fog_for(&set, distance);
    commands
        .entity(preview.key)
        .try_insert(sky::shadows_for(&set, distance));
}
