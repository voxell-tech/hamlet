//! What differs between the graph editors, which share the canvas,
//! the menus, the parameters, the files and the layout.

use bevy::app::App;
use bevy::math::Vec2;
use hamlet_blueprint::Registry;
use hamlet_ui::AnyView;
use hamlet_ui::widgets::icons::Glyph;

use super::Editor;
use crate::library::{Item, Kind};

/// A kind of graph the editor edits.
pub(crate) trait Flavor:
    Sized + Send + Sync + 'static
{
    /// The dock panel's id.
    const PANEL: &'static str;
    /// The dock panel's title.
    const TITLE: &'static str;
    /// The dock panel's icon.
    const ICON: Glyph;
    /// The folder in the window's folder the book is kept in, as
    /// `<FOLDER>/book.ron`, its assets beside it.
    const FOLDER: &'static str;
    /// What its books are in the library.
    const KIND: Kind;
    /// Books to start from, by name; the first opens at launch.
    const EXAMPLES: &'static [(&'static str, &'static str)];
    /// Whether its preview floats over the canvas as a card of its
    /// own, rather than filling a side of the panel.
    const PREVIEW_FLOATS: bool = false;
    /// The room a card leaves under its header for the flavor to
    /// fill, in sheet pixels; 0 for none.
    const ROOM: f32 = 0.0;
    /// What the flavor keeps of the last run, and any state of its
    /// own.
    type Got: Default + Send + Sync + 'static;
    /// The kinds of node its graphs use.
    fn registry() -> Registry;
    /// Works the open graph out again, filling `editor.got` and
    /// `editor.problems`.
    fn run(editor: &mut Editor<Self>);
    /// What the end of the toolbar says of the last run.
    fn status(editor: &Editor<Self>) -> String;
    /// The view beside the canvas.
    fn preview() -> AnyView;
    /// Tools of its own in the toolbar, after the file tools.
    fn tools() -> Vec<AnyView> {
        Vec::new()
    }
    /// Sections of its own at the end of the parameters panel.
    fn sections() -> Vec<AnyView> {
        Vec::new()
    }
    /// Its own plugins, added once by `graphs::plugin`.
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
    fn plugin(app: &mut App);
}
