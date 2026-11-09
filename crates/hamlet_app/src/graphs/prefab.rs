//! The PREFAB panel: a mesh of the library as Blender made it, its
//! objects in a hierarchy, and an inspector that gives the selected
//! one a shader graph or a material and moves, turns and scales it.
//! The view draws the model in the realm's light through its own
//! camera: dragging turns it, dragging with the middle button moves
//! it, and scrolling brings it nearer. Each change is written to the
//! prefab's file at once, as a material's is. The model follows its
//! `.glb`: saving the file again in Blender redraws the prefab, and
//! the parts keep their settings.

mod model;
mod surface;

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Context;
use bevy::app::{App, Startup, Update};
use bevy::asset::uuid::Uuid;
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{RenderLayers, Visibility};
use bevy::camera::{
    Camera, ImageRenderTarget, PerspectiveProjection, Projection,
    RenderTarget,
};
use bevy::color::{Color, LinearRgba};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::gizmos::AppGizmoBuilder;
use bevy::gizmos::config::{GizmoConfigGroup, GizmoConfigStore};
use bevy::gizmos::gizmos::Gizmos;
use bevy::image::Image;
use bevy::light::DirectionalLight;
use bevy::material::AlphaMode;
use bevy::math::{EulerRot, Quat, Vec3};
use bevy::mesh::{Mesh, Mesh3d, VertexAttributeValues};
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::picking::events::{PointerDrag, PointerScroll};
use bevy::picking::pointer::PointerButton;
use bevy::reflect::Reflect;
use bevy::render::render_resource::{
    TextureDimension, TextureFormat, TextureUsages,
};
use bevy::time::Time;
use bevy::transform::components::{GlobalTransform, Transform};
use bevy::ui::widget::ViewportNode;
use bevy::ui::{AlignItems, Node, Overflow, UiRect, percent, px};
use bevy::window::{PrimaryWindow, RequestRedraw, Window};
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    label, number_field, row, scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_graph::Graph;
use hamlet_graph::prefab::{Finish, Part, Prefab};
use hamlet_graph_editor::library::{Kind, Library, Source};
use hamlet_realm::sky::{self, CameraSettings, Hourly};
use hamlet_shader::Compiled;
use hamlet_ui::styles::{
    self, ICON, description, labelled, note, section, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx};

use self::model::Model;
use self::surface::{
    GraphExt, GraphMaterial, Shaded, Shaders, shaded,
};
use crate::dock::{PanelAppExt, flush_panel};
use crate::graphs::shader::swatch::Slots;

/// The render layer only the prefab view's camera, lights and model
/// are on: apart from the blueprint preview's 8, the shader preview's
/// 9, the thumbnails' 10 and the material preview's 11.
const LAYER: usize = 12;

/// The width of the hierarchy and of the inspector, in pixels.
const SIDE: f32 = 280.0;

/// How the camera looks at the model before it is turned.
const YAW: f32 = 0.7;
const PITCH: f32 = 0.35;

pub(super) fn plugin(app: &mut App) {
    app.add_panel(
        "prefab",
        DockWindowKind::new("PREFAB", || flush_panel(screen()))
            .icon(Glyph::Hamlet.image(ICON)),
    )
    .init_resource::<Open>()
    .init_resource::<Orbit>()
    .init_gizmo_group::<PrefabGizmos>()
    .add_plugins(surface::plugin)
    .add_systems(Startup, (spawn, style_outline))
    .add_systems(
        Update,
        (follow, build, feed, open_view, pose, outline),
    );
}

/// The prefab open in the panel, its model as last read, and what is
/// selected in it.
#[derive(Resource, Default)]
pub(crate) struct Open {
    name: Option<String>,
    prefab: Prefab,
    model: Option<Loaded>,
    /// Why the model could not be read.
    error: String,
    /// The path of the selected object.
    selected: Option<String>,
    /// The objects whose children the hierarchy shows.
    expanded: BTreeSet<String>,
    /// Changes when the hierarchy and the inspector are built again.
    version: u64,
    /// Whether the model is drawn again on the next frame.
    stale: bool,
    /// The last thing done, or its Error.
    note: String,
    /// Why each part's shader graph cannot be drawn, by its path.
    errors: HashMap<String, String>,
    /// Each material drawn whose graph reads the time, with the
    /// graph, to fill its slots again each frame.
    timed: Vec<(Handle<GraphMaterial>, Compiled, Graph)>,
}

/// A model read from its file, with what is drawn of it.
struct Loaded {
    model: Model,
    file: PathBuf,
    stamp: Option<SystemTime>,
    /// Each object's meshes, made the first time it is drawn.
    meshes: Vec<Vec<Handle<Mesh>>>,
    images: Vec<Option<Handle<Image>>>,
    /// The entity of each object, and of each of its meshes.
    entities: Vec<(Entity, Vec<Entity>)>,
    /// The middle of the box around it, and the distance from there
    /// to its corners.
    middle: Vec3,
    radius: f32,
}

