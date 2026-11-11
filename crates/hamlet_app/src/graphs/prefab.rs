//! The PREFAB panel: meshes of the library put together, dragged in
//! from ASSETS, their objects in a hierarchy, and an inspector that
//! gives the selected one a shader graph or a material and moves,
//! turns and scales it. A material given to an object is its
//! children's too, unless they have their own.
//! The view draws the model in the realm's light through its own
//! camera: dragging turns it, dragging with the middle button moves
//! it, and scrolling brings it nearer. Several prefabs can be open,
//! one shown at a time and picked from the bar's list, each keeping
//! its changes until it is saved. The model follows its
//! `.glb`: saving the file again in Blender redraws the prefab, and
//! the parts keep their settings.

mod gizmo;
mod model;
mod surface;

use core::f32::consts::{FRAC_PI_2, PI};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Context;
use bevy::app::{App, PostUpdate, Startup, Update};
use bevy::asset::uuid::Uuid;
use bevy::asset::{Assets, Handle, RenderAssetUsages};
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
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::gizmos::AppGizmoBuilder;
use bevy::gizmos::config::{GizmoConfigGroup, GizmoConfigStore};
use bevy::gizmos::gizmos::Gizmos;
use bevy::image::Image;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::InputFocus;
use bevy::light::DirectionalLight;
use bevy::material::AlphaMode;
use bevy::math::{EulerRot, Quat, Rect, Rot2, Vec2, Vec3};
use bevy::mesh::{Mesh, Mesh3d, VertexAttributeValues};
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::picking::events::{
    PointerClick, PointerDrag, PointerDragDrop, PointerMove,
    PointerOut, PointerPress, PointerScroll,
};
use bevy::picking::hover::Hovered;
use bevy::picking::mesh_picking::ray_cast::{
    MeshRayCast, MeshRayCastSettings, RayCastVisibility,
};
use bevy::picking::pointer::PointerButton;
use bevy::reflect::Reflect;
use bevy::render::render_resource::{
    TextureDimension, TextureFormat, TextureUsages,
};
use bevy::text::{Justify, TextLayout};
use bevy::time::Time;
use bevy::transform::components::{GlobalTransform, Transform};
use bevy::ui::widget::ViewportNode;
use bevy::ui::{
    AlignItems, ComputedNode, JustifyContent, Node, Overflow,
    UiGlobalTransform, UiRect, UiTransform, percent, px,
};
use bevy::window::{
    PrimaryWindow, RequestRedraw, SystemCursorIcon, Window,
};
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, Invoke, Layer, Mods, Scope,
    ScopeId, ScopeSpec, ShortcutAppExt,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    self, BehaviorExt, FrameProps, TooltipExt, button, column,
    dropdown, frame, label, menu_item, number_field, overlay, popup,
    row, scroll, text_field,
};
use bevy_fynix::{
    Focused, ModifierExt, OverrideCursor, ScopedExt, ViewExt, keyed,
    resource, style,
};
use hamlet_graph::Graph;
use hamlet_graph::prefab::{Finish, Part, Prefab};
use hamlet_graph_editor::Editor;
use hamlet_graph_editor::canvas::corner;
use hamlet_graph_editor::library::carry::tile_of;
use hamlet_graph_editor::library::{Kind, Library, Source};
use hamlet_realm::sky::{self, CameraSettings, Hourly};
use hamlet_shader::Compiled;
use hamlet_ui::styles::{
    self, ICON, description, labelled, note, section, text_input,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx};

use self::model::Model;
use self::surface::{
    GraphExt, GraphMaterial, Live, Shaded, Shaders, graph_of, shaded,
    uses,
};
use crate::dock::{PanelAppExt, flush_panel};
use crate::graphs::shader::swatch::Slots;
use crate::graphs::shader::{self, Got, ShaderGraph};
use crate::sides::{self, Side};

/// The render layer only the prefab view's camera, lights and model
/// are on: apart from the blueprint preview's 8, the shader preview's
/// 9, the thumbnails' 10 and the material preview's 11.
const LAYER: usize = 12;

/// The width the hierarchy and the inspector start at, in pixels.
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
    .init_resource::<Listing>()
    .init_resource::<Orbit>()
    .init_resource::<Pressed>()
    .init_resource::<OnObject>()
    .init_resource::<gizmo::Hot>()
    .init_resource::<gizmo::Dragging>()
    .init_gizmo_group::<gizmo::PrefabHandleGizmos>()
    .add_systems(Update, align_right)
    .add_observer(dropped_on_view)
    .init_gizmo_group::<PrefabGizmos>()
    .add_plugins(surface::plugin)
    .add_systems(Startup, (spawn, style_outline))
    .add_systems(
        Update,
        (
            settle,
            keys,
            follow,
            live.after(shader::run).before(build),
            build,
            feed,
            open_view,
            pose,
            outline,
            point,
            relist,
        ),
    )
    .add_systems(Startup, (gizmo::style, gizmo::spawn))
    .add_systems(Update, gizmo::cancel)
    .add_systems(PostUpdate, gizmo::handles);
    sides::plugin::<Screen>(app, SIDE, SIDE);
    views(app);
}

/// The prefab shown in the panel, its model as last read, and what is
/// selected in it; and the other prefabs open, to show instead.
#[derive(Resource, Default)]
pub(crate) struct Open {
    /// Which of the prefabs open is shown.
    id: u64,
    /// Every prefab open, in the order opened. The one shown is kept
    /// in the fields here instead, and its own is stale.
    docs: Vec<Doc>,
    /// The last id given.
    made: u64,
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
    /// Each material drawn with a shader graph, to fill its slots
    /// again when the graph's values or the time change.
    graphed: Vec<Graphed>,
    /// The changes to undo and redo.
    history: History,
    /// The prefab as its file holds it, or none for a new one: it is
    /// written only when saved.
    stored: Prefab,
    /// Whether the person is asked to save the changes before the
    /// prefab shown closes.
    asking: bool,
    /// What a name being typed is for.
    naming: Option<Naming>,
    /// Whether the prefab shown closes once it is named and saved.
    closing: bool,
}

/// A prefab open but not shown, as it was left.
struct Doc {
    id: u64,
    name: Option<String>,
    prefab: Prefab,
    stored: Prefab,
    history: History,
    selected: Option<String>,
    expanded: BTreeSet<String>,
}

/// What a name being typed is for.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Naming {
    /// Saving the prefab shown, new, for the first time.
    First,
    /// Saving the prefab shown as a new one, and going on with that.
    Copy,
    /// Making a new prefab, of no mesh, and showing it.
    New,
}

/// Where the list of the prefabs open hangs from, while it is open.
#[derive(Resource, Default)]
struct Listing(Option<Rect>);

/// A material drawn with a shader graph.
struct Graphed {
    material: Handle<GraphMaterial>,
    /// What gives the graph.
    finish: Finish,
    compiled: Compiled,
    /// The graph its slots are filled from.
    graph: Graph,
}

/// The prefab as each change left it, to step back and forth through.
#[derive(Default)]
struct History {
    /// The prefab as last written.
    saved: Prefab,
    /// Earlier prefabs, the latest last.
    undo: Vec<Prefab>,
    /// Prefabs undone, the latest undone last.
    redo: Vec<Prefab>,
    /// When the last step was kept, so the many changes of a field
    /// being dragged or typed in are undone as one.
    at: Option<f32>,
    /// Whether a handle in the view is dragged: what it changes is
    /// one step, however slow.
    holding: bool,
}

