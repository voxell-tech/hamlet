//! The marks a view puts on a node for the feel of the panels
//! (docs/ui-warmth.md) to read.

use bevy::ecs::component::Component;

/// The meaning of a press on a button, which sets its light, its
/// sound and its motion.
#[derive(Component, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mood {
    /// A tab, a menu item or the like: it gives under a press and
    /// nothing more.
    #[default]
    Quiet,
    /// An ordinary lettered button.
    Plain,
    /// Lettering with no plate, such as a way back: lit as a plain
    /// button is, with the light about its words and its mark alone.
    Ink,
    /// The action a screen is for: a lamp that burns low at rest.
    Good,
    /// Something refused, stopped or thrown away.
    Bad,
    /// A text field, which is lit while it is typed in.
    Field,
}

/// A panel that light falls into.
#[derive(Component, Clone, Copy, Default)]
pub struct Lit {
    /// A lamp is lit over it while something waits for the chief.
    pub alarm: bool,
    /// It flashes when the agent it shows changes.
    pub flash: bool,
}
