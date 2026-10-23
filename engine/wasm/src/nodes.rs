//! The nodes of a plugin's retained panels, as the plugin makes,
//! sets, orders and drops them in a call: kept as changes to make to
//! the engine's widgets once the call ends, each held to what its
//! kind may hold and be set to, and to the guards' depth.

use core::fmt;
use std::collections::HashMap;

use bevy::math::Vec2;
use hamlet_engine_shell::{CardLook, CardPlace};
use wasmtime::component::Resource;

use crate::instance::State;
use crate::instance::ui_bindings::hamlet::engine::ui::{
    CardLook as WitCardLook, CardPlace as WitCardPlace, Host,
    HostNode, Kind, MenuItem, Motion, Property, StatusItem,
};
use crate::surfaces::SurfaceChange;

/// The most pixels a node is moved from where it is laid out, either
/// way, so the layout's sums stay finite.
const MOST_OFFSET: f32 = 100_000.0;

/// A node of a retained panel, as the plugin holds it.
pub struct UiNode;

/// A change to a plugin's widgets, by the number the host gave each
/// node.
#[derive(Clone, Debug)]
pub enum NodeChange {
    Make {
        node: u32,
        /// The plugin's name for it, in the inputs it is given.
        key: u32,
        kind: Kind,
        parent: u32,
    },
    Set {
        node: u32,
        value: Property,
        motion: Option<Motion>,
    },
    Order {
        node: u32,
        children: Vec<u32>,
    },
    Watch {
        node: u32,
        hover: bool,
        press: bool,
    },
    Drop(u32),
    /// A card floating over the window, its frame `node`.
    Card {
        node: u32,
        key: u32,
        look: CardLook,
        widest: f32,
    },
    /// A card shown or hidden; a place takes it down from where it
    /// hung.
    Place {
        node: u32,
        place: CardPlace,
    },
}

/// A node the plugin holds: its kind, how deep it sits under its
/// panel's root, and whether it is the frame of a card.
#[derive(Clone, Copy, Debug)]
struct Held {
    kind: Kind,
    depth: usize,
    card: bool,
}

/// The nodes a plugin holds, and what it did to them in the call
/// under way.
#[derive(Default)]
pub(crate) struct Nodes {
    next: u32,
    held: HashMap<u32, Held>,
    pub(crate) changes: Vec<NodeChange>,
    /// Makes, sets and orders in the call.
    pub(crate) spent: usize,
    /// Calls on nodes in the call, of every kind.
    pub(crate) calls: u64,
}

impl Nodes {
    /// A node of the host's own, handed to the plugin as the root of
    /// a panel: a frame.
    pub(crate) fn root(&mut self) -> Resource<UiNode> {
        Resource::new_own(self.number(Held {
            kind: Kind::Frame,
            depth: 0,
            card: false,
        }))
    }

    /// Fails unless `node` is the frame of a card the plugin holds.
    pub(crate) fn card(&self, node: u32) -> wasmtime::Result<()> {
        match self.held.get(&node) {
            Some(held) if held.card => Ok(()),
            Some(_) => Err(wasmtime::Error::new(NodeError::Property)),
            None => Err(wasmtime::Error::new(NodeError::Unknown)),
        }
    }

    fn number(&mut self, held: Held) -> u32 {
        // Numbers go round, past 0 and those still held, so a plugin
        // that makes nodes for ever never runs out of them.
        loop {
            self.next = self.next.wrapping_add(1);
            if self.next != 0 && !self.held.contains_key(&self.next) {
                break;
            }
        }
        self.held.insert(self.next, held);
        self.next
    }

    /// Counts one change.
    fn count(&mut self) {
        self.calls += 1;
        self.spent += 1;
    }

    /// Fails unless `node` is a frame the plugin holds.
    pub(crate) fn frame(&self, node: u32) -> wasmtime::Result<()> {
        match self.kind(node)? {
            Kind::Frame => Ok(()),
            _ => Err(wasmtime::Error::new(NodeError::Property)),
        }
    }