/// Changes closer together than this, in seconds, are one step.
const STEP: f32 = 0.6;

/// A model read from its file, with what is drawn of it.
struct Loaded {
    /// The meshes put in, as one model.
    model: Model,
    /// Each mesh's file, and when it was last saved.
    files: Vec<(PathBuf, Option<SystemTime>)>,
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

    /// Whether it has changes its file does not, or is new and has
    /// something in it.
    fn is_dirty(&self) -> bool {
        self.prefab != self.stored
    }

    /// The part of the selected object, if one is.
    fn part(&self) -> Option<Part> {
        self.selected.as_ref().map(|path| self.prefab.part(path))
    }

    /// The prefabs open, in the order opened: each one's id, what it
    /// is called, and whether it has changes not saved.
    /// A new prefab with nothing in it yet is left out.
    fn listed(&self) -> Vec<(u64, String, bool)> {
        self.docs
            .iter()
            .filter_map(|doc| {
                let (name, prefab, dirty) = if doc.id == self.id {
                    (&self.name, &self.prefab, self.is_dirty())
                } else {
                    (&doc.name, &doc.prefab, doc.prefab != doc.stored)
                };
                let label = shown_as(name.as_deref(), prefab);
                (!label.is_empty()).then_some((doc.id, label, dirty))
            })
            .collect()
    }

    /// What the bar's list shows of the prefab shown.
    fn label(&self) -> String {
        shown_as(self.name.as_deref(), &self.prefab)
    }

    /// The id of the open prefab of the library named `name`.
    fn find(&self, name: &str) -> Option<u64> {
        if self.name.as_deref() == Some(name) {
            return Some(self.id);
        }
        self.docs
            .iter()
            .find(|doc| {
                doc.id != self.id && doc.name.as_deref() == Some(name)
            })
            .map(|doc| doc.id)
    }

    /// Keeps the prefab shown, as it is, among the others, to show it
    /// again.
    fn park(&mut self) {
        let id = self.id;
        let Some(doc) = self.docs.iter_mut().find(|doc| doc.id == id)
        else {
            return;
        };
        doc.name = self.name.clone();
        doc.prefab = self.prefab.clone();
        doc.stored = self.stored.clone();
        doc.history = std::mem::take(&mut self.history);
        doc.selected = self.selected.take();
        doc.expanded = std::mem::take(&mut self.expanded);
    }
}

/// What a prefab of the library named `name`, or a new one of none,
/// is called in the panel.
/// What the list shows of `prefab`, of the library's prefab `name` or
/// of none: nothing for a new one with nothing in it yet.
fn shown_as(name: Option<&str>, prefab: &Prefab) -> String {
    if name.is_none() && *prefab == Prefab::default() {
        String::new()
    } else {
        called(name)
    }
}

fn called(name: Option<&str>) -> String {
    name.map_or_else(
        || "New prefab".to_owned(),
        |name| name.replace('_', " "),
    )
}

/// Shows the library's prefab `name` in the panel, opening it first
/// unless it is open: the prefab shown keeps its changes.
pub(crate) fn open(
    world: &mut World,
    name: &str,
) -> anyhow::Result<()> {
    if let Some(id) = world.resource::<Open>().find(name) {
        show(world, id);
        return Ok(());
    }
    let prefab = world
        .resource::<Library>()
        .find(Kind::Prefab, name)
        .with_context(|| format!("there is no prefab named {name}"))?
        .prefab()?;
    add(world, Some(name.to_owned()), prefab);
    Ok(())
}

/// Opens `prefab`, of the library's prefab `name` or of none, and
/// shows it.
fn add(world: &mut World, name: Option<String>, prefab: Prefab) {
    let mut open = world.resource_mut::<Open>();
    open.made += 1;
    let id = open.made;
    open.docs.push(Doc {
        id,
        name,
        stored: prefab.clone(),
        history: History {
            saved: prefab.clone(),
            ..History::default()
        },
        prefab,
        selected: None,
        expanded: BTreeSet::new(),
    });
    show(world, id);
}

/// Shows the open prefab `id`, keeping the one shown as it is. A new
/// prefab left with nothing in it closes.
fn show(world: &mut World, id: u64) {
    let mut open = world.resource_mut::<Open>();
    let open = &mut *open;
    if open.id == id && open.model.is_some() {
        return;
    }
    open.park();
    let left = open.id;
    open.docs.retain(|doc| {
        doc.id == id
            || doc.id != left
            || doc.name.is_some()
            || doc.prefab != Prefab::default()
    });
    let Some(doc) = open.docs.iter_mut().find(|doc| doc.id == id)
    else {
        return;
    };
    open.id = id;
    open.name = doc.name.clone();
    open.prefab = doc.prefab.clone();
    open.stored = doc.stored.clone();
    open.history = std::mem::take(&mut doc.history);
    open.selected = doc.selected.take();
    open.expanded = std::mem::take(&mut doc.expanded);
    open.asking = false;
    open.naming = None;
    open.closing = false;
    open.note = String::new();
    open.model = None;
    reload(world);
}

/// Closes the prefab shown, once the person says whether to save its
/// changes when it has any.
fn close(world: &mut World) {
    let mut open = world.resource_mut::<Open>();
    if open.is_dirty() {
        open.naming = None;
        open.asking = true;
    } else {
        close_now(world);
    }
}

/// Closes the prefab shown, dropping its changes, and shows the one
/// opened last, or a new one when none is left.
fn close_now(world: &mut World) {
    let mut open = world.resource_mut::<Open>();
    let id = open.id;
    open.docs.retain(|doc| doc.id != id);
    open.asking = false;
    open.closing = false;
    open.naming = None;
    // None of the open is shown, so nothing is kept of it.
    open.id = 0;
    match open.docs.last().map(|doc| doc.id) {
        Some(next) => show(world, next),
        None => add(world, None, Prefab::default()),
    }
}

/// Writes the prefab shown to its file, and whether it was: a new one
/// first has its name typed, and is written once it is.
fn store(world: &mut World) -> bool {
    let (name, prefab) = {
        let open = world.resource::<Open>();
        (open.name.clone(), open.prefab.clone())
    };
    let Some(name) = name else {
        world.resource_mut::<Open>().naming = Some(Naming::First);
        return false;
    };
    let saved =
        world.resource_mut::<Library>().save_prefab(&name, &prefab);
    let mut open = world.resource_mut::<Open>();
    match saved {
        Ok(_) => {
            open.stored = prefab;
            open.note = format!("Saved {}", called(Some(&name)));
            true
        }
        Err(error) => {
            open.note = format!("Error: {error:#}");
            false
        }
    }
}

