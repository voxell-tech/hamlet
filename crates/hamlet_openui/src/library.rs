//! The library of components, checked once.

use bevy::prelude::*;
use openui_lang::{ComponentDef, Library};

use crate::components;

/// The library every program is checked against.
#[derive(Resource, Clone, Debug)]
pub struct PanelLibrary(pub Library);

impl Default for PanelLibrary {
    fn default() -> Self {
        let mut library = Library::builder().root(components::ROOT);
        for component in components::all() {
            library = library.component(component);
        }
        Self(library.build().expect("the components are a library"))
    }
}

impl PanelLibrary {
    /// The components, in the order a program is told about them.
    pub fn components(&self) -> &[ComponentDef] {
        self.0.components()
    }

    /// The signature of each component, as a model is shown it.
    pub fn signatures(&self) -> Vec<String> {
        self.components()
            .iter()
            .map(ComponentDef::signature)
            .collect()
    }
}
