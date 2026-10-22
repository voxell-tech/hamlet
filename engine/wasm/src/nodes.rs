//! The nodes of a plugin's retained panels, as the plugin makes,
//! sets, orders and drops them in a call: kept as changes to make to
//! the engine's widgets once the call ends, each held to the budgets
//! and to what its kind may hold and be set to.

use core::fmt;
use core::hash::{Hash, Hasher};
use std::collections::HashMap;
use std::hash::DefaultHasher;

use hamlet_engine_core::LogLevel;
use wasmtime::component::Resource;

use crate::instance::State;
use crate::instance::ui_bindings::hamlet::engine::ui::{
    Host, HostNode, Kind, MenuItem, Motion, Property, StatusItem,
};
use crate::picture::measure;
use crate::surfaces::SurfaceChange;

/// The most pixels a node is moved from where it is laid out, either
/// way, so the layout's sums stay finite.
const MOST_OFFSET: f32 = 100_000.0;

/// The tallest a frame is set, in pixels, so the layout's sums stay
/// finite.
const MOST_HEIGHT: f32 = 100_000.0;

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
}

/// A node the plugin holds: its kind, and how deep it sits under its
/// panel's root.
#[derive(Clone, Copy, Debug)]
struct Held {
    kind: Kind,
    depth: usize,
}

/// The picture an image node shows: a hash of the bytes it was
/// handed, and the bytes it takes decoded.
#[derive(Clone, Copy, Debug)]
struct Shown {
    hash: u64,
    memory: usize,
}

/// The nodes a plugin holds, and what it did to them in the call
/// under way.
#[derive(Default)]
pub(crate) struct Nodes {
    next: u32,
    held: HashMap<u32, Held>,
    pictures: HashMap<u32, Shown>,
    /// Bytes of the pictures shown, decoded.
    picture_memory: usize,
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
        }))
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

    /// Counts one change within `most`.
    fn spend(&mut self, most: usize) -> wasmtime::Result<()> {
        self.calls += 1;
        if self.spent >= most {
            return Err(wasmtime::Error::new(NodeError::Changes));
        }
        self.spent += 1;
        Ok(())
    }

    /// Fails unless `node` is a frame the plugin holds.
    pub(crate) fn frame(&self, node: u32) -> wasmtime::Result<()> {
        match self.kind(node)? {
            Kind::Frame => Ok(()),
            _ => Err(wasmtime::Error::new(NodeError::Property)),
        }
    }

    /// Lets go of the picture `node` shows.
    fn forget_picture(&mut self, node: u32) {
        if let Some(shown) = self.pictures.remove(&node) {
            self.picture_memory -= shown.memory;
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
            | Property::Grow(_)
            | Property::Height(_) => self == Self::Frame,
            Property::Placeholder(_) | Property::Lines(_) => {
                self == Self::Field
            }
            Property::On(_) => self == Self::Toggle,
            Property::Stick(_) => self == Self::Scroll,
            Property::Options(_) | Property::Selected(_) => {
                self == Self::Dropdown
            }
            Property::Picture(_)
            | Property::Extent(_)
            | Property::Fit(_)
            | Property::Rounded(_) => self == Self::Image,
        }
    }
}

impl Property {
    /// It with an opacity from 0 to 1, an offset within
    /// [`MOST_OFFSET`], a grow of 0 or more and a height from 0 to
    /// [`MOST_HEIGHT`], a value that is not finite taken as the
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
            Self::Height(height) => Self::Height(
                within(height, 0.0, MOST_HEIGHT).max(0.0),
            ),
            Self::Extent((width, height)) => Self::Extent((
                within(width, 0.0, MOST_OFFSET).max(0.0),
                within(height, 0.0, MOST_OFFSET).max(0.0),
            )),
            other => other,
        }
    }

    /// Bytes of what it holds that the host keeps.
    fn bytes(&self) -> usize {
        match self {
            Self::Text(text)
            | Self::Placeholder(text)
            | Self::Tip(text)
            | Self::Folder(text) => text.len(),
            Self::Picture(bytes) => bytes.len(),
            Self::Options(options) => options
                .iter()
                .map(|option| size_of::<String>() + option.len())
                .sum(),
            Self::Menu(menu) => menu
                .items
                .iter()
                .map(|item| {
                    size_of::<MenuItem>()
                        + item.text.len()
                        + item.group.len()
                })
                .sum(),
            _ => 0,
        }
    }
}