/// Does what the name typed, the nearest free one to `wanted`, is
/// for: saves the prefab shown under it, or makes a new prefab of it.
fn named(world: &mut World, wanted: &str) {
    let Some(naming) = world.resource::<Open>().naming else {
        return;
    };
    let prefab = match naming {
        Naming::New => Prefab::default(),
        Naming::First | Naming::Copy => {
            world.resource::<Open>().prefab.clone()
        }
    };
    let made = world
        .resource_mut::<Library>()
        .create(Kind::Prefab, wanted)
        .and_then(|item| {
            world
                .resource_mut::<Library>()
                .save_prefab(&item.name, &prefab)?;
            Ok(item.name)
        });
    let name = match made {
        Ok(name) => name,
        Err(error) => {
            world.resource_mut::<Open>().note =
                format!("Error: {error:#}");
            return;
        }
    };
    let mut open = world.resource_mut::<Open>();
    open.naming = None;
    if naming == Naming::New {
        add(world, Some(name), prefab);
        return;
    }
    open.note = format!("Saved {}", called(Some(&name)));
    open.name = Some(name);
    open.stored = prefab;
    if open.closing {
        close_now(world);
    }
}

/// Follows the prefabs open to their new name when the library's
/// `from` of `kind` is renamed `to`.
pub(crate) fn renamed(
    world: &mut World,
    kind: Kind,
    from: &str,
    to: &str,
) {
    if kind != Kind::Prefab {
        return;
    }
    let mut open = world.resource_mut::<Open>();
    if open.name.as_deref() == Some(from) {
        open.name = Some(to.to_owned());
    }
    for doc in &mut open.docs {
        if doc.name.as_deref() == Some(from) {
            doc.name = Some(to.to_owned());
        }
    }
}

/// Reads the open prefab's meshes again from their files, as one
/// model.
fn reload(world: &mut World) {
    let meshes = world.resource::<Open>().prefab.meshes.clone();
    let mut models = Vec::new();
    let mut files = Vec::new();
    let mut errors = Vec::new();
    for placed in &meshes {
        let file = world
            .resource::<Library>()
            .find(Kind::Mesh, &placed.mesh)
            .and_then(|item| match item.source {
                Source::File(path) => Some(path),
                _ => None,
            });
        let Some(file) = file else {
            errors.push(format!(
                "there is no mesh named {} in the library",
                placed.mesh
            ));
            continue;
        };
        match fs::read(&file)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| model::read(&bytes))
        {
            Ok(model) => {
                files.push((file.clone(), stamp(&file)));
                models.push((
                    placed.name.clone(),
                    placed.name.replace('_', " "),
                    model,
                ));
            }
            Err(error) => {
                errors.push(format!("{}: {error:#}", placed.mesh));
            }
        }
    }
    let model = Model::join(models);
    let (middle, radius) = bounds(&model);
    let roots: Vec<String> = model
        .objects
        .iter()
        .filter(|object| object.parent.is_none())
        .map(|object| object.path.clone())
        .collect();
    let mut open = world.resource_mut::<Open>();
    open.model = Some(Loaded {
        files,
        meshes: Vec::new(),
        images: Vec::new(),
        entities: Vec::new(),
        middle,
        radius,
        model,
    });
    open.error = if errors.is_empty() {
        String::new()
    } else {
        format!("Error: {}", errors.join("; "))
    };
    // A mesh just put in shows its objects.
    open.expanded.extend(roots);
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

/// Keeps the change just made to the prefab shown as a step to undo.
/// Nothing is written until it is saved.
fn record(world: &mut World) {
    let now = world.resource::<Time>().elapsed_secs();
    let mut open = world.resource_mut::<Open>();
    if open.prefab != open.history.saved {
        let history = &mut open.history;
        let joined = history
            .at
            .is_some_and(|at| history.holding || now - at < STEP);
        let before = std::mem::take(&mut history.saved);
        if !joined {
            history.undo.push(before);
        }
        history.redo.clear();
        history.at = Some(now);
        open.history.saved = open.prefab.clone();
    }
}

/// Steps back through the history, or forward with `redo`, and
/// draws the prefab as it was.
fn step(world: &mut World, redo: bool) {
    let mut open = world.resource_mut::<Open>();
    let open = &mut *open;
    let history = &mut open.history;
    let (from, to) = if redo {
        (&mut history.redo, &mut history.undo)
    } else {
        (&mut history.undo, &mut history.redo)
    };
    let Some(prefab) = from.pop() else {
        open.note = if redo {
            "Nothing to redo"
        } else {
            "Nothing to undo"
        }
        .to_owned();
        return;
    };
    to.push(std::mem::replace(&mut open.prefab, prefab));
    history.saved = open.prefab.clone();
    history.at = None;
    let remodelled = to
        .last()
        .is_some_and(|was| was.meshes != open.prefab.meshes);
    open.note = if redo { "Redone" } else { "Undone" }.to_owned();
    open.stale = true;
    open.changed();
    if remodelled {
        reload(world);
    }
}

/// Marks the panel, to know when the pointer is on it.
#[derive(Component)]
struct Screen;

/// Ctrl+Z undoes, Ctrl+Shift+Z or Ctrl+Y redoes and Ctrl+S saves,
/// while the pointer is on the panel and nothing is being typed in.
fn keys(world: &mut World) {
    if world.resource::<InputFocus>().get().is_some()
        || !world
            .query_filtered::<&Hovered, With<Screen>>()
            .iter(world)
            .any(Hovered::get)
    {
        return;
    }
    let keys = world.resource::<ButtonInput<KeyCode>>();
    let command = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    let shift =
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let undo = command && keys.just_pressed(KeyCode::KeyZ);
    let redo = command && keys.just_pressed(KeyCode::KeyY);
    let save = command && keys.just_pressed(KeyCode::KeyS);
    if undo || redo {
        step(world, redo || shift);
    } else if save {
        store(world);
    }
}

/// The scope of the panel's keys, in force while the pointer is on
/// it.
const SCOPE: ScopeId = ScopeId("prefab");

/// The views from the sides, on the blueprint preview's keys: F the
/// front, Shift+R the right, B the back and L the left, as Blender's;
/// and T from the top and Shift+T from the bottom.
fn views(app: &mut App) {
    app.add_scope(ScopeSpec {
        id: SCOPE,
        label: "Prefab",
        layer: Layer::Panel,
        active: None,
    });
    let views: [(&str, &str, Chord, fn(&mut World, Invoke)); 6] = [
        (
            "prefab.front",
            "Front View",
            Chord::key(KeyCode::KeyF),
            |w, _| {
                look(w, 0.0, 0.0);
            },
        ),
        (
            "prefab.right",
            "Right View",
            Chord {
                key: KeyCode::KeyR,
                mods: Mods::SHIFT,
            },
            |w, _| look(w, FRAC_PI_2, 0.0),
        ),
        (
            "prefab.back",
            "Back View",
            Chord::key(KeyCode::KeyB),
            |w, _| {
                look(w, PI, 0.0);
            },
        ),
        (
            "prefab.left",
            "Left View",
            Chord::key(KeyCode::KeyL),
            |w, _| {
                look(w, -FRAC_PI_2, 0.0);
            },
        ),
        (
            "prefab.top",
            "Top View",
            Chord::key(KeyCode::KeyT),
            |w, _| {
                look(w, 0.0, STRAIGHT);
            },
        ),
        (
            "prefab.bottom",
            "Bottom View",
            Chord {
                key: KeyCode::KeyT,
                mods: Mods::SHIFT,
            },
            |w, _| look(w, 0.0, -STRAIGHT),
        ),
    ];
    for (id, label, chord, run) in views {
        app.add_command(
            CommandSpec {
                id: CommandId(id),
                label,
                scope: SCOPE,
                run,
                enabled: |_| true,
                repeat: false,
            },
            &[chord],
        );
    }
}

