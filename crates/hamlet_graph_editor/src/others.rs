//! The editor's view of the library's other graphs: every asset of
//! the editor's kind but the open one, read again when the library or
//! the open asset changes.

use std::collections::BTreeMap;

use bevy::app::{App, Update};
use bevy::ecs::system::{Local, Res, ResMut};
use hamlet_graph::Graph;

use super::Editor;
use super::library::{Library, lone};
use super::setup::{EditorKey, Nesting, Setup};

pub(super) fn plugin<K: EditorKey>(app: &mut App) {
    app.add_systems(Update, refresh::<K>);
}

/// Gives the editor the other assets' graphs.
fn refresh<K: EditorKey>(
    library: Res<Library>,
    mut editor: ResMut<Editor<K>>,
    mut seen: Local<Option<(u64, String)>>,
) {
    if editor.setup.nesting == Nesting::Never {
        return;
    }
    if seen.as_ref().is_some_and(|(revision, name)| {
        *revision == library.revision && *name == editor.name
    }) {
        return;
    }
    let name = editor.name.clone();
    let others = graphs(&library, &editor.setup, &name);
    editor.set_others(others);
    *seen = Some((library.revision, name));
}

/// The graph of each asset of the kind `setup` edits but `open`, the
/// library's own before the examples of the same name. An asset that
/// cannot be read is left out.
fn graphs(
    library: &Library,
    setup: &Setup,
    open: &str,
) -> BTreeMap<String, Graph> {
    let mut graphs = BTreeMap::new();
    for item in library
        .items
        .iter()
        .filter(|item| item.kind == setup.kind && item.name != open)
    {
        let Ok(book) = item.read() else {
            continue;
        };
        let mut sheet = lone(book);
        (setup.upgrade)(&mut sheet.graph);
        graphs.entry(item.name.clone()).or_insert(sheet.graph);
    }
    graphs
}