    fn kind(&self, node: u32) -> wasmtime::Result<Kind> {
        self.held
            .get(&node)
            .map(|held| held.kind)
            .ok_or_else(|| wasmtime::Error::new(NodeError::Unknown))
    }
}

impl Kind {
    /// Whether a node of this kind holds others.
    pub(crate) fn holds(self) -> bool {
        matches!(
            self,
            Self::Frame | Self::Button | Self::List | Self::Scroll
        )
    }

    /// Whether `value` is a property of this kind.
    fn takes(self, value: &Property) -> bool {
        match value {
            Property::Text(_) => matches!(
                self,
                Self::Label
                    | Self::Button
                    | Self::Icon
                    | Self::Toggle
                    | Self::Field
            ),
            Property::Tone(_) => {
                matches!(self, Self::Label | Self::Icon)
            }
            Property::Size(_)
            | Property::Bold(_)
            | Property::Face(_) => self == Self::Label,
            Property::Opacity(_)
            | Property::Offset(_)
            | Property::Tip(_)
            | Property::Menu(_)
            | Property::Pointer(_) => true,
            Property::Style(_) | Property::Folder(_) => {
                self == Self::Button
            }
            Property::Direction(_)
            | Property::Space(_)
            | Property::Align(_)
            | Property::Fill(_)
            | Property::Padding(_)
            | Property::Grow(_) => self == Self::Frame,
            Property::Placeholder(_) | Property::Lines(_) => {
                self == Self::Field
            }
            Property::On(_) => self == Self::Toggle,
            Property::Stick(_) => self == Self::Scroll,
            Property::Options(_) | Property::Selected(_) => {
                self == Self::Dropdown
            }
        }
    }
}

impl Property {
    /// It with an opacity from 0 to 1 and an offset within
    /// [`MOST_OFFSET`], a value that is not finite taken as the
    /// first.
    fn bounded(self) -> Self {
        let within = |value: f32, first: f32, most: f32| {
            if value.is_finite() {
                value.clamp(-most, most)
            } else {
                first
            }
        };
        match self {
            Self::Opacity(opacity) => {
                Self::Opacity(within(opacity, 1.0, 1.0).max(0.0))
            }
            Self::Offset((x, y)) => Self::Offset((
                within(x, 0.0, MOST_OFFSET),
                within(y, 0.0, MOST_OFFSET),
            )),
            Self::Grow(grow) => {
                Self::Grow(within(grow, 0.0, f32::MAX).max(0.0))
            }
            other => other,
        }
    }
}

/// A plugin misused its nodes, or nested one past the guards' depth.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NodeError {
    Depth,
    /// A node made under one that holds none.
    Leaf,
    /// A property set on a node whose kind has none such.
    Property,
    /// A node it never held.
    Unknown,
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Depth => {
                write!(f, "it nested nodes deeper than it may")
            }
            Self::Leaf => {
                write!(f, "it put a node under one that holds none")
            }
            Self::Property => {
                write!(f, "it set a property a node does not have")
            }
            Self::Unknown => {
                write!(f, "it named a node it never made")
            }
        }
    }
}

impl core::error::Error for NodeError {}

impl Host for State {
    fn set_status(
        &mut self,
        name: String,
        item: StatusItem,
    ) -> wasmtime::Result<()> {
        self.nodes.count();
        self.surfaces.push(SurfaceChange::Status { name, item });
        Ok(())
    }

    fn show_overlay(
        &mut self,
        name: String,
        shown: bool,
    ) -> wasmtime::Result<()> {
        self.nodes.count();
        self.surfaces.push(SurfaceChange::Overlay { name, shown });
        Ok(())
    }

    fn set_section(
        &mut self,
        name: String,
        holds: u32,
    ) -> wasmtime::Result<()> {
        self.nodes.count();
        self.surfaces.push(SurfaceChange::Section { name, holds });
        Ok(())
    }

    fn open(&mut self, name: String) -> wasmtime::Result<()> {
        self.nodes.count();
        self.surfaces.push(SurfaceChange::Open(name));
        Ok(())
    }