/// Just short of straight up or down, where the camera's up would be
/// the line it looks along.
const STRAIGHT: f32 = FRAC_PI_2 - 0.001;

/// Turns the camera to look at the model from `yaw` round from its
/// front (+z) and `pitch` above it, keeping its distance and what it
/// looks at.
fn look(world: &mut World, yaw: f32, pitch: f32) {
    let mut orbit = world.resource_mut::<Orbit>();
    orbit.yaw = yaw;
    orbit.pitch = pitch;
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
    record(world);
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
    if open.model.is_none() || time.elapsed_secs() - *last < 1.0 {
        return;
    }
    *last = time.elapsed_secs();
    let moved = open.model.as_ref().is_some_and(|loaded| {
        loaded.files.iter().any(|(file, at)| stamp(file) != *at)
    });
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
        // The files as saved; `live` shows the shader editor's
        // changes before then.
        let now = shaded(&library, finish, None)
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
    config.line.width = 2.5;
    config.depth_bias = -0.02;
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
    editor: Res<Editor<ShaderGraph>>,
) {
    if !open.stale {
        return;
    }
    let live = Some(Live {
        name: &editor.name,
        graph: &editor.sheet.graph,
    });
    open.stale = false;
    redraw.write(RequestRedraw);
    for entity in &drawn {
        commands.entity(entity).despawn();
    }
    let open = &mut *open;
    open.errors.clear();
    open.graphed.clear();
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
    // What each object is drawn with: its own finish, else the
    // nearest one above it.
    let mut finishes: Vec<Finish> = Vec::new();
    for (at, object) in loaded.model.objects.iter().enumerate() {
        let part = open.prefab.part(&object.path);
        let finish = if part.finish.is_own() {
            object
                .parent
                .and_then(|above| finishes.get(above).cloned())
                .unwrap_or_default()
        } else {
            part.finish.clone()
        };
        finishes.push(finish.clone());
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
            &finish,
            &mut compiled,
            &mut shaders,
            live,
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
                    open.graphed.push(Graphed {
                        material: material.clone(),
                        finish: finish.clone(),
                        compiled: shaded.compiled.clone(),
                        graph: shaded.graph.clone(),
                    });
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
    live: Option<Live>,
) -> Option<Result<(Shaded, Uuid), String>> {
    if finish.is_own() {
        return None;
    }
    let got = cache.entry(finish.clone()).or_insert_with(|| {
        match shaded(library, finish, live) {
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
    for graphed in &open.graphed {
        if graphed.compiled.reads_time()
            && let Some(mut material) =
                graphs.get_mut(&graphed.material)
        {
            material.extension.slots = Slots::of(
                &graphed
                    .compiled
                    .values(&graphed.graph, time.elapsed_secs()),
            );
        }
    }
}

/// Shows the shader editor's changes on the parts that use its graph
/// as they are made, before they are saved, as Blender shows a
/// modifier's: a value changed fills their materials' slots, and new
/// code draws the model again.
fn live(
    editor: Res<Editor<ShaderGraph>>,
    got: Res<Got>,
    library: Res<Library>,
    time: Res<Time>,
    mut open: ResMut<Open>,
    mut graphs: ResMut<Assets<GraphMaterial>>,
    mut seen: Local<Option<(u64, u64)>>,
) {
    let now = (editor.changes, got.version);
    let Some((changes, version)) = seen.replace(now) else {
        return;
    };
    if (changes, version) == now {
        return;
    }
    let name = editor.name.as_str();
    let open = &mut *open;
    if !open
        .graphed
        .iter()
        .any(|graphed| uses(&library, &graphed.finish, name))
    {
        return;
    }
    if version != got.version {
        open.stale = true;
        return;
    }
    let live = Some(Live {
        name,
        graph: &editor.sheet.graph,
    });
    for graphed in &mut open.graphed {
        if !uses(&library, &graphed.finish, name) {
            continue;
        }
        let Some(Ok(graph)) =
            graph_of(&library, &graphed.finish, live)
        else {
            continue;
        };
        graphed.graph = graph;
        if let Some(mut material) = graphs.get_mut(&graphed.material)
        {
            material.extension.slots = Slots::of(
                &graphed
                    .compiled
                    .values(&graphed.graph, time.elapsed_secs()),
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
        commands
            .entity(frame)
            .observe(drag)
            .observe(zoom)
            .observe(press)
            .observe(pick)
            .observe(hover)
            .observe(leave)
            .observe(gizmo::grab)
            .observe(gizmo::steer)
            .observe(gizmo::release);
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
    gizmo: Res<gizmo::Dragging>,
    mut orbit: ResMut<Orbit>,
) {
    if frames.get(drag.event_target()).is_err()
        || gizmo.is_under_way()
    {
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

/// The furthest the pointer may move between a press and a release
/// that is still a click, in logical pixels.
const SLOP: f32 = 3.0;

/// Where the primary button last went down on the view.
#[derive(Resource, Default)]
struct Pressed(Option<Vec2>);

/// Notes where the primary button goes down, so a drag that turns
/// the view is not taken for a click.
fn press(press: On<PointerPress>, mut pressed: ResMut<Pressed>) {
    pressed.0 = (press.button == PointerButton::Primary)
        .then_some(press.pointer.position);
}

/// Selects the object under a click in the view, as its row in the
/// hierarchy does, or none when the click is on nothing.
fn pick(
    click: On<PointerClick>,
    pressed: Res<Pressed>,
    stage: Option<Res<Stage>>,
    frames: Query<(&ComputedNode, &UiGlobalTransform), With<Shown>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut ray_cast: MeshRayCast,
    mut open: ResMut<Open>,
) {
    let there = click.pointer.position;
    if click.button != PointerButton::Primary
        || pressed.0.is_none_or(|start| start.distance(there) > SLOP)
    {
        return;
    }
    let (Some(stage), Ok((node, at))) =
        (stage, frames.get(click.event_target()))
    else {
        return;
    };
    let Ok((camera, eye)) = cameras.get(stage.camera) else {
        return;
    };
    let Some(loaded) = &open.model else {
        return;
    };
    let hit = under(
        loaded,
        camera,
        eye,
        there - corner(node, at),
        &mut ray_cast,
    )
    .map(|object| loaded.model.objects[object].path.clone());
    if open.selected != hit {
        // Open what holds the object, so its row shows.
        if let Some(path) = &hit {
            let objects = &loaded.model.objects;
            let mut above = objects
                .iter()
                .position(|object| object.path == *path)
                .and_then(|at| objects[at].parent);
            let mut opened = Vec::new();
            while let Some(up) = above {
                opened.push(objects[up].path.clone());
                above = objects[up].parent;
            }
            open.expanded.extend(opened);
        }
        open.selected = hit;
        open.changed();
    }
}

/// The object of `loaded` whose meshes are nearest the camera at
/// `at` in the view, by its place in the model's objects.
fn under(
    loaded: &Loaded,
    camera: &Camera,
    eye: &GlobalTransform,
    at: Vec2,
    ray_cast: &mut MeshRayCast,
) -> Option<usize> {
    let ray = camera.viewport_to_world(eye, at).ok()?;
    let owner: HashMap<Entity, usize> = loaded
        .entities
        .iter()
        .enumerate()
        .flat_map(|(object, (_, pieces))| {
            pieces.iter().map(move |piece| (*piece, object))
        })
        .collect();
    let filter = |entity: Entity| owner.contains_key(&entity);
    let settings = MeshRayCastSettings::default()
        .with_visibility(RayCastVisibility::Any)
        .with_filter(&filter);
    ray_cast
        .cast_ray(ray, &settings)
        .first()
        .and_then(|(entity, _)| owner.get(entity).copied())
}

/// Whether the pointer is on an object of the model, which a click
/// would select.
#[derive(Resource, Default)]
struct OnObject(bool);

/// Notes whether the pointer, moved over the view, is on an object.
#[expect(clippy::too_many_arguments, reason = "an observer")]
fn hover(
    moved: On<PointerMove>,
    stage: Option<Res<Stage>>,
    frames: Query<(&ComputedNode, &UiGlobalTransform), With<Shown>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    open: Res<Open>,
    gizmo: Res<gizmo::Dragging>,
    mut ray_cast: MeshRayCast,
    mut on: ResMut<OnObject>,
) {
    if gizmo.is_under_way() {
        return;
    }
    let found = (|| {
        let stage = stage?;
        let (node, at) = frames.get(moved.event_target()).ok()?;
        let (camera, eye) = cameras.get(stage.camera).ok()?;
        let loaded = open.model.as_ref()?;
        let there = moved.pointer.position - corner(node, at);
        under(loaded, camera, eye, there, &mut ray_cast)
    })()
    .is_some();
    if on.0 != found {
        on.0 = found;
    }
}

/// The pointer has left the view, so it is on no object.
fn leave(
    out: On<PointerOut>,
    frames: Query<(), With<Shown>>,
    mut on: ResMut<OnObject>,
) {
    if frames.get(out.event_target()).is_ok() && on.0 {
        on.0 = false;
    }
}

/// Builds the hierarchy and the inspector again when the library
/// lists something new, so a material or shader just made is there to
/// pick.
fn relist(
    library: Res<Library>,
    mut open: ResMut<Open>,
    mut seen: Local<Option<u64>>,
) {
    if seen
        .replace(library.revision)
        .is_some_and(|was| was != library.revision)
    {
        open.changed();
    }
}

/// Asks for the pointing hand while the pointer is on an object a
/// click would select, the open hand on a handle and the closed one
/// while a handle is dragged. It lets go of the cursor only when it
/// was the one that asked for it.
fn point(
    on: Res<OnObject>,
    hot: Res<gizmo::Hot>,
    dragging: Res<gizmo::Dragging>,
    mut cursor: ResMut<OverrideCursor>,
    mut asked: Local<Option<SystemCursorIcon>>,
) {
    let want = if dragging.is_under_way() {
        Some(SystemCursorIcon::Grabbing)
    } else if hot.0.is_some() {
        Some(SystemCursorIcon::Grab)
    } else if on.0 {
        Some(SystemCursorIcon::Pointer)
    } else {
        None
    };
    if want == *asked {
        return;
    }
    if want.is_some() {
        cursor.0 = want;
    } else if cursor.0 == *asked {
        cursor.0 = None;
    }
    *asked = want;
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

/// The edges of one mesh an outline can follow: each a line between
/// two corners, with the normals of the faces on either side, or of
/// the one face of an open edge. Edges between faces of one plane are
/// left out, as no outline follows them.
struct Edges(Vec<(Vec3, Vec3, Vec3, Option<Vec3>)>);

impl Edges {
    /// The edges of `mesh`, its corners at one place taken as one.
    fn of(mesh: &Mesh) -> Self {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return Self(Vec::new());
        };
        let corners: Vec<Vec3> =
            positions.iter().map(|p| Vec3::from(*p)).collect();
        let order: Vec<usize> = match mesh.indices() {
            Some(indices) => indices.iter().collect(),
            None => (0..corners.len()).collect(),
        };
        // Corners split for flat shading are welded by place.
        let key = |p: Vec3| {
            let q = (p * 1e4).round();
            (q.x as i64, q.y as i64, q.z as i64)
        };
        let mut welded: HashMap<(i64, i64, i64), usize> =
            HashMap::new();
        let mut places = Vec::new();
        let id: Vec<usize> = corners
            .iter()
            .map(|p| {
                *welded.entry(key(*p)).or_insert_with(|| {
                    places.push(*p);
                    places.len() - 1
                })
            })
            .collect();
        let mut sides: HashMap<(usize, usize), Vec<Vec3>> =
            HashMap::new();
        for &[a, b, c] in order.as_chunks::<3>().0 {
            let (Some(pa), Some(pb), Some(pc)) =
                (corners.get(a), corners.get(b), corners.get(c))
            else {
                continue;
            };
            let normal =
                (*pb - *pa).cross(*pc - *pa).normalize_or_zero();
            if normal == Vec3::ZERO {
                continue;
            }
            for (from, to) in [(a, b), (b, c), (c, a)] {
                let (from, to) = (id[from], id[to]);
                if from == to {
                    continue;
                }
                sides
                    .entry((from.min(to), from.max(to)))
                    .or_default()
                    .push(normal);
            }
        }
        Self(
            sides
                .into_iter()
                .filter_map(|((from, to), normals)| {
                    let (a, b) = (places[from], places[to]);
                    match normals.as_slice() {
                        [one] => Some((a, b, *one, None)),
                        [one, two, ..] if one.dot(*two) < 0.9999 => {
                            Some((a, b, *one, Some(*two)))
                        }
                        _ => None,
                    }
                })
                .collect(),
        )
    }
}

/// Outlines the selected object and those inside it as Blender does:
/// along each edge where the surface turns away from the camera, and
/// along open edges.
fn outline(
    open: Res<Open>,
    stage: Option<Res<Stage>>,
    eyes: Query<&GlobalTransform>,
    pieces: Query<(&Mesh3d, &GlobalTransform)>,
    meshes: Res<Assets<Mesh>>,
    mut cache: Local<HashMap<Entity, Edges>>,
    mut gizmos: Gizmos<PrefabGizmos>,
) {
    let (Some(selected), Some(loaded), Some(stage)) =
        (&open.selected, &open.model, stage)
    else {
        cache.clear();
        return;
    };
    let Ok(eye) = eyes.get(stage.camera) else {
        return;
    };
    let eye = eye.translation();
    let inside = format!("{selected}/");
    let mut shown = Vec::new();
    for (object, (_, drawn)) in
        loaded.model.objects.iter().zip(&loaded.entities)
    {
        if object.path == *selected
            || object.path.starts_with(&inside)
        {
            shown.extend(drawn.iter().copied());
        }
    }
    cache.retain(|piece, _| shown.contains(piece));
    let colour = Color::srgb(1.0, 0.6, 0.15);
    for piece in shown {
        let Ok((mesh, at)) = pieces.get(piece) else {
            continue;
        };
        let edges = cache.entry(piece).or_insert_with(|| {
            meshes.get(&mesh.0).map_or(Edges(Vec::new()), Edges::of)
        });
        // Which side of a face the camera is on does not change
        // under the object's transform, so it is asked in the
        // mesh's own space.
        let local = at.affine().inverse().transform_point3(eye);
        for (a, b, one, two) in &edges.0 {
            let middle = (*a + *b) * 0.5;
            let towards = local - middle;
            let edge = match two {
                None => true,
                Some(two) => {
                    (one.dot(towards) > 0.0)
                        != (two.dot(towards) > 0.0)
                }
            };
            if edge {
                gizmos.line(
                    at.transform_point(*a),
                    at.transform_point(*b),
                    colour,
                );
            }
        }
    }
}

/// The panel: the bar, then the hierarchy, the view and the
/// inspector side by side.
fn screen() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(
            column((
                bar(),
                row((
                    hierarchy(),
                    sides::line::<Screen>(Side::Left),
                    view(),
                    sides::line::<Screen>(Side::Right),
                    inspector(),
                ))
                .width(percent(100.0))
                .height(percent(100.0))
                .min_height(px(0.0))
                .gap(gap),
            ))
            .width(percent(100.0))
            .height(percent(100.0))
            .gap(gap)
            .padding(UiRect::all(px(gap)))
            .with((
                Screen,
                Hovered::default(),
                Scope(SCOPE),
            )),
        )
    })
}

/// While the panel shows, opens a new prefab when none is, and keeps
/// each prefab open, as a new one with its changes, when its file is
/// removed.
fn settle(world: &mut World) {
    let shows = world
        .query_filtered::<(), With<Shown>>()
        .iter(world)
        .next()
        .is_some();
    if !shows {
        return;
    }
    let gone = |library: &Library, name: &Option<String>| {
        name.as_ref().is_some_and(|name| {
            library
                .find(Kind::Prefab, name)
                .is_none_or(|item| !item.is_own())
        })
    };
    world.resource_scope(
        |world, mut open: bevy::ecs::world::Mut<Open>| {
            let library = world.resource::<Library>();
            if gone(library, &open.name) {
                open.name = None;
                open.stored = Prefab::default();
            }
            let id = open.id;
            for doc in &mut open.docs {
                if doc.id != id && gone(library, &doc.name) {
                    doc.name = None;
                    doc.stored = Prefab::default();
                }
            }
        },
    );
    let open = world.resource::<Open>();
    if open.model.is_none() {
        if open.docs.is_empty() {
            add(world, None, Prefab::default());
        } else {
            let id = open.id;
            show(world, id);
        }
    }
}

/// The list of the prefabs open, as a dropdown showing the one shown
/// and a dot while it has changes not saved, the buttons that make a
/// new one, save it, save it as a new one and close it, and what last
/// went wrong; or the name being typed, or whether to save before
/// closing.
fn bar() -> AnyView {
    keyed(
        resource::<Open, _>(|open| {
            (open.label(), open.is_dirty(), open.naming, open.asking)
        }),
        |(shown, dirty, naming, asking): &(
            String,
            bool,
            Option<Naming>,
            bool,
        )| {
            let (shown, dirty, naming, asking) =
                (shown.clone(), *dirty, *naming, *asking);
            AnyView::new(move |cx: &mut Cx| {
                let gap = cx.theme().space.gap;
                let middle: AnyView = if asking {
                    ask(&shown)
                } else if let Some(naming) = naming {
                    naming_field(naming, &shown)
                } else {
                    row((
                        list_button(&shown, dirty),
                        styles::tool(
                            Glyph::Plus,
                            "New prefab: name it, then drag meshes \
                             in from ASSETS",
                            |world| {
                                world.resource_mut::<Open>().naming =
                                    Some(Naming::New);
                            },
                        ),
                        styles::tool(
                            Glyph::Save,
                            "Save the changes (Ctrl+S)",
                            |world| {
                                store(world);
                            },
                        ),
                        styles::tool(
                            Glyph::SaveAs,
                            "Save as… a new prefab under another \
                             name, and go on editing that one. This \
                             one stays as it was last saved",
                            |world| {
                                world.resource_mut::<Open>().naming =
                                    Some(Naming::Copy);
                            },
                        ),
                        styles::tool(
                            Glyph::Cross,
                            "Close this prefab",
                            close,
                        ),
                    ))
                    .gap(gap / 2.0)
                    .align(AlignItems::Center)
                    .boxed()
                };
                cx.build(
                    row((
                        glyph(Kind::Prefab.glyph(), ICON),
                        middle,
                        frame().grow(1.0),
                        // Only what went wrong stays in sight.
                        description(resource::<Open, _>(|open| {
                            if open.note.starts_with("Error") {
                                open.note.clone()
                            } else {
                                String::new()
                            }
                        })),
                    ))
                    .width(percent(100.0))
                    .gap(gap)
                    .align(AlignItems::Center),
                )
            })
        },
    )
    .within(column(()).gap(0.0).width(percent(100.0)))
    .boxed()
}

/// How wide the list of the prefabs open is, and its button.
const LIST_WIDTH: f32 = 220.0;

/// The button showing the prefab shown, `shown`, with a dot while it
/// is `dirty`, which opens the list of the prefabs open.
fn list_button(shown: &str, dirty: bool) -> AnyView {
    button(
        row((
            label(shown.to_owned()).wrap(false).grown(1.0),
            if dirty {
                hamlet_graph_editor::toolbar::unsaved()
            } else {
                column(()).boxed()
            },
            // The chevron is drawn pointing up.
            row((glyph(Glyph::Chevron, ICON),)).tagged(UiTransform {
                rotation: Rot2::degrees(180.0),
                ..UiTransform::IDENTITY
            }),
        ))
        .gap(4.0)
        .grow(1.0)
        .align(AlignItems::Center),
    )
    .width(px(LIST_WIDTH))
    .padding(UiRect::axes(px(8.0), px(2.0)))
    .rules(styles::button)
    .tagged(Variant::Default)
    .tooltip(|| note("The prefabs open: pick one to show it"))
    .on_activate_with(|world, node| {
        let anchor =
            crate::menu::rect_of(world, node).unwrap_or_default();
        world.resource_mut::<Listing>().0 = Some(anchor);
    })
    .boxed()
}

/// The list of the prefabs open, over everything while it is open:
/// each with a dot while it has changes not saved, then a way to make
/// a new one.
pub(crate) fn popups() -> AnyView {
    keyed(
        resource::<Listing, _>(|listing| listing.0),
        |anchor: &Option<Rect>| {
            let Some(anchor) = *anchor else {
                return frame().boxed();
            };
            AnyView::new(move |cx: &mut Cx| {
                let gap = cx.theme().space.gap;
                let open = cx.world.resource::<Open>();
                let shown = open.id;
                let rows = open
                    .listed()
                    .into_iter()
                    .map(|(id, called, dirty)| {
                        let tone = if id == shown {
                            Tone::Accent
                        } else {
                            Tone::Body
                        };
                        menu_item(
                            row((
                                label(called)
                                    .wrap(false)
                                    .tone(tone)
                                    .grown(1.0),
                                if dirty {
                                    hamlet_graph_editor::toolbar::unsaved()
                                } else {
                                    column(()).boxed()
                                },
                            ))
                            .gap(gap)
                            .grow(1.0)
                            .align(AlignItems::Center),
                        )
                        .on_activate(move |world| {
                            world.resource_mut::<Listing>().0 = None;
                            show(world, id);
                        })
                        .boxed()
                    })
                    .collect::<Vec<_>>();
                cx.build(
                    popup(anchor, (column(rows).gap(0.0),))
                        .on_dismiss(|world, _| {
                            world.resource_mut::<Listing>().0 = None;
                        })
                        .min_width(px(anchor.width().max(LIST_WIDTH))),
                )
            })
        },
    )
    .within(overlay(()))
    .boxed()
}

/// A button of the bar that says `text`, lit when `primary`, and
/// tells `tip` under the pointer when it is not empty.
fn word_button(
    text: &'static str,
    primary: bool,
    tip: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    let button = button(label(text))
        .padding(UiRect::axes(px(10.0), px(3.0)))
        .rules(if primary {
            styles::primary_button
        } else {
            styles::button
        })
        .tagged(Variant::Default)
        .on_activate(act);
    if tip.is_empty() {
        button.boxed()
    } else {
        button.tooltip(move || note(tip)).boxed()
    }
}

/// Whether to save the changes of the prefab `shown` before it
/// closes.
fn ask(shown: &str) -> AnyView {
    row((
        label(format!(
            "Save the changes to {shown} before closing it?"
        ))
        .bold(true),
        word_button("Save", true, "", |world| {
            if store(world) {
                close_now(world);
            } else {
                let mut open = world.resource_mut::<Open>();
                open.asking = false;
                open.closing = true;
            }
        }),
        word_button("Don't save", false, "", close_now),
        word_button("Cancel", false, "", |world| {
            world.resource_mut::<Open>().asking = false;
        }),
    ))
    .gap(8.0)
    .align(AlignItems::Center)
    .boxed()
}

/// The name to type for `naming`, starting from a free one near the
/// name of the prefab shown, `shown`.
fn naming_field(naming: Naming, shown: &str) -> AnyView {
    let (words, start) = match naming {
        Naming::First => ("Save as", "new_prefab".to_owned()),
        Naming::Copy => (
            "Save a copy as",
            if shown.is_empty() {
                "new_prefab".to_owned()
            } else {
                format!("{}_copy", shown.replace(' ', "_"))
            },
        ),
        Naming::New => ("New prefab named", "new_prefab".to_owned()),
    };
    let done = match naming {
        Naming::New => "Make",
        Naming::First | Naming::Copy => "Save",
    };
    row((
        label(words),
        text_field(start, |world, wanted| {
            // Focus leaving, as for Cancel, keeps what was typed
            // without doing it: only Enter does.
            let entered = world
                .resource::<ButtonInput<KeyCode>>()
                .any_just_pressed([
                    KeyCode::Enter,
                    KeyCode::NumpadEnter,
                ]);
            if entered {
                named(world, &wanted);
            }
        })
        .width(px(200.0))
        .rules(text_input)
        .tagged(crate::focus::TakesFocus),
        description(format!("Enter to {}", done.to_lowercase())),
        word_button("Cancel", false, "", |world| {
            let mut open = world.resource_mut::<Open>();
            open.naming = None;
            open.closing = false;
        }),
    ))
    .gap(8.0)
    .align(AlignItems::Center)
    .boxed()
}

/// Puts the mesh `name`, just added from a file dropped on the
/// window, in the prefab shown. Nothing changes while the panel does
/// not show.
pub(crate) fn mesh_dropped(world: &mut World, name: &str) {
    let shows = world
        .query_filtered::<(), With<Shown>>()
        .iter(world)
        .next()
        .is_some();
    if shows {
        put_in(world, name);
    }
}

/// Puts the mesh `mesh` in the prefab shown, selects it, and draws
/// it.
fn put_in(world: &mut World, mesh: &str) {
    let mut open = world.resource_mut::<Open>();
    let name = open.prefab.add(mesh);
    open.selected = Some(name);
    open.note = format!("Put {mesh} in");
    record(world);
    reload(world);
}

/// Takes the mesh put in as `name` out of the open prefab.
fn take_out(world: &mut World, name: &str) {
    let mut open = world.resource_mut::<Open>();
    if !open.prefab.remove(name) {
        return;
    }
    open.selected = None;
    record(world);
    reload(world);
}

/// Puts a mesh tile dragged from ASSETS and dropped on the view in
/// the open prefab.
fn dropped_on_view(
    drop: On<PointerDragDrop>,
    views: Query<(), With<Shown>>,
    mut commands: Commands,
) {
    if !views.contains(drop.event_target()) {
        return;
    }
    let dropped = drop.dropped;
    commands.queue(move |world: &mut World| {
        let Some(item) = tile_of(world, dropped) else {
            return;
        };
        if item.kind == Kind::Mesh {
            put_in(world, &item.name);
        } else {
            world.resource_mut::<Open>().note = format!(
                "Error: only meshes go in a prefab, not the {} {}",
                item.kind.word().to_lowercase(),
                item.name
            );
        }
    });
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
                if !open.error.is_empty() {
                    rows.push(
                        description(open.error.clone()).boxed(),
                    );
                }
                if objects.is_empty() {
                    rows.push(
                        description(
                            "Drag meshes from ASSETS onto the view \
                             to put them in.",
                        )
                        .boxed(),
                    );
                }
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
                    let part = open.prefab.part(&object.path);
                    let changed = !Part {
                        name: None,
                        ..part.clone()
                    }
                    .is_plain();
                    rows.push(object_row(
                        object.path.clone(),
                        part.name
                            .unwrap_or_else(|| object.name.clone()),
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
                cx.build(column(rows).gap(1.0))
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
            .width(sides::width::<Screen>(Side::Left))
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
        .rules(views::icon_button)
        .on_activate(move |world| {
            let mut state = world.resource_mut::<Open>();
            if !state.expanded.remove(&toggled) {
                state.expanded.insert(toggled.clone());
            }
            state.changed();
        })
        .boxed(),
        None => column(()).width(px(ICON)).shrink(0.0).boxed(),
    };
    let mark: AnyView = if changed {
        column(())
            .width(px(6.0))
            .height(px(6.0))
            .shrink(0.0)
            .radius(3.0)
            .fill(Color::srgb(0.85, 0.65, 0.3))
            .tooltip(|| note("Changed in this prefab"))
            .boxed()
    } else {
        column(()).boxed()
    };
    let tip = path.replace('/', " / ");
    let pick = button(
        row((
            glyph(Glyph::Cube, ICON).tone(if selected {
                Tone::Accent
            } else {
                Tone::Dim
            }),
            column((one_line(label(name)),))
                .grow(1.0)
                .min_width(px(0.0))
                .overflow(Overflow::clip()),
            mark,
        ))
        .width(percent(100.0))
        .gap(6.0)
        .align(AlignItems::Center),
    )
    .grow(1.0)
    .min_width(px(0.0))
    .justify(JustifyContent::FlexStart)
    .padding(UiRect::axes(px(6.0), px(4.0)))
    .rules(row_rules)
    .tooltip(move || note(tip.clone()));
    let pick = if selected {
        pick.tagged(Selected).boxed()
    } else {
        pick.boxed()
    };
    row((
        column(()).width(px(depth as f32 * 14.0)).shrink(0.0),
        toggle,
        pick.on_activate(move |world| {
            let mut open = world.resource_mut::<Open>();
            open.selected = Some(path.clone());
            open.changed();
        }),
    ))
    .width(percent(100.0))
    .gap(2.0)
    .align(AlignItems::Center)
    .boxed()
}

/// Marks the selected object's row.
#[derive(Component)]
struct Selected;

/// A row of the hierarchy: bare, lit under the pointer and while
/// selected.
fn row_rules(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(Color::NONE)
                .border_color(Color::NONE)
                .radius(t.space.round)
        })
        .hovered(|s| s.frame(|f, t| f.fill(t.palette.gold_hover)))
        .when::<Selected>(|s| {
            s.frame(|f, t| f.fill(t.palette.primary_low))
        })
        .apply(cx);
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
            .rules(views::icon_button)
            .tooltip(|| note("Forget this part's settings"))
            .on_activate(move |world| {
                let mut open = world.resource_mut::<Open>();
                open.prefab.parts.remove(&path);
                open.changed();
                record(world);
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
            .tagged(Shown)
            .tooltip(|| {
                note("Drag a mesh here from ASSETS to put it in")
            });
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
                        "Click an object in the view or the \
                         hierarchy to change it here.",
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
                        Finish::Own => "from the file".to_owned(),
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
                let file_name = path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&path)
                    .to_owned();
                let shown = part
                    .name
                    .clone()
                    .unwrap_or_else(|| file_name.clone());
                let name = text_field(shown, move |world, to| {
                    let to = to.trim().to_owned();
                    let name = (!to.is_empty() && to != file_name)
                        .then_some(to);
                    edit_part(world, |part| part.name = name);
                    world.resource_mut::<Open>().changed();
                })
                .width(percent(100.0))
                .rules(text_input);
                let mut rows: Vec<AnyView> = vec![
                    labelled("name", name.boxed()),
                    labelled("material", finish.boxed()),
                ];
                let put = open
                    .prefab
                    .meshes
                    .iter()
                    .any(|placed| placed.name == path);
                if put {
                    let out = path.clone();
                    rows.push(
                        button(label("Take out of the prefab"))
                            .padding(UiRect::axes(px(8.0), px(2.0)))
                            .tooltip(|| {
                                note(
                                    "Take this mesh out, with its \
                                     changes",
                                )
                            })
                            .rules(styles::button)
                            .tagged(Variant::Default)
                            .on_activate(move |world| {
                                take_out(world, &out);
                            })
                            .boxed(),
                    );
                }
                if part.name.is_some() {
                    rows.push(
                        note(format!(
                            "In the file: {}",
                            path.replace('/', " / ")
                        ))
                        .boxed(),
                    );
                }
                if let Some(error) = error {
                    rows.push(
                        description(format!("Error: {error}"))
                            .boxed(),
                    );
                }
                rows.push(caption("Position"));
                rows.push(axes(
                    |part| part.offset.to_array(),
                    |part, axis, value| {
                        part.offset[axis] = value;
                    },
                    0.01,
                    "m",
                    2,
                    false,
                ));
                rows.push(caption("Rotation"));
                rows.push(axes(
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
                    "°",
                    0,
                    false,
                ));
                rows.push(caption("Scale"));
                rows.push(axes(
                    |part| part.scale.to_array(),
                    |part, axis, value| {
                        part.scale[axis] = value;
                    },
                    0.01,
                    "",
                    3,
                    true,
                ));
                rows.push(
                    button(label("Reset to file"))
                        .padding(UiRect::axes(px(8.0), px(2.0)))
                        .tooltip(|| {
                            note(
                                "Give the object back the material, \
                                 position, rotation and scale the \
                                 file gives it, keeping its name",
                            )
                        })
                        .rules(styles::button)
                        .tagged(Variant::Default)
                        .on_activate(|world| {
                            edit_part(world, |part| {
                                *part = Part {
                                    name: part.name.take(),
                                    ..Part::default()
                                };
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
            .width(sides::width::<Screen>(Side::Right))
            .shrink(0.0)
            .height(percent(100.0))
            .fill(theme.palette.card)
            .border(1.0)
            .border_color(theme.palette.border),
        )
    })
}

/// What a group of axes is, above its rows, as Blender words it.
fn caption(text: &str) -> AnyView {
    column((label(text.to_owned()).tone(Tone::Dim),))
        .padding(UiRect::top(px(4.0)))
        .boxed()
}

/// A row each for X, Y and Z of what `get` reads of the selected
/// part, which `put` changes: the axis on the left, the number and
/// its `unit` on the right, rounded to `decimals`. `pad` keeps every
/// decimal, as scale's 1.000; otherwise trailing zeros go, as 0 m.
fn axes(
    get: fn(&Part) -> [f32; 3],
    put: fn(&mut Part, usize, f32),
    step: f64,
    unit: &'static str,
    decimals: i32,
    pad: bool,
) -> AnyView {
    let k = 10f64.powi(decimals);
    let round = move |value: f64| (value * k).round() / k;
    let rows = (0..3)
        .map(|axis| {
            let field = number_field(
                resource::<Open, _>(move |open| {
                    // `+ 0.0` turns -0 into 0.
                    open.part().map_or(0.0, |part| {
                        round(f64::from(get(&part)[axis])) + 0.0
                    })
                }),
                move |world: &mut World, value: f64| {
                    edit_part(world, |part| {
                        put(part, axis, round(value) as f32);
                    });
                },
            )
            .step(step);
            let field = if pad {
                field.precision(decimals.unsigned_abs() as usize)
            } else {
                field
            };
            // The unit sits beside the number, out of the text typed,
            // so what is typed still reads as a number.
            row((
                column((
                    label(["X", "Y", "Z"][axis]).tone(Tone::Dim),
                ))
                .shrink(0.0)
                .padding(UiRect::left(px(8.0))),
                field
                    .grow(1.0)
                    .min_width(px(0.0))
                    .rules(bare_field)
                    .tagged(RightAligned),
                column((label(unit).tone(Tone::Dim),))
                    .shrink(0.0)
                    .padding(UiRect::right(px(if unit.is_empty() {
                        4.0
                    } else {
                        8.0
                    }))),
            ))
            .width(percent(100.0))
            .min_width(px(0.0))
            .gap(4.0)
            .align(AlignItems::Center)
            .rules(axis_row)
            .boxed()
        })
        .collect::<Vec<_>>();
    column(rows).width(percent(100.0)).gap(2.0).boxed()
}

/// A row of [`axes`]: a quiet bar, lighter under the pointer.
fn axis_row(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.input)
                .border_color(Color::NONE)
                .radius(t.space.round)
        })
        .hovered(|s| s.frame(|f, t| f.fill(t.palette.button_hover)))
        .apply(cx);
}

/// A number field whose text stands at its right edge.
#[derive(Component)]
struct RightAligned;

/// Puts the text of every [`RightAligned`] field at its right edge.
fn align_right(
    fields: Query<Entity, With<RightAligned>>,
    children: Query<&Children>,
    mut layouts: Query<&mut TextLayout>,
) {
    for field in &fields {
        for kid in std::iter::once(field)
            .chain(children.iter_descendants(field))
        {
            if let Ok(mut layout) = layouts.get_mut(kid)
                && layout.justify != Justify::Right
            {
                layout.justify = Justify::Right;
            }
        }
    }
}

/// A number field inside a box that draws the frame: no fill or
/// border of its own.
fn bare_field(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, _| f.fill(Color::NONE).border_color(Color::NONE))
        .hovered(|s| {
            s.frame(|f, _| {
                f.fill(Color::NONE).border_color(Color::NONE)
            })
        })
        .when::<Focused>(|s| {
            s.frame(|f, _| {
                f.fill(Color::NONE).border_color(Color::NONE)
            })
        })
        .apply(cx);
}
