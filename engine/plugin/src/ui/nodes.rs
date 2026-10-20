//! The plugin's nodes as the engine holds them. A node is made in
//! the engine when something is first sent for it, so its kind is
//! known by then, and nodes are made in the order they were spawned,
//! so a parent is always made before its children.

use std::collections::{BTreeSet, HashMap};

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

    fn is_empty(self) -> bool {
        self.0 == 0
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

enum Handle {
    /// Not made yet, with its kind once an element said it.
    Waiting(Option<Kind>),
    Made(Node),
}

struct Entry {
    parent: Option<Key>,
    children: Vec<Key>,
    handle: Handle,
    /// The properties sent, one bit each by [`bit`].
    sent: u8,
    watch: States,
    states: States,
    /// The movement the engine animates its values over.
    motion: Option<Motion>,
}

/// Every node of the plugin's panels.
#[derive(Default)]
pub struct Nodes {
    next: Key,
    entries: HashMap<Key, Entry>,
    waiting: BTreeSet<Key>,
    /// Nodes despawned since fynix was last told.
    pub(crate) gone: Vec<Key>,
    /// Calls made into the engine.
    pub(crate) calls: u64,
}

impl Nodes {
    /// Takes `node`, made by the engine, as a node of the plugin's.
    pub fn adopt(&mut self, node: Node) -> Key {
        let key = self.key();
        self.entries.insert(key, entry(None, Handle::Made(node)));
        key
    }

    pub fn spawn(&mut self, parent: Option<Key>) -> Key {
        let key = self.key();
        self.entries
            .insert(key, entry(parent, Handle::Waiting(None)));
        if let Some(parent) =
            parent.and_then(|p| self.entries.get_mut(&p))
        {
            parent.children.push(key);
        }
        self.waiting.insert(key);
        key
    }

    /// The key the next node spawned will have.
    pub fn next_key(&self) -> Key {
        self.next
    }

    pub fn prepare(&mut self, key: Key, kind: Kind) {
        if let Some(Entry {
            handle: Handle::Waiting(waiting),
            ..
        }) = self.entries.get_mut(&key)
        {
            *waiting = Some(kind);
        }
    }

    /// Has the engine animate the values of the nodes from `first`
    /// on over `motion`.
    pub fn animate_from(&mut self, first: Key, motion: Motion) {
        for key in first..self.next {
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.motion = Some(motion);
            }
        }
    }

    /// Sends `value` for `key`, unless it is the first value and none
    /// was sent before.
    pub fn set(&mut self, key: Key, value: Property) {
        let bit = bit(&value);
        let Some(entry) = self.entries.get(&key) else {
            return;
        };
        if entry.sent & bit == 0 && is_first(&value) {
            return;
        }
        let motion = entry.motion;
        let Some(node) = self.made(key) else {
            return;
        };
        node.set(&value, motion);
        self.calls += 1;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.sent |= bit;
        }
    }

    pub fn reorder(&mut self, parent: Key, children: &[Key]) {
        if let Some(entry) = self.entries.get_mut(&parent) {
            entry.children = children.to_vec();
        }
        let last = children.iter().copied().chain([parent]).max();
        if let Some(last) = last {
            self.make_through(last);
        }
        let Some(Handle::Made(node)) =
            self.entries.get(&parent).map(|entry| &entry.handle)
        else {
            return;
        };
        let handles = children
            .iter()
            .filter_map(|child| {
                match &self.entries.get(child)?.handle {
                    Handle::Made(node) => Some(node),
                    Handle::Waiting(_) => None,
                }
            })
            .collect::<Vec<_>>();
        node.reorder(&handles);
        self.calls += 1;
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
            self.waiting.remove(&key);
            if let Some(entry) = self.entries.remove(&key) {
                if matches!(entry.handle, Handle::Made(_)) {
                    self.calls += 1;
                }
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
        if let Handle::Made(node) = &entry.handle {
            entry.watch.watch(node);
            self.calls += 1;
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

    /// Makes every node still waiting.
    pub fn flush(&mut self) {
        if let Some(&last) = self.waiting.last() {
            self.make_through(last);
        }
    }

    fn key(&mut self) -> Key {
        let key = self.next;
        self.next += 1;
        key
    }

    /// The handle of `key`, making it and every node spawned before
    /// it first.
    fn made(&mut self, key: Key) -> Option<&Node> {
        self.make_through(key);
        match &self.entries.get(&key)?.handle {
            Handle::Made(node) => Some(node),
            Handle::Waiting(_) => None,
        }
    }

    fn make_through(&mut self, last: Key) {
        while let Some(&key) = self.waiting.first() {
            if key > last {
                break;
            }
            self.waiting.pop_first();
            self.make(key);
        }
    }

    fn make(&mut self, key: Key) {
        let Some(entry) = self.entries.get(&key) else {
            return;
        };
        let &Handle::Waiting(kind) = &entry.handle else {
            return;
        };
        let watch = entry.watch;
        let Some(Handle::Made(parent)) = entry
            .parent
            .and_then(|parent| self.entries.get(&parent))
            .map(|parent| &parent.handle)
        else {
            crate::error("a node of a panel was built outside it");
            return;
        };
        let node =
            Node::new(kind.unwrap_or(Kind::Frame), parent, key);
        self.calls += 1;
        if !watch.is_empty() {
            watch.watch(&node);
            self.calls += 1;
        }
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.handle = Handle::Made(node);
        }
    }
}

fn entry(parent: Option<Key>, handle: Handle) -> Entry {
    Entry {
        parent,
        children: Vec::new(),
        handle,
        sent: 0,
        watch: States::default(),
        states: States::default(),
        motion: None,
    }
}

/// The bit of the property `value` sets, in the order the WIT names
/// them.
fn bit(value: &Property) -> u8 {
    let at = match value {
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

/// Whether `value` is what a node starts with.
fn is_first(value: &Property) -> bool {
    match value {
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