impl Open {
    fn changed(&mut self) {
        self.version += 1;
    }

    /// The part of the selected object, if one is.
    fn part(&self) -> Option<Part> {
        self.selected.as_ref().map(|path| self.prefab.part(path))
    }
}

/// Opens the library's prefab `name` in the panel.
pub(crate) fn open(
    world: &mut World,
    name: &str,
) -> anyhow::Result<()> {
    let item = world
        .resource::<Library>()
        .find(Kind::Prefab, name)
        .with_context(|| {
        format!("there is no prefab named {name}")
    })?;
    let prefab = item.prefab()?;
    let mut open = world.resource_mut::<Open>();
    open.name = Some(name.to_owned());
    open.prefab = prefab;
    open.selected = None;
    open.expanded.clear();
    open.note = format!("Opened {name}");
    open.model = None;
    reload(world);
    Ok(())
}

/// Reads the open prefab's model again from its file.
fn reload(world: &mut World) {
    let model = world.resource::<Open>().prefab.model.clone();
    let file = world
        .resource::<Library>()
        .find(Kind::Mesh, &model)
        .and_then(|item| match item.source {
            Source::File(path) => Some(path),
            _ => None,
        });
    let read = match (&file, model.is_empty()) {
        (_, true) => Err("Choose the mesh it is made of.".to_owned()),
        (None, false) => Err(format!(
            "Error: there is no mesh named {model} in the library"
        )),
        (Some(file), false) => fs::read(file)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| model::read(&bytes))
            .map_err(|error| format!("Error: {model}: {error:#}")),
    };
    let mut open = world.resource_mut::<Open>();
    match read.and_then(|model| {
        file.map(|file| (model, file))
            .ok_or_else(|| "Error: the mesh has no file".to_owned())
    }) {
        Ok((model, file)) => {
            let (middle, radius) = bounds(&model);
            open.model = Some(Loaded {
                stamp: stamp(&file),
                file,
                meshes: Vec::new(),
                images: Vec::new(),
                entities: Vec::new(),
                middle,
                radius,
                model,
            });
            open.error.clear();
            let roots: Vec<String> = open
                .model
                .iter()
                .flat_map(|loaded| &loaded.model.objects)
                .filter(|object| object.parent.is_none())
                .map(|object| object.path.clone())
                .collect();
            if open.expanded.is_empty() {
                open.expanded.extend(roots);
            }
        }
        Err(error) => {
            open.model = None;
            open.error = error;
        }
    }
    open.stale = true;
    open.changed();
    world.resource_mut::<Orbit>().fitted = false;
}

fn stamp(file: &PathBuf) -> Option<SystemTime> {
    fs::metadata(file).ok()?.modified().ok()
}

/// The middle of the box around every mesh of `model`, placed as its
/// objects place them, and the distance from there to its corners.
fn bounds(model: &Model) -> (Vec3, f32) {
    let mut world = Vec::with_capacity(model.objects.len());
    let (mut low, mut high) =
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for object in &model.objects {
        let above = object
            .parent
            .map_or(GlobalTransform::IDENTITY, |at| world[at]);
        let here = above.mul_transform(object.local);
        for piece in &object.pieces {
            if let Some(VertexAttributeValues::Float32x3(positions)) =
                piece.mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            {
                for p in positions {
                    let p = here.transform_point(Vec3::from(*p));
                    low = low.min(p);
                    high = high.max(p);
                }
            }
        }
        world.push(here);
    }
    if low.x > high.x {
        return (Vec3::ZERO, 1.0);
    }
    ((low + high) * 0.5, ((high - low) * 0.5).length().max(0.05))
}

/// Writes the open prefab to its file, and keeps why it could not be
/// as the note.
fn save(world: &mut World) {
    let (name, prefab) = {
        let open = world.resource::<Open>();
        let Some(name) = open.name.clone() else {
            return;
        };
        (name, open.prefab.clone())
    };
    let saved =
        world.resource_mut::<Library>().save_prefab(&name, &prefab);
    if let Err(error) = saved {
        world.resource_mut::<Open>().note =
            format!("Error: {error:#}");
    }
}

/// Changes the selected object's part as `edit` says, writes the
/// prefab, and draws the object again: all of the model when its
/// finish changes, else only its transform.
fn edit_part(world: &mut World, edit: impl FnOnce(&mut Part)) {
    let mut open = world.resource_mut::<Open>();
    let Some(path) = open.selected.clone() else {
        return;
    };
    let mut part = open.prefab.part(&path);
    let finish = part.finish.clone();
    edit(&mut part);
    let refinished = part.finish != finish;
    // Only the object's transform changes: it is set in place, so a
    // field being dragged is not built again under the pointer.
    let moved = open.model.as_ref().and_then(|loaded| {
        let at = loaded.model.find(&path)?;
        let (entity, _) = loaded.entities.get(at)?;
        Some((*entity, placed(loaded.model.objects[at].local, &part)))
    });
    open.prefab.set(&path, part);
    if refinished {
        open.stale = true;
        open.changed();
    }
    if !refinished
        && let Some((entity, transform)) = moved
        && let Some(mut at) = world.get_mut::<Transform>(entity)
    {
        *at = transform;
        world.write_message(RequestRedraw);
    }
    save(world);
}

