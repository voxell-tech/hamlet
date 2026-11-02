//! The editor's view of the library's other graphs: every asset of
//! the flavor's kind but the open one, read again when the library or
//! the open asset changes.

use std::collections::BTreeMap;

use bevy::app::{App, Update};
use bevy::ecs::system::{Local, Res, ResMut};
use hamlet_graph::Graph;

use super::Editor;
use super::flavor::{Flavor, Nesting};
use super::library::{Library, lone};

pub(super) fn plugin<F: Flavor>(app: &mut App) {
    app.add_systems(Update, refresh::<F>);
}

/// Gives the editor the other assets' graphs.
fn refresh<F: Flavor>(
    library: Res<Library>,
    mut editor: ResMut<Editor<F>>,
    mut seen: Local<Option<(u64, String)>>,
) {
    if F::NESTING == Nesting::Never {
        return;
    }
    if seen.as_ref().is_some_and(|(revision, name)| {
        *revision == library.revision && *name == editor.name
    }) {
        return;
    }
    let name = editor.name.clone();
    editor.set_others(graphs::<F>(&library, &name));
    *seen = Some((library.revision, name));
}

/// The graph of each asset of the flavor's kind but `open`, the
/// library's own before the examples of the same name. An asset that
/// cannot be read is left out.
fn graphs<F: Flavor>(
    library: &Library,
    open: &str,
) -> BTreeMap<String, Graph> {
    let mut graphs = BTreeMap::new();
    for item in library
        .items
        .iter()
        .filter(|item| item.kind == F::KIND && item.name != open)
    {
        let Ok(book) = item.read() else {
            continue;
        };
        let mut sheet = lone(book);
        F::upgrade(&mut sheet.graph);
        graphs.entry(item.name.clone()).or_insert(sheet.graph);
    }
    graphs
}
