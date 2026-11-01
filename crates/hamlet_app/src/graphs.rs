//! The graph editors in the window: the blueprint and the material
//! flavors of `hamlet_graph_editor`, each in a panel of the dock.

mod assets;
mod blueprint;
mod gizmo;
mod handles;
mod material;
mod overlay;
mod preview;
mod thumbnails;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy_fynix::dock::DockWindowKind;
use hamlet_graph_editor::canvas::Dragging;
use hamlet_graph_editor::files::Files;
use hamlet_graph_editor::library::{Item, Kind, Source};
use hamlet_graph_editor::{Editor, Flavor, example, screen};
use hamlet_ui::styles::ICON;

pub(crate) use self::assets::add_mesh;
pub(crate) use self::blueprint::Blueprint;
pub(crate) use self::material::Material;
use crate::cursor::Faint;
use crate::dock::{PanelAppExt, flush_panel};

/// Adds the editor for `F` with its dock panel.
pub(crate) fn plugin<F: Flavor>(app: &mut App) {
    app.add_panel(
        F::PANEL,
        DockWindowKind::new(F::TITLE, || flush_panel(screen::<F>()))
            .icon(F::ICON.image(ICON)),
    )
    .add_plugins(hamlet_graph_editor::plugin::<F>);
}

/// Draws the cursor faint while a wire is dragged, so what it is
/// dragged over shows through.
pub(crate) fn faint(app: &mut App) {
    app.add_systems(Update, fade);
}

fn fade(dragging: Res<Dragging>, mut faint: ResMut<Faint>) {
    if dragging.is_changed() {
        faint.0 = dragging.0;
    }
}

/// Opens the library's `item` in the editor of its kind: a book in
/// the blueprint or material editor, saved back to its file. A mesh
/// or a texture is not opened; it is dragged onto a canvas.
pub(crate) fn open_item(
    world: &mut World,
    item: &Item,
) -> Result<(), OpenRefusal> {
    match item.kind {
        Kind::Blueprint => open_book::<Blueprint>(world, item),
        Kind::Material => open_book::<Material>(world, item),
        Kind::Mesh | Kind::Texture => Err(OpenRefusal::Dragged {
            name: item.name.clone(),
        }),
    }
}

/// Why a library item does not open in an editor.
#[derive(thiserror::Error, Debug)]
pub(crate) enum OpenRefusal {
    #[error("Drag {name} onto a canvas to use it")]
    Dragged { name: String },
    #[error("{name} is not a book")]
    NotABook { name: String },
}

fn open_book<F: Flavor>(
    world: &mut World,
    item: &Item,
) -> Result<(), OpenRefusal> {
    let (book, path) = match &item.source {
        Source::Example(text) => (example(text), None),
        Source::File(path) => (
            std::fs::read_to_string(path)
                .ok()
                .and_then(|text| example(&text)),
            Some(path.clone()),
        ),
        Source::BuiltIn => (None, None),
    };
    let book = book.ok_or_else(|| OpenRefusal::NotABook {
        name: item.name.clone(),
    })?;
    world.resource_mut::<Editor<F>>().load(book);
    world.resource_mut::<Files<F>>().path = path;
    hamlet_graph_editor::canvas::fit::<F>(world);
    Ok(())
}