/// Reads the model again when its file changes, and draws it again
/// when a shader graph or material a part uses changes.
fn follow(
    mut commands: Commands,
    time: Res<Time>,
    library: Res<Library>,
    open: Res<Open>,
    mut last: Local<f32>,
    mut drawn: Local<HashMap<Finish, Option<Graph>>>,
) {
    if open.name.is_none() || time.elapsed_secs() - *last < 1.0 {
        return;
    }
    *last = time.elapsed_secs();
    let moved = open
        .model
        .as_ref()
        .is_some_and(|loaded| stamp(&loaded.file) != loaded.stamp);
    let mut regraphed = false;
    let finishes: BTreeSet<&Finish> = open
        .prefab
        .parts
        .values()
        .map(|part| &part.finish)
        .filter(|finish| !finish.is_own())
        .collect();
    drawn.retain(|finish, _| finishes.contains(finish));
    for finish in finishes {
        let now = shaded(&library, finish)
            .and_then(Result::ok)
            .map(|shaded| shaded.graph);
        if drawn.get(finish) != Some(&now) {
            regraphed |= drawn.contains_key(finish);
            drawn.insert(finish.clone(), now);
        }
    }
    if moved {
        commands.queue(reload);
    } else if regraphed {
        commands.queue(|world: &mut World| {
            world.resource_mut::<Open>().stale = true;
        });
    }
}

/// The camera, its picture and its key light.
#[derive(Resource)]
struct Stage {
    camera: Entity,
    key: Entity,
}

/// Marks what is drawn of the model, to be taken away when it is
/// drawn again.
#[derive(Component)]
struct Drawn;

/// The selected object's box, seen only by the view's camera.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct PrefabGizmos;

fn style_outline(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<PrefabGizmos>();
    config.render_layers = RenderLayers::layer(LAYER);
    config.line.width = 2.0;
    config.depth_bias = -1.0;
}

fn spawn(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
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
                order: -3,
                is_active: false,
                ..Camera::default()
            },
            RenderTarget::Image(ImageRenderTarget {
                handle,
                scale_factor: scale,
            }),
            RenderLayers::layer(LAYER),
            Transform::from_xyz(0.0, 0.0, 5.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .insert(Projection::Perspective(PerspectiveProjection {
            fov: settings.fov.to_radians(),
            near: 0.02,
            far: 2000.0,
            ..PerspectiveProjection::default()
        }))
        .id();
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
    commands.insert_resource(Stage { camera, key });
}

/// The transform of `object` with its part's offset, turn and scale
/// on top.
fn placed(local: Transform, part: &Part) -> Transform {
    Transform {
        translation: local.translation + part.offset,
        rotation: local.rotation * part.turn,
        scale: local.scale * part.scale,
    }
}