    fn open_menu(
        &mut self,
        key: u32,
        items: Vec<MenuItem>,
    ) -> wasmtime::Result<()> {
        self.nodes.count();
        self.surfaces.push(SurfaceChange::Menu { key, items });
        Ok(())
    }

    fn make_card(
        &mut self,
        key: u32,
        look: WitCardLook,
        widest: f32,
    ) -> wasmtime::Result<Resource<UiNode>> {
        self.nodes.count();
        let node = self.nodes.number(Held {
            kind: Kind::Frame,
            depth: 0,
            card: true,
        });
        let widest = if widest.is_finite() {
            widest.clamp(0.0, MOST_OFFSET)
        } else {
            0.0
        };
        self.nodes.changes.push(NodeChange::Card {
            node,
            key,
            look: match look {
                WitCardLook::Plain => CardLook::Plain,
                WitCardLook::Card => CardLook::Card,
                WitCardLook::Tag => CardLook::Tag,
                WitCardLook::Menu => CardLook::Menu,
            },
            widest,
        });
        Ok(Resource::new_own(node))
    }

    fn place_card(
        &mut self,
        card: Resource<UiNode>,
        place: WitCardPlace,
    ) -> wasmtime::Result<()> {
        self.nodes.count();
        self.nodes.card(card.rep())?;
        // A card placed here opens where the pointer is as the call
        // ends.
        let place = match place {
            WitCardPlace::Hidden => CardPlace::Hidden,
            WitCardPlace::Pointer => CardPlace::Pointer,
            WitCardPlace::Here => CardPlace::Here(Vec2::ZERO),
        };
        self.nodes.changes.push(NodeChange::Place {
            node: card.rep(),
            place,
        });
        Ok(())
    }
}

impl HostNode for State {
    fn new(
        &mut self,
        kind: Kind,
        parent: Resource<UiNode>,
        key: u32,
    ) -> wasmtime::Result<Resource<UiNode>> {
        self.nodes.count();
        let Some(&above) = self.nodes.held.get(&parent.rep()) else {
            return Err(wasmtime::Error::new(NodeError::Unknown));
        };
        if !above.kind.holds() {
            return Err(wasmtime::Error::new(NodeError::Leaf));
        }
        if above.depth >= self.caps.node_depth {
            return Err(wasmtime::Error::new(NodeError::Depth));
        }
        let node = self.nodes.number(Held {
            kind,
            depth: above.depth + 1,
            card: false,
        });
        self.nodes.changes.push(NodeChange::Make {
            node,
            key,
            kind,
            parent: parent.rep(),
        });
        Ok(Resource::new_own(node))
    }

    fn set(
        &mut self,
        node: Resource<UiNode>,
        value: Property,
        motion: Option<Motion>,
    ) -> wasmtime::Result<()> {
        self.nodes.count();
        if !self.nodes.kind(node.rep())?.takes(&value) {
            return Err(wasmtime::Error::new(NodeError::Property));
        }
        self.nodes.changes.push(NodeChange::Set {
            node: node.rep(),
            value: value.bounded(),
            motion,
        });
        Ok(())
    }

    fn reorder(
        &mut self,
        node: Resource<UiNode>,
        children: Vec<Resource<UiNode>>,
    ) -> wasmtime::Result<()> {
        self.nodes.count();
        self.nodes.changes.push(NodeChange::Order {
            node: node.rep(),
            children: children.iter().map(Resource::rep).collect(),
        });
        Ok(())
    }

    fn watch(
        &mut self,
        node: Resource<UiNode>,
        hover: bool,
        press: bool,
    ) -> wasmtime::Result<()> {
        self.nodes.count();
        self.nodes.changes.push(NodeChange::Watch {
            node: node.rep(),
            hover,
            press,
        });
        Ok(())
    }

    fn drop(
        &mut self,
        node: Resource<UiNode>,
    ) -> wasmtime::Result<()> {
        self.nodes.calls += 1;
        if self.nodes.held.remove(&node.rep()).is_some() {
            self.nodes.changes.push(NodeChange::Drop(node.rep()));
        }
        Ok(())
    }
}
