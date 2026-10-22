//! The plugin's nodes as the engine holds them. A node is made in
//! the engine as fynix spawns it, of the kind of its element.

use std::collections::HashMap;

use crate::bindings::hamlet::engine::ui::{
    ButtonStyle, Direction, Kind, Motion, Node, Property, Size,
    Space, Tone,
};

/// A node of fynix's: the plugin's name for it, which the engine's
/// inputs carry.
pub type Key = u32;

/// States of a node the engine tells of, a bit each.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct States(u8);

impl States {
    pub const HOVER: Self = Self(1);
    pub const PRESS: Self = Self(2);

    fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    fn set(&mut self, other: Self, on: bool) {
        if on {
            self.0 |= other.0;
        } else {
            self.0 &= !other.0;
        }
    }

    /// Has the engine tell of these states of `node`.
    fn watch(self, node: &Node) {
        node.watch(
            self.contains(Self::HOVER),
            self.contains(Self::PRESS),
        );
    }
}

struct Entry {
    parent: Option<Key>,
    children: Vec<Key>,
    /// `None` for a node built outside a panel.
    node: Option<Node>,
    /// The properties sent, one bit each by [`Property::bit`].
    sent: u8,
    watch: States,
    states: States,
}

/// Every node of the plugin's panels.
#[derive(Default)]
pub struct Nodes {
    next: Key,
    entries: HashMap<Key, Entry>,
    /// Nodes despawned since fynix was last told.
    pub(crate) gone: Vec<Key>,
}

impl Nodes {
    /// Takes `node`, made by the engine, as a node of the plugin's.
    pub fn adopt(&mut self, node: Node) -> Key {
        let key = self.key();
        self.entries.insert(key, Entry::new(None, Some(node)));
        key
    }

    /// Makes a node of `kind`, last under `parent`.
    pub fn spawn(&mut self, parent: Option<Key>, kind: Kind) -> Key {
        let key = self.key();
        let made = parent
            .and_then(|parent| self.entries.get_mut(&parent))
            .and_then(|parent| {
                parent.children.push(key);
                parent.node.as_ref()
            })
            .map(|parent| Node::new(kind, parent, key));
        if made.is_none() {
            crate::error("a node of a panel was built outside it");
        }
        self.entries.insert(key, Entry::new(parent, made));
        key
    }

    /// Sends `value` for `key`, over `motion` where the engine moves
    /// it, unless it is the first value and none was sent before.
    pub fn set(
        &mut self,
        key: Key,
        value: Property,
        motion: Option<Motion>,
    ) {
        let bit = value.bit();
        let Some(entry) = self.entries.get_mut(&key) else {
            return;
        };
        if entry.sent & bit == 0 && value.is_first() {
            return;
        }
        if let Some(node) = &entry.node {
            node.set(&value, motion);
            entry.sent |= bit;
        }
    }

    pub fn reorder(&mut self, parent: Key, children: &[Key]) {
        let Some(entry) = self.entries.get_mut(&parent) else {
            return;
        };
        entry.children = children.to_vec();
        let Some(node) = &self.entries[&parent].node else {
            return;
        };
        let handles = children
            .iter()
            .filter_map(|child| {
                self.entries.get(child)?.node.as_ref()
            })
            .collect::<Vec<_>>();
        node.reorder(&handles);
    }

    /// Takes `key` away, and every node under it: their handles are
    /// dropped, which takes them away in the engine. The keys gone.
    pub fn despawn(&mut self, key: Key) -> Vec<Key> {
        if let Some(parent) = self
            .entries
            .get(&key)
            .and_then(|entry| entry.parent)
            .and_then(|parent| self.entries.get_mut(&parent))
        {
            parent.children.retain(|&child| child != key);
        }
        let mut gone = vec![key];
        let mut at = 0;
        while let Some(&key) = gone.get(at) {
            at += 1;
            if let Some(entry) = self.entries.remove(&key) {
                gone.extend(entry.children);
            }
        }
        self.gone.extend_from_slice(&gone);
        gone
    }

    /// Has the engine tell when `key` enters or leaves `states`.
    pub fn watch(&mut self, key: Key, states: States) {
        let Some(entry) = self.entries.get_mut(&key) else {
            return;
        };
        if entry.watch.contains(states) {
            return;
        }
        entry.watch.set(states, true);
        if let Some(node) = &entry.node {
            entry.watch.watch(node);
        }
    }

    pub fn holds(&self, key: Key, states: States) -> bool {
        self.entries
            .get(&key)
            .is_some_and(|entry| entry.states.contains(states))
    }

    /// Puts `key` in or out of `states`, as the engine told.
    pub fn enter(&mut self, key: Key, states: States, on: bool) {
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.states.set(states, on);
        }
    }

    fn key(&mut self) -> Key {
        let key = self.next;
        self.next += 1;
        key
    }
}

impl Entry {
    fn new(parent: Option<Key>, node: Option<Node>) -> Self {
        Self {
            parent,
            children: Vec::new(),
            node,
            sent: 0,
            watch: States::default(),
            states: States::default(),
        }
    }
}

impl Property {
    /// The bit of the property this sets, in the order the WIT names
    /// them.
    fn bit(&self) -> u8 {
        let at = match self {
            Property::Text(_) => 0,
            Property::Tone(_) => 1,
            Property::Size(_) => 2,
            Property::Bold(_) => 3,
            Property::Opacity(_) => 4,
            Property::Style(_) => 5,
            Property::Direction(_) => 6,
            Property::Space(_) => 7,
        };
        1 << at
    }

    /// Whether this is what a node starts with.
    fn is_first(&self) -> bool {
        match self {
            Property::Text(text) => text.is_empty(),
            Property::Tone(tone) => *tone == Tone::Body,
            Property::Size(size) => *size == Size::Body,
            Property::Bold(bold) => !bold,
            Property::Opacity(opacity) => *opacity == 1.0,
            Property::Style(style) => *style == ButtonStyle::Plain,
            Property::Direction(direction) => {
                *direction == Direction::Column
            }
            Property::Space(space) => *space == Space::Gap,
        }
    }
}