/// Draws the open model again: its objects as entities placed as
/// their parts say, each mesh in the file's material or the shader
/// graph its part gives it.
#[expect(clippy::too_many_arguments, reason = "a system")]
fn build(
    mut commands: Commands,
    mut open: ResMut<Open>,
    library: Res<Library>,
    time: Res<Time>,
    drawn: Query<Entity, With<Drawn>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut graphs: ResMut<Assets<GraphMaterial>>,
    mut compiled: ResMut<Shaders>,
    mut shaders: ResMut<Assets<bevy::shader::Shader>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if !open.stale {
        return;
    }
    open.stale = false;
    redraw.write(RequestRedraw);
    for entity in &drawn {
        commands.entity(entity).despawn();
    }
    let open = &mut *open;
    open.errors.clear();
    open.timed.clear();
    let Some(loaded) = open.model.as_mut() else {
        return;
    };
    if loaded.meshes.is_empty() {
        loaded.meshes = loaded
            .model
            .objects
            .iter()
            .map(|object| {
                object
                    .pieces
                    .iter()
                    .map(|piece| meshes.add(piece.mesh.clone()))
                    .collect()
            })
            .collect();
        loaded.images = loaded
            .model
            .images
            .iter()
            .map(|image| image.clone().map(|image| images.add(image)))
            .collect();
    }
    let layer = RenderLayers::layer(LAYER);
    let root = commands
        .spawn((
            Drawn,
            Transform::IDENTITY,
            Visibility::default(),
            layer.clone(),
        ))
        .id();
    let mut cache: HashMap<Finish, Result<(Shaded, Uuid), String>> =
        HashMap::new();
    loaded.entities.clear();
    for (at, object) in loaded.model.objects.iter().enumerate() {
        let part = open.prefab.part(&object.path);
        let parent = object
            .parent
            .and_then(|above| loaded.entities.get(above))
            .map_or(root, |(entity, _)| *entity);
        let entity = commands
            .spawn((
                placed(object.local, &part),
                Visibility::default(),
                layer.clone(),
                ChildOf(parent),
            ))
            .id();
        let graph = shaded_cached(
            &mut cache,
            &library,
            &part.finish,
            &mut compiled,
            &mut shaders,
        );
        if let Some(Err(error)) = &graph {
            open.errors.insert(object.path.clone(), error.clone());
        }
        let mut pieces = Vec::new();
        for (piece, mesh) in
            object.pieces.iter().zip(&loaded.meshes[at])
        {
            let own = &piece.own;
            let texture = own.texture.and_then(|index| {
                loaded.images.get(index).cloned().flatten()
            });
            let base = StandardMaterial {
                base_color: Color::linear_rgba(
                    own.base[0],
                    own.base[1],
                    own.base[2],
                    own.base[3],
                ),
                base_color_texture: texture,
                metallic: own.metallic,
                perceptual_roughness: own.roughness,
                emissive: LinearRgba::rgb(
                    own.emissive[0],
                    own.emissive[1],
                    own.emissive[2],
                ),
                alpha_mode: if own.base[3] < 1.0 {
                    AlphaMode::Blend
                } else {
                    AlphaMode::Opaque
                },
                ..StandardMaterial::default()
            };
            let mut drawn = commands.spawn((
                Mesh3d(mesh.clone()),
                Transform::IDENTITY,
                layer.clone(),
                ChildOf(entity),
            ));
            match &graph {
                Some(Ok((shaded, shader))) => {
                    let material = graphs.add(GraphMaterial {
                        base,
                        extension: GraphExt {
                            slots: Slots::of(
                                &shaded.compiled.values(
                                    &shaded.graph,
                                    time.elapsed_secs(),
                                ),
                            ),
                            shader: *shader,
                        },
                    });
                    if shaded.compiled.reads_time() {
                        open.timed.push((
                            material.clone(),
                            shaded.compiled.clone(),
                            shaded.graph.clone(),
                        ));
                    }
                    drawn.insert(MeshMaterial3d(material));
                }
                _ => {
                    drawn.insert(MeshMaterial3d(standard.add(base)));
                }
            }
            pieces.push(drawn.id());
        }
        loaded.entities.push((entity, pieces));
    }
    open.changed();
}

/// The graph `finish` draws with and its shader, compiled once for
/// all the parts drawn now that use it.
fn shaded_cached(
    cache: &mut HashMap<Finish, Result<(Shaded, Uuid), String>>,
    library: &Library,
    finish: &Finish,
    compiled: &mut Shaders,
    shaders: &mut Assets<bevy::shader::Shader>,
) -> Option<Result<(Shaded, Uuid), String>> {
    if finish.is_own() {
        return None;
    }
    let got = cache.entry(finish.clone()).or_insert_with(|| {
        match shaded(library, finish) {
            Some(Ok(shaded)) => {
                let shader = compiled.of(&shaded.compiled, shaders);
                Ok((shaded, shader))
            }
            Some(Err(error)) => Err(format!("{error:#}")),
            None => Err("it has no graph".to_owned()),
        }
    });
    Some(match got {
        Ok((shaded, shader)) => Ok((
            Shaded {
                compiled: shaded.compiled.clone(),
                graph: shaded.graph.clone(),
            },
            *shader,
        )),
        Err(error) => Err(error.clone()),
    })
}

/// Fills again, each frame, the slots of every material whose graph
/// reads the time.
fn feed(
    time: Res<Time>,
    open: Res<Open>,
    mut graphs: ResMut<Assets<GraphMaterial>>,
) {
    for (handle, compiled, graph) in &open.timed {
        if let Some(mut material) = graphs.get_mut(handle) {
            material.extension.slots = Slots::of(
                &compiled.values(graph, time.elapsed_secs()),
            );
        }
    }
}

/// How the camera looks at the model.
#[derive(Resource)]
struct Orbit {
    yaw: f32,
    pitch: f32,
    distance: f32,
    target: Vec3,
    /// Whether it has been fitted to the model read last.
    fitted: bool,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            yaw: YAW,
            pitch: PITCH,
            distance: 5.0,
            target: Vec3::ZERO,
            fitted: false,
        }
    }
}

impl Orbit {
    fn eye(&self) -> Vec3 {
        self.target
            + Quat::from_euler(
                EulerRot::YXZ,
                self.yaw,
                -self.pitch,
                0.0,
            ) * Vec3::Z
                * self.distance
    }
}

/// Marks the frame the view is drawn in.
#[derive(Component)]
struct Shown;

/// Draws the view while its frame is in the panel, and lets it be
/// turned, moved and brought nearer.
fn open_view(
    mut commands: Commands,
    stage: Option<Res<Stage>>,
    shown: Query<Entity, With<Shown>>,
    mut cameras: Query<&mut Camera>,
    mut watched: Local<Option<Entity>>,
) {
    let Some(stage) = stage else {
        return;
    };
    let frame = shown.iter().next();
    if let Some(frame) = frame
        && *watched != Some(frame)
    {
        commands.entity(frame).observe(drag).observe(zoom);
        *watched = Some(frame);
    }
    if let Ok(mut camera) = cameras.get_mut(stage.camera)
        && camera.is_active != frame.is_some()
    {
        camera.is_active = frame.is_some();
    }
}

