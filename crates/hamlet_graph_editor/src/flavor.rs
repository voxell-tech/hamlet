//! The [`Flavor`] of a graph editor: the parts that differ between
//! editors sharing the canvas, the menus, the parameters, the files
//! and the layout.

use bevy::app::App;
use bevy::math::Vec2;
use hamlet_graph::{Graph, Registry};
use hamlet_ui::AnyView;
use hamlet_ui::widgets::icons::Glyph;

use super::Editor;
use crate::library::{Item, Kind};

/// A kind of graph the editor edits.
pub trait Flavor: Sized + Send + Sync + 'static {
    /// The dock panel's id.
    const PANEL: &'static str;
    /// The dock panel's title.
    const TITLE: &'static str;
    /// The dock panel's icon.
    const ICON: Glyph;
    /// The folder in the window's folder the book is kept in, as
    /// `<FOLDER>/book.ron`, its assets beside it.
    const FOLDER: &'static str;
    /// The [`Kind`] of its books in the library.
    const KIND: Kind;
    /// Books to start from, by name; the first opens at launch.
    const EXAMPLES: &'static [(&'static str, &'static str)];
    /// Whether its preview floats over the canvas as a card of its
    /// own, rather than filling a side of the panel.
    const PREVIEW_FLOATS: bool = false;
    /// The room a card leaves under its header for the flavor to
    /// fill, in sheet pixels; 0 for none.
    const ROOM: f32 = 0.0;
    /// The result of the last run, and any state of the flavor's
    /// own.
    type Got: Default + Send + Sync + 'static;
    /// The kinds of node its graphs use.
    fn registry() -> Registry;
    /// Brings `graph`, written for older kinds, up to date as its
    /// book is opened.
    fn upgrade(_graph: &mut Graph) {}
    /// Works the open graph out again, filling `editor.got` and
    /// `editor.errors`.
    fn run(editor: &mut Editor<Self>);
    /// The text at the end of the toolbar about the last run.
    fn status(editor: &Editor<Self>) -> String;
    /// The view beside the canvas.
    fn preview() -> AnyView;
    /// Tools of its own in the toolbar, after the file tools.
    fn tools() -> Vec<AnyView> {
        Vec::new()
    }
    /// Tabs of its own in the parameters card, after the parameters:
    /// each an icon, a name and a builder of its body, which runs
    /// again when the graph's shape changes.
    fn tabs() -> Vec<(Glyph, &'static str, fn() -> AnyView)> {
        Vec::new()
    }
    /// Adds what the library's `item` is to the open graph, its
    /// card at `at`, when it is dropped on the canvas; false for an
    /// asset it takes nothing of.
    fn dropped(
        _editor: &mut Editor<Self>,
        _item: &Item,
        _at: Vec2,
    ) -> bool {
        false
    }
    /// Its own plugins, added once by [`plugin`](crate::plugin).
    fn plugin(app: &mut App);
}
