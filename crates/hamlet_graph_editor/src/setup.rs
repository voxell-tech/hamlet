//! What an editor is set up with: the [`EditorKey`] that tells its
//! state from another editor's, and the [`Setup`] of the graphs it
//! edits. What it shows is composed of views by whoever puts it in a
//! panel.

use hamlet_graph::{Graph, Registry};

use crate::library::Kind;

/// What tells one editor's state from another's: any type, as an
/// empty struct named for the graphs the editor edits.
pub trait EditorKey: Send + Sync + 'static {}

impl<T: Send + Sync + 'static> EditorKey for T {}

/// Whether the graphs of an editor may run each other as nodes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nesting {
    /// A node may run any other asset of the editor's kind.
    Runs,
    /// A node runs only a kind of the registry.
    Never,
}

/// The graphs an editor edits, and how it keeps them.
#[derive(Clone, Copy)]
pub struct Setup {
    /// The [`Kind`] of its graphs in the library.
    pub kind: Kind,
    /// The folder in the window's folder that remembers which asset
    /// is open and its unsaved changes.
    pub folder: &'static str,
    /// The kinds of node its graphs use.
    pub registry: fn() -> Registry,
    /// Brings a graph written for older kinds up to date as it is
    /// opened.
    pub upgrade: fn(&mut Graph),
    /// Whether its graphs run each other.
    pub nesting: Nesting,
    /// Whether the sheet shows the Outputs card, where the graph's
    /// outputs are wired.
    pub outputs: bool,
    /// The room a card leaves under its header for something drawn
    /// there, in sheet pixels; 0 for none.
    pub room: f32,
    /// Example graphs, by asset name and text; the first opens at
    /// launch.
    pub examples: &'static [(&'static str, &'static str)],
}

/// Leaves a graph as it is, for graphs that need no upgrade.
pub fn keep(_graph: &mut Graph) {}