/// Turns the camera around the model by the left or right button's
/// drag, and moves it by the middle button's.
fn drag(
    drag: On<PointerDrag>,
    frames: Query<(), With<Shown>>,
    mut orbit: ResMut<Orbit>,
) {
    if frames.get(drag.event_target()).is_err() {
        return;
    }
    match drag.button {
        PointerButton::Primary | PointerButton::Secondary => {
            orbit.yaw -= drag.delta.x * 0.008;
            orbit.pitch =
                (orbit.pitch + drag.delta.y * 0.008).clamp(-1.5, 1.5);
        }
        PointerButton::Middle => {
            let turn = Quat::from_euler(
                EulerRot::YXZ,
                orbit.yaw,
                -orbit.pitch,
                0.0,
            );
            let step = orbit.distance * 0.0015;
            let moved =
                turn * Vec3::new(-drag.delta.x, drag.delta.y, 0.0);
            orbit.target += moved * step;
        }
    }
}

fn zoom(scroll: On<PointerScroll>, mut orbit: ResMut<Orbit>) {
    orbit.distance =
        (orbit.distance * (1.0 - scroll.y * 0.1)).max(0.05);
}

/// Places the camera as the orbit says, fitted to the model when it
/// is first read.
fn pose(
    mut commands: Commands,
    stage: Option<Res<Stage>>,
    open: Res<Open>,
    mut orbit: ResMut<Orbit>,
    set: Res<CameraSettings>,
    mut cameras: Query<(&mut Transform, &mut bevy::pbr::DistanceFog)>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let Some(stage) = stage else {
        return;
    };
    if !orbit.fitted
        && let Some(loaded) = &open.model
    {
        let fov = set.fov.to_radians();
        *orbit = Orbit {
            target: loaded.middle,
            distance: loaded.radius / (fov * 0.5).sin() * 1.1,
            fitted: true,
            ..Orbit::default()
        };
    }
    if !orbit.is_changed() {
        return;
    }
    redraw.write(RequestRedraw);
    if let Ok((mut transform, mut fog)) =
        cameras.get_mut(stage.camera)
    {
        *transform = Transform::from_translation(orbit.eye())
            .looking_at(orbit.target, Vec3::Y);
        fog.falloff = sky::fog_for(&set, orbit.distance * 4.0 + 20.0);
    }
    commands
        .entity(stage.key)
        .try_insert(sky::shadows_for(&set, orbit.distance * 2.0));
}

/// Draws the box around the selected object and those inside it.
fn outline(
    open: Res<Open>,
    boxes: Query<(&Aabb, &GlobalTransform)>,
    mut gizmos: Gizmos<PrefabGizmos>,
) {
    let (Some(selected), Some(loaded)) =
        (&open.selected, &open.model)
    else {
        return;
    };
    let inside = format!("{selected}/");
    let (mut low, mut high) =
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for (object, (_, pieces)) in
        loaded.model.objects.iter().zip(&loaded.entities)
    {
        if object.path != *selected
            && !object.path.starts_with(&inside)
        {
            continue;
        }
        for piece in pieces {
            let Ok((aabb, at)) = boxes.get(*piece) else {
                continue;
            };
            let (centre, half) = (
                Vec3::from(aabb.center),
                Vec3::from(aabb.half_extents),
            );
            for corner in 0..8 {
                let sign = Vec3::new(
                    if corner & 1 == 0 { -1.0 } else { 1.0 },
                    if corner & 2 == 0 { -1.0 } else { 1.0 },
                    if corner & 4 == 0 { -1.0 } else { 1.0 },
                );
                let p = at.transform_point(centre + half * sign);
                low = low.min(p);
                high = high.max(p);
            }
        }
    }
    if low.x > high.x {
        return;
    }
    gizmos.cube(
        Transform::from_translation((low + high) * 0.5)
            .with_scale(high - low),
        Color::srgb(1.0, 0.8, 0.3),
    );
}

/// The panel: the bar, then the hierarchy, the view and the
/// inspector side by side.
fn screen() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(
            column((
                bar(),
                row((hierarchy(), view(), inspector()))
                    .width(percent(100.0))
                    .height(percent(100.0))
                    .min_height(px(0.0))
                    .gap(gap),
            ))
            .width(percent(100.0))
            .height(percent(100.0))
            .gap(gap)
            .padding(UiRect::all(px(gap))),
        )
    })
}

