//! The chief this window belongs to, as the panels ask about it: its
//! page in a Hub, the way to its Hearth, and the company its mascot
//! keeps.

use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::resource::Resource;
use hamlet_ui::AnyView;

/// Selects the Hearth of the chief this window belongs to and brings
/// it into view.
#[derive(Event, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoHome;

/// A message sent from the following Hub's field to `agent`.
#[derive(Event, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spoke(pub Entity);

/// The page a Hub shows for a chief, set by the part that draws the
/// chiefs. A Hub shows nothing for one without it.
#[derive(Resource, Clone, Copy)]
pub struct ChiefPage(pub fn(Entity) -> AnyView);
