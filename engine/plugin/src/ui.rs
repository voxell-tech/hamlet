//! Retained panels written with fynix, as the engine's own panels
//! are. [`Hamlet`] is fynix's backend: each node is a `node` of the
//! engine's, made when it is first needed, and each prop written is a
//! `set` on it. The engine builds its own widgets from them, in its
//! theme, and tells the plugin what the person does to them.
//!
//! ```ignore
//! fn mount(&mut self, _: &str, world: &mut World) -> AnyView {
//!     world.insert(Count(0));
//!     column((
//!         label(resource::<Count, _>(|count| count.0.to_string())),
//!         button("Add one")
//!             .on_click(|world| world.resource_mut::<Count>().0 += 1),
//!     ))
//!     .boxed()
//! }
//! ```

mod elements;
#[doc(hidden)]
pub mod glue;
mod nodes;
mod world;

use core::marker::PhantomData;
use core::time::Duration;

pub use fynix::{
    Condition, Motion, MotionTokens, Prop, ScopedExt, Signal, Styled,
    View, ViewExt, derived,
};

pub use self::elements::{
    Button, ButtonProps, EngineTransition, Frame, FrameProps, Label,
    LabelProps, OnClick, Stack, UiExt, button, column, heading,
    label, row,
};
pub use self::nodes::{Key, States};
pub use self::world::{World, resource};
pub use crate::bindings::hamlet::engine::ui::{
    ButtonStyle, Direction, Size, Space, Tone,
};

/// The engine, as a backend of fynix.
pub struct Hamlet;

/// A view of a plugin's panel.
pub type AnyView = fynix::AnyView<Hamlet, Theme>;
/// What a view is built in.
pub type Cx<'a> = fynix::Cx<'a, Hamlet, Theme>;

impl fynix::Backend for Hamlet {
    type World = World;
    type Node = Key;

    fn spawn(world: &mut World, parent: Option<Key>) -> Key {
        world.nodes.spawn(parent)
    }

    fn despawn(world: &mut World, node: Key) {
        world.despawn(node);
    }

    fn reorder(world: &mut World, parent: Key, children: &[Key]) {
        world.nodes.reorder(parent, children);
    }
}

/// What the plugin knows of the engine's theme: how long its
/// movements take. Colours and sizes are roles the engine fills in.
#[derive(Clone, Copy, Default, Debug)]
pub struct Theme {
    pub interact: Duration,
    pub expand: Duration,
}

impl MotionTokens for Theme {
    fn motion(&self, motion: Motion) -> fynix::Curve {
        let duration = match motion {
            Motion::Interact => self.interact,
            Motion::Expand => self.expand,
        };
        fynix::Curve {
            duration,
            ease: |t| 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

/// A node in the state `S`, as the engine tells it.
pub struct When<S>(PhantomData<fn() -> S>);

/// The pointer is over the node or one of its descendants.
pub struct Hovered;

/// A pointer button is down on the node.
pub struct Pressed;

/// A state the engine can tell of.
pub trait UiState {
    const STATES: States;
}

impl UiState for Hovered {
    const STATES: States = States::HOVER;
}

impl UiState for Pressed {
    const STATES: States = States::PRESS;
}

impl<S: UiState> Condition<Hamlet> for When<S> {
    fn holds(world: &World, node: Key) -> bool {
        world.nodes.holds(node, S::STATES)
    }

    fn watch(world: &mut World, node: Key) {
        world.nodes.watch(node, S::STATES);
    }
}

/// What any view can be given state rules with.
pub trait StateExt: Sized {
    /// This view, with the rules `rules` sets holding while its root
    /// node is in the state `S`.
    fn when<S: UiState, F>(
        self,
        rules: F,
    ) -> fynix::scoped::When<Self, F, When<S>> {
        self.when_in::<When<S>, F>(rules)
    }
}

impl<V> StateExt for V {}
