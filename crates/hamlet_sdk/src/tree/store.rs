//! What each plugin panel shows, by the panel's id in the dock. The
//! panels' views read it; the host writes it when a plugin draws.

use std::collections::HashMap;

use bevy::prelude::*;

use super::node::{Key, Node, Tag};
use crate::dock::Panel;

/// What a plugin panel shows now.
#[derive(Clone, PartialEq, Debug)]
pub enum Drawn {
    Tree(Node),
    /// Its plugin stopped, for this reason.
    Stopped(String),
}

/// The state of a panel, without the tree. A view built for one
/// shape is built again when the shape changes.
#[derive(Clone, PartialEq, Debug)]
pub enum Shape {
    /// Nothing has been drawn yet.
    Waiting,
    /// A tree whose root is of this tag.
    Tree(Tag),
    Stopped(String),
}

/// Drops what a panel showed when the panel goes.
pub fn forget(
    remove: On<Remove, Panel>,
    panels: Query<&Panel>,
    mut trees: ResMut<PluginTrees>,
) {
    let Ok(panel) = panels.get(remove.entity) else {
        return;
    };
    if trees.contains(&panel.id) {
        trees.remove(&panel.id);
    }
}

/// The read-model of the plugin panels.
#[derive(Resource, Default, PartialEq, Debug)]
pub struct PluginTrees(HashMap<String, Drawn>);

impl PluginTrees {
    /// Whether `panel` shows `drawn` already.
    pub fn shows(&self, panel: &str, drawn: &Drawn) -> bool {
        self.0.get(panel) == Some(drawn)
    }

    pub fn set(&mut self, panel: &str, drawn: Drawn) {
        self.0.insert(panel.into(), drawn);
    }

    pub fn contains(&self, panel: &str) -> bool {
        self.0.contains_key(panel)
    }

    pub fn remove(&mut self, panel: &str) {
        self.0.remove(panel);
    }

    pub fn shape(&self, panel: &str) -> Shape {
        match self.0.get(panel) {
            None => Shape::Waiting,
            Some(Drawn::Tree(root)) => Shape::Tree(root.tag()),
            Some(Drawn::Stopped(why)) => Shape::Stopped(why.clone()),
        }
    }

    /// The node at `path`, the keys from the root down.
    pub fn node(&self, panel: &str, path: &[Key]) -> Option<&Node> {
        let Some(Drawn::Tree(root)) = self.0.get(panel) else {
            return None;
        };
        path.iter().try_fold(root, |node, key| node.child(key))
    }

    /// The words of the node at `path`, empty if it is gone.
    pub fn text(&self, panel: &str, path: &[Key]) -> String {
        self.node(panel, path)
            .map(|node| node.text().to_string())
            .unwrap_or_default()
    }

    pub fn checked(&self, panel: &str, path: &[Key]) -> bool {
        self.node(panel, path).is_some_and(Node::checked)
    }

    /// The keys of the children of the node at `path`.
    pub fn keys(&self, panel: &str, path: &[Key]) -> Vec<Key> {
        self.node(panel, path).map(Node::keys).unwrap_or_default()
    }
}