/// The prefab shown, the button that makes a new one, the mesh it is
/// made of, and the last thing done.
fn bar() -> AnyView {
    keyed(
        resource::<Library, _>(|library| library.revision),
        |_: &u64| {
            AnyView::new(|cx: &mut Cx| {
                let gap = cx.theme().space.gap;
                let library = cx.world.resource::<Library>();
                let open = cx.world.resource::<Open>();
                let names = |kind: Kind| -> Vec<String> {
                    let mut names: Vec<String> = library
                        .items
                        .iter()
                        .filter(|item| {
                            item.kind == kind && item.is_own()
                        })
                        .map(|item| item.name.clone())
                        .collect();
                    names.sort();
                    names.dedup();
                    names
                };
                let prefabs = names(Kind::Prefab);
                let meshes = names(Kind::Mesh);
                let shown = open
                    .name
                    .as_ref()
                    .and_then(|name| {
                        prefabs.iter().position(|p| p == name)
                    })
                    .map_or(0, |at| at + 1);
                let made_of = meshes
                    .iter()
                    .position(|mesh| *mesh == open.prefab.model)
                    .map_or(0, |at| at + 1);
                let words =
                    |none: &str, names: &[String]| -> Vec<String> {
                        std::iter::once(none.to_owned())
                            .chain(
                                names.iter().map(|name| {
                                    name.replace('_', " ")
                                }),
                            )
                            .collect()
                    };
                let pick_prefab = dropdown(
                    words("none open", &prefabs),
                    shown,
                    Glyph::Chevron.image(ICON),
                    move |world, at| {
                        let Some(name) = at
                            .checked_sub(1)
                            .and_then(|at| prefabs.get(at))
                        else {
                            return;
                        };
                        if let Err(error) = self::open(world, name) {
                            world.resource_mut::<Open>().note =
                                format!("Error: {error:#}");
                        }
                    },
                )
                .width(px(200.0));
                let pick_mesh = dropdown(
                    words("no mesh", &meshes),
                    made_of,
                    Glyph::Chevron.image(ICON),
                    move |world, at| {
                        let model = at
                            .checked_sub(1)
                            .and_then(|at| meshes.get(at))
                            .cloned()
                            .unwrap_or_default();
                        set_model(world, model);
                    },
                )
                .width(px(200.0));
                cx.build(
                    row((
                        labelled("prefab", pick_prefab.boxed()),
                        button(
                            row((
                                glyph(Glyph::Plus, ICON),
                                label("Prefab"),
                            ))
                            .gap(4.0),
                        )
                        .padding(UiRect::axes(px(8.0), px(2.0)))
                        .tooltip(|| {
                            note("New prefab, made of the mesh shown")
                        })
                        .rules(styles::button)
                        .tagged(Variant::Default)
                        .on_activate(new_prefab),
                        labelled("mesh", pick_mesh.boxed()),
                        button(
                            row((
                                glyph(Glyph::Plus, ICON),
                                label(".glb"),
                            ))
                            .gap(4.0),
                        )
                        .padding(UiRect::axes(px(8.0), px(2.0)))
                        .tooltip(|| {
                            note(
                                "Add a .glb file to the library as \
                                 a mesh to make prefabs of",
                            )
                        })
                        .rules(styles::button)
                        .tagged(Variant::Default)
                        .on_activate(crate::graphs::assets::import),
                        description(resource::<Open, _>(|open| {
                            open.note.clone()
                        })),
                    ))
                    .gap(gap)
                    .align(AlignItems::Center),
                )
            })
        },
    )
    .within(column(()).gap(0.0))
    .boxed()
}

/// Makes a new prefab of the mesh shown, or of none, and opens it.
pub(crate) fn new_prefab(world: &mut World) {
    let model = world.resource::<Open>().prefab.model.clone();
    let wanted = if model.is_empty() {
        "new_prefab".to_owned()
    } else {
        model.clone()
    };
    let made = world
        .resource_mut::<Library>()
        .create(Kind::Prefab, &wanted)
        .and_then(|item| {
            world
                .resource_mut::<Library>()
                .save_prefab(&item.name, &Prefab::of(&model))?;
            Ok(item.name)
        });
    match made.and_then(|name| open(world, &name).map(|()| name)) {
        Ok(name) => {
            world.resource_mut::<Open>().note =
                format!("Made {name}");
        }
        Err(error) => {
            world.resource_mut::<Open>().note =
                format!("Error: {error:#}");
        }
    }
}

/// Makes the open prefab of the mesh `model`, keeping its parts, and
/// draws it.
fn set_model(world: &mut World, model: String) {
    if world.resource::<Open>().name.is_none() {
        world.resource_mut::<Open>().prefab.model = model;
        world.resource_mut::<Open>().note =
            "Make or open a prefab to give it the mesh.".to_owned();
        return;
    }
    world.resource_mut::<Open>().prefab.model = model;
    save(world);
    reload(world);
}

