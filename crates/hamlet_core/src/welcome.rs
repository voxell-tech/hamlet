//! Opening the first-run setup again, from wherever the chief asks
//! for it.

use bevy::ecs::event::Event;

/// Opens the first-run setup: from its start, or at adding another
/// provider while keeping the current model.
#[derive(Event, Clone, Copy, Debug, PartialEq)]
pub enum OpenWelcome {
    Again,
    AddProvider,
}

/// Opens the card that brings in projects from other programs.
#[derive(Event, Clone, Copy, Debug, PartialEq)]
pub struct OpenProjects;