/// A plugin went past a budget of its nodes, or misused one.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NodeError {
    Changes,
    Nodes,
    Depth,
    /// A node made under one that holds none.
    Leaf,
    /// A property set on a node whose kind has none such.
    Property,
    /// A node it never held.
    Unknown,
    /// A picture of more bytes than one may have.
    PictureBytes,
    /// A picture with more pixels a side than one may have.
    PictureSide,
    /// Pictures shown past what they may take.
    PictureMemory,
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changes => {
                write!(f, "it changed more nodes than one call may")
            }
            Self::Nodes => {
                write!(f, "it held more nodes than it may")
            }
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
            Self::PictureBytes => {
                write!(
                    f,
                    "it gave a picture of more bytes than it may"
                )
            }
            Self::PictureSide => {
                write!(f, "it gave a picture larger than it may")
            }
            Self::PictureMemory => {
                write!(f, "it showed more pictures than it may")
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
        self.nodes.spend(self.caps.node_changes)?;
        self.keep_bytes(
            name.len()
                + item.icon.len()
                + item.text.len()
                + item.tip.len(),
        )?;
        self.surfaces.push(SurfaceChange::Status { name, item });
        Ok(())
    }

    fn show_overlay(
        &mut self,
        name: String,
        shown: bool,
    ) -> wasmtime::Result<()> {
        self.nodes.spend(self.caps.node_changes)?;
        self.keep_bytes(name.len())?;
        self.surfaces.push(SurfaceChange::Overlay { name, shown });
        Ok(())
    }

    fn set_section(
        &mut self,
        name: String,
        holds: u32,
    ) -> wasmtime::Result<()> {
        self.nodes.spend(self.caps.node_changes)?;
        self.keep_bytes(name.len())?;
        self.surfaces.push(SurfaceChange::Section { name, holds });
        Ok(())
    }

    fn open(&mut self, name: String) -> wasmtime::Result<()> {
        self.nodes.spend(self.caps.node_changes)?;
        self.keep_bytes(name.len())?;
        self.surfaces.push(SurfaceChange::Open(name));
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
        self.nodes.spend(self.caps.node_changes)?;
        if self.nodes.held.len() >= self.caps.nodes {
            return Err(wasmtime::Error::new(NodeError::Nodes));
        }
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
        self.nodes.spend(self.caps.node_changes)?;
        if !self.nodes.kind(node.rep())?.takes(&value) {
            return Err(wasmtime::Error::new(NodeError::Property));
        }
        self.keep_bytes(value.bytes())?;
        let value = match value {
            Property::Picture(bytes) => {
                match self.hold_picture(node.rep(), bytes)? {
                    Some(bytes) => Property::Picture(bytes),
                    None => return Ok(()),
                }
            }
            value => value,
        };
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
        self.nodes.spend(self.caps.node_changes)?;
        self.keep_bytes(children.len() * size_of::<u32>())?;
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
        self.nodes.spend(self.caps.node_changes)?;
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
        self.nodes.forget_picture(node.rep());
        if self.nodes.held.remove(&node.rep()).is_some() {
            self.nodes.changes.push(NodeChange::Drop(node.rep()));
        }
        Ok(())
    }
}

impl State {
    /// Holds `bytes`, the picture of the image `node`, to the budgets
    /// of pictures: the bytes to show, empty for none, or none when
    /// it shows them already. One that cannot be read shows none,
    /// and the plugin's log says why.
    fn hold_picture(
        &mut self,
        node: u32,
        bytes: Vec<u8>,
    ) -> wasmtime::Result<Option<Vec<u8>>> {
        if bytes.len() > self.caps.picture_bytes {
            return Err(wasmtime::Error::new(
                NodeError::PictureBytes,
            ));
        }
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        let hash = hasher.finish();
        if self
            .nodes
            .pictures
            .get(&node)
            .is_some_and(|shown| shown.hash == hash)
        {
            return Ok(None);
        }
        let (bytes, memory) = if bytes.is_empty() {
            (bytes, 0)
        } else {
            match measure(&bytes) {
                Ok((width, height)) => {
                    if width.max(height) > self.caps.picture_side {
                        return Err(wasmtime::Error::new(
                            NodeError::PictureSide,
                        ));
                    }
                    (bytes, width as usize * height as usize * 4)
                }
                Err(why) => {
                    self.log_line(
                        LogLevel::Warn,
                        format!("a picture cannot be read: {why}"),
                    );
                    (Vec::new(), 0)
                }
            }
        };
        let before = self
            .nodes
            .pictures
            .get(&node)
            .map_or(0, |shown| shown.memory);
        if self.nodes.picture_memory - before + memory
            > self.caps.picture_memory
        {
            return Err(wasmtime::Error::new(
                NodeError::PictureMemory,
            ));
        }
        self.nodes.picture_memory += memory;
        self.nodes.picture_memory -= before;
        self.nodes.pictures.insert(node, Shown { hash, memory });
        Ok(Some(bytes))
    }
}
