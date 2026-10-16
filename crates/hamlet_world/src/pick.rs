//! The things of the world a pointer can hit, the one under it and
//! the ones chosen.

mod camera;
mod reach;
mod rings;
mod signs;
#[cfg(test)]
mod tests;

use bevy::asset::uuid_handle;
use bevy::camera::primitives::Aabb;
use bevy::math::bounding::Aabb3d;
use bevy::picking::PickingSystems;
use bevy::picking::backend::ray::RayMap;
use bevy::picking::events::pointer_events;
use bevy::picking::hover::{HoverMap, generate_hovermap};
use bevy::picking::mesh_picking::ray_cast::{
    RayCastVisibility, ray_aabb_intersection_3d,
};
use bevy::picking::mesh_picking::{
    MeshPickingCamera, MeshPickingPlugin, MeshPickingSettings,
};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

pub use self::camera::{
    Centre, Following, Glide, Leash, Orbit, Stance, Steer, Steering,
    focus_on,
};
pub use self::reach::{Reach, aim};
pub use self::signs::{
    NameTag, Named, NoNameTag, Reads, Sign, Signed,
};
use crate::play::Played;
use crate::sky::CameraSettings;
use crate::works::Switched;
use crate::{WorldCamera, WorldSystems};

/// Hover, selection and the camera moved by hand. What can be
/// pointed at is up to whoever spawns it with [`watch`].
pub fn plugin(app: &mut App) {
    app.add_plugins((
        MeshPickingPlugin,
        camera::plugin,
        rings::plugin,
        signs::plugin,
    ))
    // Only marked things are tested, never the large meshes.
    .insert_resource(MeshPickingSettings {
        require_markers: true,
        ray_cast_visibility: RayCastVisibility::Any,
    })
    .init_resource::<Hovered>()
    .init_resource::<Selected>()
    .init_resource::<Also>()
    .init_resource::<Asked>()
    .init_resource::<AskedGround>()
    .init_resource::<CameraSettings>()
    .init_resource::<PickSettings>()
    .init_resource::<Travel>()
    .init_resource::<Taken>()
    .init_resource::<Reach>()
    .add_message::<Played>()
    .add_observer(press)
    .add_observer(click_surface)
    .add_observer(click)
    // Before the pointer's events, so a click reads this frame's
    // hover.
    .add_systems(
        PreUpdate,
        hover
            .in_set(PickingSystems::Hover)
            .after(generate_hovermap)
            .before(pointer_events),
    )
    .add_systems(Update, (mark_camera, aim.before(WorldSystems)));
}

/// The room each thing takes for the pointer, its ring and its name.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickSettings {
    /// The box a pointer can hit for a robot.
    pub robot_box: Vec3,
    /// Radius and height of the room a house takes.
    pub house: (f32, f32),
    /// Radius and height of the room a common takes.
    pub common: (f32, f32),
    /// Height of a ring above the ground.
    pub ring_lift: f32,
    /// Distance past its ring to which the ground under a thing that
    /// moves is looked up, so that it is not looked up again every
    /// frame.
    pub ring_slack: f32,
    /// Camera distance beyond which names are not shown.
    pub signs_far: f32,
}

/// A thing in the world that can be pointed at.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Thing {
    /// The mayor.
    Keeper,
    /// An agent besides the mayor: the entity that carries its
    /// `Name`.
    Resident(Entity),
    /// The house of that agent.
    ResidentHouse(Entity),
    /// The remains of the house of an agent that was removed or
    /// merged.
    Ruin(Entity),
    /// The mayor's house.
    Cottage,
    /// A hamlet's common: the entity that carries the hamlet's
    /// `Name`.
    Common(Entity),
    /// A chief's mascot: the entity that carries the chief's
    /// `Name`.
    Chief(Entity),
    /// The place of that chief.
    ChiefPlace(Entity),
    /// The library every agent shares.
    Library,
    /// The Agent Bazaar, where the home's models are bought.
    Bazaar,
    /// One stall of the bazaar.
    Stall(crate::bazaar::Stall),
    /// The clockwork on a hamlet's workbench, which sets its mode.
    Clockwork(Entity),
    /// The workbench on a hamlet's common, which stands for its
    /// folder.
    Workbench(Entity),
}

impl Thing {
    pub fn name(self, names: &Query<&Name>) -> String {
        let resident = |agent: Entity| {
            names
                .get(agent)
                .map(|name| name.as_str().to_string())
                .unwrap_or_default()
        };
        match self {
            Thing::Keeper => "The Mayor".into(),
            Thing::Resident(agent) => resident(agent),
            Thing::ResidentHouse(agent) => {
                format!("{}'s House", resident(agent))
            }
            Thing::Ruin(agent) => {
                format!("{}'s Ruin", resident(agent))
            }
            Thing::Cottage => "The Mayor's House".into(),
            Thing::Common(hamlet) => {
                format!("The Common of {}", resident(hamlet))
            }
            Thing::Chief(chief) => {
                format!("{}, Chief", resident(chief))
            }
            Thing::ChiefPlace(chief) => {
                format!("{}'s Hearth", resident(chief))
            }
            Thing::Library => "The Library".into(),
            Thing::Bazaar => "The Agent Bazaar".into(),
            Thing::Stall(stall) => format!("The {}", stall.name()),
            Thing::Clockwork(hamlet) => {
                format!("The Clockwork of {}", resident(hamlet))
            }
            Thing::Workbench(hamlet) => {
                format!("The Workbench of {}", resident(hamlet))
            }
        }
    }
}

