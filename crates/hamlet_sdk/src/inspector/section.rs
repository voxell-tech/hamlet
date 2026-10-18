//! A page a plugin adds to the inspector.

use bevy::ecs::component::Component;

/// On an entity a plugin declared: a tab of the inspector's page of
/// the things of one kind, whose page is a plugin panel.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct InspectorSection {
    /// The [`kind`](crate::command::kind) of thing whose page has
    /// the tab. Only `agent` has tabs.
    pub target: String,
    /// What the tab says.
    pub title: String,
    /// The id in the dock of the panel that fills the page, whose
    /// icon the tab takes.
    pub panel: String,
}
