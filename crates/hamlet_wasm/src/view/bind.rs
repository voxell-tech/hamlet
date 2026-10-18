//! The props of a node, read from the tree as it stands.

use bevy_fynix::{Signal, resource};

use crate::tree::Key;
use crate::trees::PluginTrees;

/// A node of one panel, found by the keys from the root.
#[derive(Clone)]
pub(super) struct At {
    pub panel: String,
    pub path: Vec<Key>,
}

impl At {
    pub fn root(panel: &str) -> Self {
        Self {
            panel: panel.into(),
            path: Vec::new(),
        }
    }

    pub fn under(&self, key: &Key) -> Self {
        let mut path = self.path.clone();
        path.push(key.clone());
        Self {
            panel: self.panel.clone(),
            path,
        }
    }

    pub fn text(&self) -> Signal<String> {
        let at = self.clone();
        resource::<PluginTrees, _>(move |trees| {
            trees.text(&at.panel, &at.path)
        })
    }

    pub fn checked(&self) -> Signal<bool> {
        let at = self.clone();
        resource::<PluginTrees, _>(move |trees| {
            trees.checked(&at.panel, &at.path)
        })
    }

    pub fn keys(&self) -> Signal<Vec<Key>> {
        let at = self.clone();
        resource::<PluginTrees, _>(move |trees| {
            trees.keys(&at.panel, &at.path)
        })
    }
}