impl Thing {
    /// True when it is an agent.
    fn agent(self) -> bool {
        matches!(self, Thing::Keeper | Thing::Resident(_))
    }

    /// The thing a click on it chooses.
    fn chosen(self) -> Thing {
        match self {
            Thing::Clockwork(hamlet) | Thing::Workbench(hamlet) => {
                Thing::Common(hamlet)
            }
            other => other,
        }
    }

    /// The few words shown over it all the time, when it is `Named`.
    fn sign(self, names: &Query<&Name>) -> String {
        match self {
            Thing::Cottage => "Mayor".into(),
            Thing::ResidentHouse(agent) | Thing::Ruin(agent) => {
                Thing::Resident(agent).name(names)
            }
            // A hamlet is known by the name over its common.
            Thing::Common(hamlet) => {
                Thing::Resident(hamlet).name(names)
            }
            other => other.name(names),
        }
    }
}

/// The room a thing takes, for the ring around it and the name over
/// it.
#[derive(Component, Clone, Copy)]
pub(crate) struct Footprint {
    pub radius: f32,
    pub height: f32,
    /// Height of its origin above the ground it stands on.
    pub lift: f32,
}

impl Footprint {
    /// For a box of `size` whose origin is its middle.
    fn of_box(size: Vec3) -> Self {
        Footprint {
            radius: size.xz().max_element() * 0.62,
            height: size.y,
            lift: size.y * 0.5,
        }
    }
}

/// The thing under the pointer.
#[derive(Resource, Default, PartialEq)]
pub struct Hovered(pub Option<Thing>);

/// The thing last clicked.
#[derive(Resource, Default, PartialEq)]
pub struct Selected(pub Option<Thing>);

/// A second thing, chosen with Shift held, for what takes two.
#[derive(Resource, Default, PartialEq)]
pub struct Also(pub Option<Thing>);

/// Selects `thing`, or nothing, and lets go of any second choice.
pub fn select(world: &mut World, thing: Option<Thing>) {
    world.resource_mut::<Selected>().0 = thing;
    world.resource_mut::<Also>().0 = None;
}

/// A thing that was asked about with the right button, and where the
/// pointer was in the window.
#[derive(Resource, Default, PartialEq)]
pub struct Asked(pub Option<(Thing, Vec2)>);

/// The point of the window at which the bare ground was asked about
/// with the right button.
#[derive(Resource, Default, PartialEq)]
pub struct AskedGround(pub Option<Vec2>);

/// A UI node showing the world, which takes the pointer for the
/// camera.
#[derive(Component)]
pub struct WorldSurface;

/// A UI node over the world that takes a click, with all under it.
#[derive(Component)]
pub struct Pressable;

/// The pixels the pointer has moved since it was pressed.
#[derive(Resource, Default)]
pub(crate) struct Travel(pub f32);

/// The parts of the pointer someone else has taken: a click then
/// selects nothing, a drag does not slide the ground, and Escape lets
/// go of nothing here.
#[derive(Resource, Default, PartialEq)]
pub struct Taken {
    pub click: bool,
    pub drag: bool,
    pub escape: bool,
    /// Something was open over the world as the frame began, and
    /// Escape closes that.
    pub over: bool,
}

/// Makes `entity` answer the pointer.
pub fn watch(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).try_insert(Pickable::default());
}

/// A box of side 1 about its middle, which every hit box is scaled
/// from.
const UNIT_BOX: Handle<Mesh> =
    uuid_handle!("6d0f3c52-8f0e-4a5e-9c1b-2f3a8f1e7b40");

/// An unseen box of `size` about `middle` that stands for `thing`. It
/// is scaled to its size, so it takes no children.
pub fn hit_box(
    thing: Thing,
    size: Vec3,
    middle: Vec3,
    meshes: &mut Assets<Mesh>,
) -> impl Bundle {
    if !meshes.contains(&UNIT_BOX) {
        // A fixed handle is always taken.
        let _ =
            meshes.insert(&UNIT_BOX, Cuboid::from_length(1.0).into());
    }
    (
        thing,
        Footprint::of_box(size),
        Mesh3d(UNIT_BOX),
        Transform::from_translation(middle).with_scale(size),
        Visibility::Hidden,
    )
}

fn mark_camera(
    mut commands: Commands,
    cameras: Query<
        Entity,
        (With<WorldCamera>, Without<MeshPickingCamera>),
    >,
) {
    for camera in &cameras {
        commands.entity(camera).try_insert(MeshPickingCamera);
    }
}

