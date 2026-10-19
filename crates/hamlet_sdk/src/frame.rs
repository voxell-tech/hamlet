//! The window around the dock as plugins fill it: the parts a
//! built-in plugin adds to the frame, the window's states and what
//! the frame reads of the plugins.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::SystemSet;
use bevy::ecs::world::World;
use bevy::state::state::{ComputedStates, States};

use crate::AnyView;
use crate::page::Native;

/// Where in the window a part of the frame goes.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug,
)]
pub enum Place {
    /// The top bar, before the room the window is held by.
    Bar,
    /// Over the dock, in the room the dock takes.
    Dock,
    /// The foot of the window.
    Foot,
    /// Over everything: popups and cards.
    Over,
}

/// A part of the frame a built-in plugin adds, drawn by the view in
/// its [`Native`]. Parts of a place are drawn by `order`.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct FramePart {
    pub place: Place,
    pub order: i32,
}

/// The parts there are, in the order they are drawn.
pub fn parts(world: &World) -> Vec<(FramePart, Entity)> {
    let Some(mut query) = world.try_query::<(Entity, &FramePart)>()
    else {
        return Vec::new();
    };
    let mut parts = query
        .iter(world)
        .map(|(entity, part)| (*part, entity))
        .collect::<Vec<_>>();
    parts.sort_by_key(|(part, entity)| {
        (part.place, part.order, *entity)
    });
    parts
}

/// The views of the parts at `place`.
pub fn views(world: &World, place: Place) -> Vec<AnyView> {
    parts(world)
        .into_iter()
        .filter(|(part, _)| part.place == place)
        .filter_map(|(_, entity)| world.get::<Native>(entity))
        .map(|Native(view)| view())
        .collect()
}

/// The systems that mount the window's views once the theme is
/// ready. What a view reads as it is built is set before them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Mounting;

/// The window on screen or in the background.
#[derive(
    States, Clone, Copy, PartialEq, Eq, Hash, Debug, Default,
)]
pub enum Showing {
    #[default]
    Shown,
    /// Background: the window is hidden and the agents keep running.
    Hidden,
}

/// The world is drawn: the window is on screen.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct WorldShown;

impl ComputedStates for WorldShown {
    type SourceStates = Showing;

    fn compute(showing: Showing) -> Option<Self> {
        (showing == Showing::Shown).then_some(WorldShown)
    }
}

/// Something on screen moves: while the window is shown it is drawn
/// every frame.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Animating(pub bool);

/// A step is about to run, and the pointer says the app is busy.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PointerWait(pub bool);
