//! Panels written with fynix over the plugin's own Bevy [`World`].
//! [`Hamlet`] is fynix's backend: each node is a `node` of the
//! engine's, made as it is spawned, and each prop written is a `set`
//! on it. Signals read the world's resources and components, and a
//! handler is given the whole world.
//!
//! ```ignore
//! fn hello() -> AnyView {
//!     column((
//!         label(resource::<Count, _>(|count| count.0.to_string())),
//!         button("Add one")
//!             .on_click(|world| world.resource_mut::<Count>().0 += 1),
//!     ))
//!     .boxed()
//! }
//! ```

mod elements;
mod nodes;
pub(crate) mod panels;

use core::any::TypeId;
use core::marker::PhantomData;
use core::time::Duration;
use std::collections::HashMap;

use bevy_app::App;
use bevy_ecs::change_detection::Tick;
use bevy_ecs::prelude::*;
pub use fynix::{
    Condition, Motion, MotionTokens, Prop, ScopedExt, Signal, Styled,
    View, ViewExt, derived,
};
use hamlet_engine_api::{DesktopRequest, encode};

use crate::bindings::exports::hamlet::engine::ui_guest::Action;
pub use crate::bindings::hamlet::engine::ui::{
    Align, ButtonStyle, Direction, Face, Fill, NodePointer,
    PointerMode, Size, Space, StatusItem, Tone,
};
pub use crate::ui::elements::{
    Button, ButtonProps, Divider, DividerProps, Dropdown,
    DropdownProps, Field, FieldProps, Frame, FrameProps, Icon,
    IconProps, Label, LabelProps, Leads, List, ListProps, MenuItem,
    Menued, OnInput, Pointed, Scroll, ScrollProps, Stack, Tipped,
    Toggle, ToggleProps, UiExt, button, column, description,
    description_rules, divider, dropdown, field, heading, icon,
    icon_button, label, list, menu_item, pressable, row, scroll,
    switch, toggle,
};
use crate::ui::nodes::Nodes;
pub use crate::ui::nodes::{Key, States};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Ui>();
}

/// The engine, as a backend of fynix.
pub struct Hamlet;

/// A view of a plugin's panel.
pub type AnyView = fynix::AnyView<Hamlet, Theme>;
/// What a view is built in.
pub type Cx<'a> = fynix::Cx<'a, Hamlet, Theme>;

impl fynix::Backend for Hamlet {
    type World = World;
    type Node = Key;

    const ANIMATES: bool = true;

    fn spawn(
        world: &mut World,
        parent: Option<Key>,
        kind: Option<TypeId>,
    ) -> Key {
        world
            .resource_mut::<Ui>()
            .nodes
            .spawn(parent, elements::kind(kind))
    }

    fn despawn(world: &mut World, node: Key) {
        world.resource_mut::<Ui>().despawn(node);
    }