/// Sets [`Hovered`] to the thing under the pointer. An agent is
/// found through whatever stands in front of it.
fn hover(
    over: Res<HoverMap>,
    rays: Res<RayMap>,
    things: Query<&Thing, With<Pickable>>,
    boxes: Query<
        (Entity, &Thing, &Aabb, &GlobalTransform),
        With<Pickable>,
    >,
    surfaces: Query<(), With<WorldSurface>>,
    cameras: Query<(), With<WorldCamera>>,
    mut hovered: ResMut<Hovered>,
) {
    // The nearest agent whose box the ray enters. A box the ray
    // begins in is passed over.
    let agent = |ray: Ray3d| {
        boxes
            .iter()
            .filter(|(_, thing, ..)| thing.agent())
            .filter_map(|(entity, _, room, at)| {
                let room =
                    Aabb3d::new(room.center, room.half_extents);
                ray_aabb_intersection_3d(ray, &room, &at.affine())
                    .filter(|far| far.is_finite() && *far > 0.0)
                    .map(|far| (far, entity))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, entity)| entity)
    };
    // The surface is under the mouse, and the things under the
    // pointer the panel forwards into its picture.
    let hits = || over.values().flat_map(|hits| hits.keys()).copied();
    let found = hits()
        .any(|hit| surfaces.contains(hit))
        .then(|| {
            let behind = rays
                .iter()
                .filter(|(ray, _)| cameras.contains(ray.camera))
                .find_map(|(_, ray)| agent(*ray));
            behind
                .or_else(|| hits().find(|hit| things.contains(*hit)))
        })
        .flatten()
        .and_then(|hit| things.get(hit).ok().copied());
    if hovered.0 != found {
        hovered.0 = found;
    }
}

/// The left button selects, or with Shift chooses a second thing.
/// The right button asks what can be done with a thing.
#[allow(clippy::too_many_arguments)]
fn click(
    click: On<Pointer<Click>>,
    things: Query<&Thing>,
    hovered: Res<Hovered>,
    travel: Res<Travel>,
    taken: Res<Taken>,
    camera: Res<CameraSettings>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut selected: ResMut<Selected>,
    mut also: ResMut<Also>,
    mut asked: ResMut<Asked>,
    mut commands: Commands,
) {
    let Ok(thing) = things.get(click.event_target()).copied() else {
        return;
    };
    if taken.click {
        return;
    }
    // An agent behind what took the click is the one meant.
    let thing =
        hovered.0.filter(|over| over.agent()).unwrap_or(thing);
    if travel.0 > camera.click_slop {
        return;
    }
    let shift = keys.is_some_and(|keys| {
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
    });
    if let (Thing::Clockwork(hamlet), PointerButton::Primary, false) =
        (thing, click.button, shift)
    {
        commands.trigger(Switched { hamlet });
        return;
    }
    let thing = thing.chosen();
    match click.button {
        PointerButton::Primary if shift => {
            // Shift on the second thing again lets it go.
            also.0 = (also.0 != Some(thing)
                && selected.0 != Some(thing))
            .then_some(thing);
        }
        PointerButton::Primary => {
            selected.0 = Some(thing);
            also.0 = None;
        }
        PointerButton::Secondary => {
            let at = window
                .iter()
                .find_map(Window::cursor_position)
                .unwrap_or_default();
            asked.0 = Some((thing, at));
        }
        PointerButton::Middle => {}
    }
}

/// A click on the bare world clears the selection, and the right
/// button there asks what can be done with the ground.
#[allow(clippy::too_many_arguments)]
fn click_surface(
    click: On<Pointer<Click>>,
    surfaces: Query<(), With<WorldSurface>>,
    window: Query<&Window, With<PrimaryWindow>>,
    hovered: Res<Hovered>,
    travel: Res<Travel>,
    taken: Res<Taken>,
    camera: Res<CameraSettings>,
    mut selected: ResMut<Selected>,
    mut also: ResMut<Also>,
    mut ground: ResMut<AskedGround>,
) {
    if click.button == PointerButton::Secondary
        && surfaces.contains(click.event_target())
        && hovered.0.is_none()
        && !taken.click
        && travel.0 <= camera.click_slop
    {
        ground.0 = window.iter().find_map(Window::cursor_position);
    }
    if click.button == PointerButton::Primary
        && surfaces.contains(click.event_target())
        && hovered.0.is_none()
        && !taken.click
        && travel.0 <= camera.click_slop
    {
        if selected.0.is_some() {
            selected.0 = None;
        }
        if also.0.is_some() {
            also.0 = None;
        }
    }
}

fn press(
    press: On<Pointer<Press>>,
    surfaces: Query<(), With<WorldSurface>>,
    mut travel: ResMut<Travel>,
) {
    if surfaces.contains(press.event_target()) {
        travel.0 = 0.0;
    }
}
