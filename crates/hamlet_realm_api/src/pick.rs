//! The things of the world a pointer can choose, the one under it,
//! the ones chosen and the camera brought to one.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::name::Name;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Query;
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::window::Window;

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
            Thing::Clockwork(hamlet) => {
                format!("The Clockwork of {}", resident(hamlet))
            }
            Thing::Workbench(hamlet) => {
                format!("The Workbench of {}", resident(hamlet))
            }
        }
    }

    /// True when it is an agent.
    pub fn agent(self) -> bool {
        matches!(self, Thing::Keeper | Thing::Resident(_))
    }

    /// The thing a click on it chooses.
    pub fn chosen(self) -> Thing {
        match self {
            Thing::Clockwork(hamlet) | Thing::Workbench(hamlet) => {
                Thing::Common(hamlet)
            }
            other => other,
        }
    }

    /// The few words shown over it all the time, when it is named.
    pub fn sign(self, names: &Query<&Name>) -> String {
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

/// Asks about `thing` at the pointer, as a right click on it does.
pub fn ask_about(world: &mut World, thing: Thing) {
    let at = world
        .query::<&Window>()
        .iter(world)
        .find_map(Window::cursor_position)
        .unwrap_or_default();
    world.resource_mut::<Asked>().0 = Some((thing, at));
}

/// On a row of a panel: the thing it stands for, whose menu the
/// right button opens there as it does in the world.
#[derive(Component)]
pub struct Asks(pub Thing);

/// The point of the window at which the bare ground was asked about
/// with the right button.
#[derive(Resource, Default, PartialEq)]
pub struct AskedGround(pub Option<Vec2>);

/// A UI node over the world that takes a click, with all under it.
#[derive(Component)]
pub struct Pressable;

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

/// A thing the camera stays on, until the ground is slid by hand.
#[derive(Resource, Default, PartialEq)]
pub struct Following(pub Option<Thing>);

/// The way a drag on the world moves the camera.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Steer {
    #[default]
    Still,
    /// The ground slides under the pointer.
    Slide,
    /// The view turns and tilts.
    Turn,
}

/// Brings `thing` to the middle of the view and comes in close, and
/// stays on it if `stay`. The realm carries it out.
#[derive(Event, Clone, Copy, Debug)]
pub struct Focus {
    pub thing: Thing,
    pub stay: bool,
}

/// Asks the realm to bring `thing` into view, as [`Focus`] says.
pub fn focus_on(world: &mut World, thing: Thing, stay: bool) {
    world.trigger(Focus { thing, stay });
    world.flush();
}
