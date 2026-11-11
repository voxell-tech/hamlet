//! The graph editors in the window: a blueprint and a shader editor
//! of `hamlet_graph_editor`, each in a panel of the dock.

pub(crate) mod assets;
mod blueprint;
mod gizmo;
mod handles;
mod overlay;
pub(crate) mod prefab;
mod preview;
pub(crate) mod shader;
mod thumbnails;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use hamlet_graph_editor::canvas::Dragging;
use hamlet_graph_editor::files;
use hamlet_graph_editor::library::{Item, Kind};

pub(crate) use self::blueprint::Blueprint;
pub(crate) use self::shader::ShaderGraph;
use crate::cursor::Faint;

/// Adds the blueprint and the shader editors, each with its dock
/// panel.
pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        blueprint::plugin,
        shader::plugin,
        prefab::plugin,
    ));
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

/// Opens the library's `item` in the editor of its kind, to be saved
/// back to its file. A mesh or a texture is not opened; it is dragged
/// onto a canvas. A material has no graph: the ASSETS panel changes
/// it.
pub(crate) fn open_item(
    world: &mut World,
    item: &Item,
) -> Result<(), OpenRefusal> {
    let opened = match item.kind {
        Kind::Blueprint => files::open_item::<Blueprint>(world, item),
        Kind::Shader => files::open_item::<ShaderGraph>(world, item),
        Kind::Prefab => prefab::open(world, &item.name),
        Kind::Mesh | Kind::Texture => {
            return Err(OpenRefusal::Dragged {
                name: item.name.clone(),
            });
        }
        Kind::Material => {
            return Err(OpenRefusal::Changed {
                name: item.name.clone(),
            });
        }
    };
    Ok(opened?)
}

/// Why a library item does not open in an editor.
#[derive(thiserror::Error, Debug)]
pub(crate) enum OpenRefusal {
    #[error("Drag {name} onto a canvas to use it")]
    Dragged { name: String },
    #[error("{name} has no graph: change it in the ASSETS panel")]
    Changed { name: String },
    #[error("{0:#}")]
    Unreadable(#[from] anyhow::Error),
}
