//! The things of the world a pointer can point at, the one under it
//! and the ones chosen, and asking the world's camera to look at one.

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::name::Name;
use bevy::ecs::query::Has;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Query;
use bevy::ecs::world::World;
use bevy::math::Vec2;
use hamlet_client::realm::{
    AgentName, Identity, Lead, Left, Lodging, Shown,
};

pub fn plugin(app: &mut App) {
    app.init_resource::<Hovered>()
        .init_resource::<Selected>()
        .init_resource::<Also>()
        .init_resource::<Asked>()
        .init_resource::<AskedGround>()
        .init_resource::<Taken>()
        .init_resource::<Following>();
}

/// A thing in the world that can be pointed at.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Thing {
    /// The lead.
    Keeper,
    /// An agent besides the lead: the entity that carries its `Name`.
    Agent(Entity),
    /// The house of that agent.
    House(Entity),
    /// The remains of the house of an agent that was removed or
    /// merged.
    Ruin(Entity),
    /// The lead's house.
    Cottage,
    /// A hamlet's common: the entity that carries the hamlet's
    /// `Name`.
    Common(Entity),
    /// A chief's mascot: the entity that carries the chief's `Name`.
    Chief(Entity),
    /// The place of that chief.
    ChiefPlace(Entity),
    /// The library every agent shares.
    Library,
    /// The Hand's tower, at the middle of the city.
    Hand,
    /// The house of a profile: the entity that carries its `Profile`
    /// and `Name`.
    Profile(Entity),
    /// The clockwork on a hamlet's workbench, which sets its mode.
    Clockwork(Entity),
    /// The workbench on a hamlet's common, which stands for its
    /// folder.
    Workbench(Entity),
}

/// What the names of things are read from: an agent's `Shown` name, a
/// profile's `Lodging`, the lead's `AgentName`, all over its `Name`.
pub type Names<'w, 's> = Query<
    'w,
    's,
    (
        &'static Name,
        Option<&'static Shown>,
        Option<&'static Lodging>,
        Option<&'static AgentName>,
        Has<Lead>,
        Has<Left>,
        Option<&'static Identity>,
    ),
>;

/// The own name of the first lead, which the world calls it by.
fn lead_name(names: &Names) -> Option<String> {
    names
        .iter()
        .filter(|(.., lead, left, _)| *lead && !left)
        .min_by_key(|(.., identity)| identity.map(|i| i.0))
        .and_then(|(_, _, _, name, ..)| name)
        .map(|name| name.0.clone())
}

impl Thing {
    pub fn name(self, names: &Names) -> String {
        let named = |agent: Entity| match names.get(agent) {
            Ok((_, Some(shown), ..)) => shown.0.clone(),
            Ok((name, None, ..)) => name.as_str().to_string(),
            Err(_) => String::new(),
        };
        let lodged = |profile: Entity, names: &Names| match names
            .get(profile)
        {
            Ok((_, _, Some(lodging), ..)) => lodging.0.clone(),
            _ => named(profile),
        };
        match self {
            Thing::Keeper => {
                lead_name(names).unwrap_or("The Lead".into())
            }
            Thing::Agent(agent) => named(agent),
            Thing::House(agent) => {
                format!("{}'s House", named(agent))
            }
            Thing::Ruin(agent) => format!("{}'s Ruin", named(agent)),
            Thing::Cottage => format!(
                "{}'s House",
                lead_name(names).unwrap_or("The Lead".into())
            ),
            Thing::Common(hamlet) => {
                format!("The Common of {}", named(hamlet))
            }
            Thing::Chief(chief) => format!("{}, Chief", named(chief)),
            Thing::ChiefPlace(chief) => {
                format!("{}'s Hearth", named(chief))
            }
            Thing::Library => "The Library".into(),
            Thing::Hand => "The Hand's Tower".into(),
            Thing::Profile(profile) => {
                format!("{}'s House", lodged(profile, names))
            }
            Thing::Clockwork(hamlet) => {
                format!("The Clockwork of {}", named(hamlet))
            }
            Thing::Workbench(hamlet) => {
                format!("The Workbench of {}", named(hamlet))
            }
        }
    }

    /// True when it is an agent.
    pub fn is_agent(self) -> bool {
        matches!(self, Thing::Keeper | Thing::Agent(_))
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

    /// The few words shown over it all the time, when it is named in
    /// the world.
    pub fn sign(self, names: &Names) -> String {
        match self {
            Thing::Cottage => {
                lead_name(names).unwrap_or("Lead".into())
            }
            Thing::House(agent) | Thing::Ruin(agent) => {
                Thing::Agent(agent).name(names)
            }
            // A hamlet is known by the name over its common.
            Thing::Common(hamlet) => Thing::Agent(hamlet).name(names),
            // A profile's house carries what it is lodged under.
            Thing::Profile(profile) => match names.get(profile) {
                Ok((_, _, Some(lodging), ..)) => lodging.0.clone(),
                _ => Thing::Agent(profile).name(names),
            },
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

/// The point of the window at which the bare ground was asked about
/// with the right button.
#[derive(Resource, Default, PartialEq)]
pub struct AskedGround(pub Option<Vec2>);

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

/// Brings a thing into view, chosen from a panel: where it is already
/// well inside the view nothing moves, and otherwise the view slides
/// only as far as brings it inside the view's margin.
#[derive(Event, Clone, Copy, Debug, PartialEq)]
pub struct BringIntoView(pub Thing);

/// Glides the camera close to a thing, and keeps it there while
/// `stay` holds, until the ground is slid by hand.
#[derive(Event, Clone, Copy, Debug, PartialEq)]
pub struct FocusOn {
    pub thing: Thing,
    pub stay: bool,
}
