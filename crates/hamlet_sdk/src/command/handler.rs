//! What a built-in command does when it is run.

use bevy::ecs::component::Component;
use bevy::ecs::system::Commands;

use super::Subject;

/// On a command of a built-in plugin: what it does.
#[derive(Component, Clone, Copy)]
#[component(immutable)]
pub struct Handler(pub fn(&mut Commands));

/// On a command of a built-in plugin: what it does to the
/// [`Subject`] it is run on.
#[derive(Component, Clone, Copy)]
#[component(immutable)]
pub struct HandlerWith(pub fn(&mut Commands, &Subject));

/// On a command of a built-in plugin: it brings the panel of this id
/// forward, opening it if it is closed.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct Opens(pub String);