    fn reorder(world: &mut World, parent: Key, children: &[Key]) {
        world.resource_mut::<Ui>().nodes.reorder(parent, children);
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

/// What runs, with the whole world, when the person acts on a node.
pub(crate) enum Handler {
    Click(Box<dyn Fn(&mut World) + Send + Sync>),
    Toggle(Box<dyn Fn(&mut World, bool) + Send + Sync>),
    Commit(Box<dyn Fn(&mut World, String) + Send + Sync>),
    Pick(Box<dyn Fn(&mut World, u32) + Send + Sync>),
    Submit(Box<dyn Fn(&mut World, String) + Send + Sync>),
    Top(Box<dyn Fn(&mut World) + Send + Sync>),
    Chose(Box<dyn Fn(&mut World, u32) + Send + Sync>),
    Folder(Box<dyn Fn(&mut World, String) + Send + Sync>),
}

/// The nodes of the plugin's panels and what runs on them.
#[derive(Resource, Default)]
pub(crate) struct Ui {
    pub(crate) nodes: Nodes,
    pub(crate) handlers: HashMap<Key, Vec<Handler>>,
    /// The address each node leads to, told of while the pointer is
    /// over it.
    pub(crate) addresses: HashMap<Key, String>,
    /// The node whose address was told of last.
    pointed: Option<Key>,
}

impl Ui {
    /// Takes `key` away, and what was under it.
    fn despawn(&mut self, key: Key) {
        for key in self.nodes.despawn(key) {
            self.handlers.remove(&key);
            self.addresses.remove(&key);
            if self.pointed == Some(key) {
                self.point(key, false);
            }
        }
    }

    /// Tells the desktop service of the address of `key` as the
    /// pointer comes onto it, and that there is none as it leaves.
    pub(crate) fn point(&mut self, key: Key, on: bool) {
        let hover =
            if on && let Some(address) = self.addresses.get(&key) {
                self.pointed = Some(key);
                Some(address.clone())
            } else if !on && self.pointed == Some(key) {
                self.pointed = None;
                None
            } else {
                return;
            };
        if let Ok(bytes) = encode(&DesktopRequest::Hover(hover)) {
            crate::bindings::hamlet::engine::host::desktop(&bytes);
        }
    }
}

/// Runs the handlers of `key` for `action`, those it has for it.
pub(crate) fn handle(world: &mut World, key: Key, action: &Action) {
    let Some(handlers) =
        world.resource_mut::<Ui>().handlers.remove(&key)
    else {
        return;
    };
    for handler in &handlers {
        handler.run(world, action);
    }
    let mut ui = world.resource_mut::<Ui>();
    // A node built again in the meantime has handlers of its own.
    if ui.nodes.has(key) {
        ui.handlers.entry(key).or_insert(handlers);
    }
}

impl Handler {
    fn run(&self, world: &mut World, action: &Action) {
        match (self, action) {
            (Handler::Click(run), Action::Click)
            | (Handler::Top(run), Action::ReachedTop) => run(world),
            (Handler::Toggle(run), Action::Toggled(on)) => {
                run(world, *on)
            }
            (Handler::Commit(run), Action::Committed(text))
            | (Handler::Submit(run), Action::Submitted(text))
            | (Handler::Folder(run), Action::Chosen(text)) => {
                run(world, text.clone())
            }
            (Handler::Pick(run), Action::Picked(at))
            | (Handler::Chose(run), Action::Chose(at)) => {
                run(world, *at)
            }
            _ => {}
        }
    }
}

/// The change tick a signal's check saw last.
#[derive(Default)]
struct Seen {
    tick: Option<Tick>,
    /// Whether a write could still land on `tick`.
    open: bool,
}

impl Seen {
    /// Whether `tick` differs from the one seen before, and takes it
    /// as seen. `None` is an absent source.
    fn fires(&mut self, world: &World, tick: Option<Tick>) -> bool {
        let fires = self.open || self.tick != tick;
        self.tick = tick;
        // A write made outside a system gets the tick seen, and would
        // go unnoticed.
        self.open = tick == Some(world.read_change_tick());
        fires
    }
}

/// A signal reading the resource `R`, read again when it changes.
/// `R` has to be in the world whenever the signal is read.
pub fn resource<R: Resource, T>(
    read: impl Fn(&R) -> T + Send + Sync + 'static,
) -> Signal<World, T> {
    let mut seen = Seen::default();
    Signal::new(
        move |world: &World| read(world.resource::<R>()),
        move |world: &World| {
            let tick = world
                .get_resource_change_ticks::<R>()
                .map(|ticks| ticks.changed);
            seen.fires(world, tick)
        },
    )
}

/// A signal reading the component `C` of `entity`, read again when it
/// changes, comes or goes. `read` is given `None` while it is absent.
pub fn component<C: Component, T>(
    entity: Entity,
    read: impl Fn(Option<&C>) -> T + Send + Sync + 'static,
) -> Signal<World, T> {
    let mut seen = Seen::default();
    Signal::new(
        move |world: &World| read(world.get::<C>(entity)),
        move |world: &World| {
            let tick = world
                .get_entity(entity)
                .ok()
                .and_then(|entity| entity.get_change_ticks::<C>())
                .map(|ticks| ticks.changed);
            seen.fires(world, tick)
        },
    )
}

/// Shows `item` in the plugin's status item `name`, declared with
/// [`HamletAppExt::add_status_item`](crate::HamletAppExt::add_status_item).
pub fn set_status(name: &str, item: &StatusItem) {
    crate::bindings::hamlet::engine::ui::set_status(name, item);
}

/// Shows the plugin's overlay `name` over the dock, or hides it.
pub fn show_overlay(name: &str, shown: bool) {
    crate::bindings::hamlet::engine::ui::show_overlay(name, shown);
}

/// Says how many things the plugin's panel section `name`, added
/// with
/// [`HamletAppExt::add_panel_section`](crate::HamletAppExt::add_panel_section),
/// holds. One that holds none is hidden.
pub fn set_section(name: &str, holds: u32) {
    crate::bindings::hamlet::engine::ui::set_section(name, holds);
}

/// Brings the plugin's panel `name` to the front, or shows its
/// settings page `name` in Settings. A full id, `plugin.id/name`,
/// brings the panel of that id to the front, of any plugin.
pub fn open(name: &str) {
    crate::bindings::hamlet::engine::ui::open(name);
}

/// A view of `build(&key)`, built again whenever the key changes to
/// another value.
pub fn keyed<K>(
    key: Signal<World, K>,
    build: impl Fn(&K) -> AnyView + Send + Sync + 'static,
) -> fynix::Keyed<Hamlet, Theme, K>
where
    K: PartialEq + Clone + Send + Sync + 'static,
{
    fynix::keyed(key, build)
}

/// One view of `build(&item)` per item, matched by `key` when the
/// list changes. A kept item is not built again.
pub fn each<I: 'static, K>(
    items: Signal<World, Vec<I>>,
    key: fn(&I) -> K,
    build: impl Fn(&I) -> AnyView + Send + Sync + 'static,
) -> fynix::Each<Hamlet, Theme, I, K>
where
    K: PartialEq + Clone + Send + Sync + 'static,
{
    fynix::each(items, key, build)
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
        world.resource::<Ui>().nodes.holds(node, S::STATES)
    }

    fn watch(world: &mut World, node: Key) {
        world.resource_mut::<Ui>().nodes.watch(node, S::STATES);
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
