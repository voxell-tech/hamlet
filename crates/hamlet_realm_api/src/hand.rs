//! What the chief's hand on the world holds, and the robots it may
//! cheer.

use bevy::ecs::component::Component;
use bevy::ecs::resource::Resource;

/// The hand's hold on the thing under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    /// It could take it.
    Ready,
    /// It carries a robot.
    Robot,
}

/// The hand's [`Grip`], none while it neither holds nor could take
/// anything.
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub struct Hold(pub Option<Grip>);

/// On an agent whose work ended a moment ago, until that clock
/// time: a click on its robot cheers it.
#[derive(Component, Clone, Copy, Debug)]
pub struct Proud {
    pub until: f32,
}