/// The model's objects as a tree, by their names in the file, and the
/// parts whose objects the file no longer has.
fn hierarchy() -> AnyView {
    let list = keyed(
        resource::<Open, _>(|open| {
            (open.version, open.selected.clone())
        }),
        |_: &(u64, Option<String>)| {
            AnyView::new(|cx: &mut Cx| {
                let open = cx.world.resource::<Open>();
                let Some(loaded) = &open.model else {
                    return cx.build(column((description(
                        open.error.clone(),
                    ),)));
                };
                let objects = &loaded.model.objects;
                let mut rows: Vec<AnyView> = Vec::new();
                for (at, object) in objects.iter().enumerate() {
                    let mut above = object.parent;
                    let mut hidden = false;
                    while let Some(up) = above {
                        if !open.expanded.contains(&objects[up].path)
                        {
                            hidden = true;
                            break;
                        }
                        above = objects[up].parent;
                    }
                    if hidden {
                        continue;
                    }
                    let parent_of = objects
                        .iter()
                        .skip(at + 1)
                        .any(|o| o.parent == Some(at));
                    let selected = open.selected.as_deref()
                        == Some(&object.path);
                    let changed =
                        open.prefab.parts.contains_key(&object.path);
                    rows.push(object_row(
                        object.path.clone(),
                        object.name.clone(),
                        object.depth,
                        parent_of.then(|| {
                            open.expanded.contains(&object.path)
                        }),
                        selected,
                        changed,
                    ));
                }
                let paths = loaded.model.paths();
                let missing: Vec<String> = open
                    .prefab
                    .missing(&paths)
                    .map(str::to_owned)
                    .collect();
                if !missing.is_empty() {
                    rows.push(section("MISSING").boxed());
                    for path in missing {
                        rows.push(missing_row(path));
                    }
                }
                cx.build(column(rows).gap(2.0))
            })
        },
    )
    .within(column(()).gap(0.0));
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        cx.build(
            scroll((column((section("HIERARCHY"), list))
                .gap(gap)
                .padding(UiRect::all(px(gap))),))
            .width(px(SIDE))
            .shrink(0.0)
            .height(percent(100.0))
            .fill(theme.palette.card)
            .border(1.0)
            .border_color(theme.palette.border),
        )
    })
}

/// One object in the hierarchy: its toggle when it has children, its
/// name, and a mark when its part changes it.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "what the hierarchy shows of a row"
)]
fn object_row(
    path: String,
    name: String,
    depth: usize,
    expanded: Option<bool>,
    selected: bool,
    changed: bool,
) -> AnyView {
    let toggled = path.clone();
    let toggle: AnyView = match expanded {
        Some(open) => button(glyph(
            if open { Glyph::Chevron } else { Glyph::Into },
            ICON,
        ))
        .rules(bevy_fynix::views::icon_button)
        .on_activate(move |world| {
            let mut state = world.resource_mut::<Open>();
            if !state.expanded.remove(&toggled) {
                state.expanded.insert(toggled.clone());
            }
            state.changed();
        })
        .boxed(),
        None => column(()).width(px(ICON + 8.0)).boxed(),
    };
    let words = if changed { format!("{name} *") } else { name };
    row((
        column(()).width(px(depth as f32 * 12.0)),
        toggle,
        button(label(words))
            .padding(UiRect::axes(px(6.0), px(1.0)))
            .rules(styles::button)
            .tagged(if selected {
                Variant::Primary
            } else {
                Variant::Default
            })
            .on_activate(move |world| {
                let mut open = world.resource_mut::<Open>();
                open.selected = Some(path.clone());
                open.changed();
            }),
    ))
    .gap(2.0)
    .align(AlignItems::Center)
    .boxed()
}

/// A part whose object the file no longer has, and the button that
/// takes it away.
fn missing_row(path: String) -> AnyView {
    let shown = path.clone();
    row((
        column((note(format!("{shown} (missing)")),))
            .width(percent(100.0))
            .min_width(px(0.0)),
        button(glyph(Glyph::Cross, ICON))
            .rules(bevy_fynix::views::icon_button)
            .tooltip(|| note("Forget this part's settings"))
            .on_activate(move |world| {
                let mut open = world.resource_mut::<Open>();
                open.prefab.parts.remove(&path);
                open.changed();
                save(world);
            }),
    ))
    .gap(4.0)
    .align(AlignItems::Center)
    .boxed()
}

/// The picture of the model, as wide as the space between the
/// hierarchy and the inspector.
fn view() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let theme = cx.theme();
        let picture = column(())
            .width(percent(100.0))
            .height(percent(100.0))
            .min_width(px(0.0))
            .overflow(Overflow::clip())
            .fill(theme.palette.input)
            .radius(theme.space.round)
            .tagged(Shown);
        let camera = cx
            .world
            .get_resource::<Stage>()
            .map(|stage| stage.camera);
        let picture = match camera {
            Some(camera) => {
                cx.build(picture.seeded(ViewportNode::new(camera)))
            }
            None => cx.build(picture),
        };
        if let Some(mut node) = cx.world.get_mut::<Node>(picture) {
            node.flex_grow = 1.0;
        }
        picture
    })
}

/// The selected object's finish, offset, turn and scale.
fn inspector() -> AnyView {
    let body = keyed(
        resource::<Open, _>(|open| {
            (open.version, open.selected.clone())
        }),
        |_: &(u64, Option<String>)| {
            AnyView::new(|cx: &mut Cx| {
                let gap = cx.theme().space.gap;
                let open = cx.world.resource::<Open>();
                let Some(path) = open.selected.clone() else {
                    return cx.build(column((description(
                        "Select an object in the hierarchy to \
                         change it here.",
                    ),)));
                };
                let error = open.errors.get(&path).cloned();
                let library = cx.world.resource::<Library>();
                let mut finishes = vec![Finish::Own];
                let mut named =
                    |kind: Kind, make: fn(String) -> Finish| {
                        let mut names: Vec<String> = library
                            .items
                            .iter()
                            .filter(|item| item.kind == kind)
                            .map(|item| item.name.clone())
                            .collect();
                        names.sort();
                        names.dedup();
                        finishes.extend(names.into_iter().map(make));
                    };
                named(Kind::Material, Finish::Material);
                named(Kind::Shader, Finish::Shader);
                let part = open.prefab.part(&path);
                let at = finishes
                    .iter()
                    .position(|f| *f == part.finish)
                    .unwrap_or(0);
                let words: Vec<String> = finishes
                    .iter()
                    .map(|finish| match finish {
                        Finish::Own => "the file's own".to_owned(),
                        Finish::Material(name) => format!(
                            "material: {}",
                            name.replace('_', " ")
                        ),
                        Finish::Shader(name) => format!(
                            "shader: {}",
                            name.replace('_', " ")
                        ),
                    })
                    .collect();
                let finish = dropdown(
                    words,
                    at,
                    Glyph::Chevron.image(ICON),
                    move |world, at| {
                        let Some(finish) = finishes.get(at).cloned()
                        else {
                            return;
                        };
                        edit_part(world, |part| part.finish = finish);
                    },
                )
                .width(percent(100.0));
                let mut rows: Vec<AnyView> = vec![
                    section(path.replace('/', " / ")).boxed(),
                    labelled("finish", finish.boxed()),
                ];
                if let Some(error) = error {
                    rows.push(
                        description(format!("Error: {error}"))
                            .boxed(),
                    );
                }
                rows.push(labelled(
                    "offset",
                    axes(
                        |part| part.offset.to_array(),
                        |part, axis, value| {
                            part.offset[axis] = value;
                        },
                        0.01,
                    ),
                ));
                rows.push(labelled(
                    "turn",
                    axes(
                        |part| {
                            let (x, y, z) =
                                part.turn.to_euler(EulerRot::XYZ);
                            [
                                x.to_degrees(),
                                y.to_degrees(),
                                z.to_degrees(),
                            ]
                        },
                        |part, axis, value| {
                            let (x, y, z) =
                                part.turn.to_euler(EulerRot::XYZ);
                            let mut angles = [x, y, z];
                            angles[axis] = value.to_radians();
                            part.turn = Quat::from_euler(
                                EulerRot::XYZ,
                                angles[0],
                                angles[1],
                                angles[2],
                            );
                        },
                        1.0,
                    ),
                ));
                rows.push(labelled(
                    "scale",
                    axes(
                        |part| part.scale.to_array(),
                        |part, axis, value| {
                            part.scale[axis] = value;
                        },
                        0.01,
                    ),
                ));
                rows.push(
                    button(label("Reset"))
                        .padding(UiRect::axes(px(8.0), px(2.0)))
                        .tooltip(|| {
                            note(
                                "Give the object back what the file \
                                 gives it",
                            )
                        })
                        .rules(styles::button)
                        .tagged(Variant::Default)
                        .on_activate(|world| {
                            edit_part(world, |part| {
                                *part = Part::default();
                            });
                        })
                        .boxed(),
                );
                cx.build(column(rows).gap(gap / 2.0))
            })
        },
    )
    .within(column(()).gap(0.0));
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        cx.build(
            scroll((column((section("INSPECTOR"), body))
                .gap(gap)
                .padding(UiRect::all(px(gap))),))
            .width(px(SIDE))
            .shrink(0.0)
            .height(percent(100.0))
            .fill(theme.palette.card)
            .border(1.0)
            .border_color(theme.palette.border),
        )
    })
}

/// Three number fields for an axis each of what `get` reads of the
/// selected part, which `put` changes.
fn axes(
    get: fn(&Part) -> [f32; 3],
    put: fn(&mut Part, usize, f32),
    step: f64,
) -> AnyView {
    let fields = (0..3)
        .map(|axis| {
            number_field(
                resource::<Open, _>(move |open| {
                    open.part().map_or(0.0, |part| {
                        f64::from(get(&part)[axis])
                    })
                }),
                move |world: &mut World, value: f64| {
                    edit_part(world, |part| {
                        put(part, axis, value as f32);
                    });
                },
            )
            .step(step)
            .precision(if step >= 1.0 { 1 } else { 3 })
            .width(percent(100.0))
            .min_width(px(0.0))
            .rules(text_input)
            .boxed()
        })
        .collect::<Vec<_>>();
    row(fields).gap(4.0).align(AlignItems::Center).boxed()
}
