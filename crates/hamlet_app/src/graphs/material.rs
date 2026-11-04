//! The material editor: material graphs edited with the same canvas,
//! menu and parameters as blueprints, compiled to a shader on every
//! change. The preview shades the material on a mesh with the
//! realm's camera and lights, and each card shows its node's output
//! on a small sphere.

mod preview;
mod swatch;

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use bevy::ecs::world::World;
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::{ViewExt, derived};
use hamlet_graph::{Error, GraphError};
use hamlet_graph_editor::library::Kind;
use hamlet_graph_editor::{
    Editor, Nesting, Setup, editor, keep, parameters, preview_card,
    toolbar,
};
use hamlet_material::{Compiled, compile};
use hamlet_ui::AnyView;
use hamlet_ui::styles::{self, description};
use hamlet_ui::widgets::icons::Glyph;

use crate::dock::{PanelAppExt, flush_panel};
use crate::roll::moved;

/// Material graphs.
pub(crate) struct Material;

/// The icon of the material editor's panel and preview.
const ICON: Glyph = Glyph::Sphere;

/// Material graphs, and how the editor keeps them.
pub(crate) const SETUP: Setup = Setup {
    kind: Kind::Material,
    folder: "materials",
    registry: hamlet_material::registry,
    upgrade: keep,
    nesting: Nesting::Never,
    outputs: false,
    room: swatch::SIZE + 8.0,
    examples: &[
        ("plaster", include_str!("material/examples/plaster.ron")),
        (
            "stone_wall",
            include_str!("material/examples/stone_wall.ron"),
        ),
        ("wood", include_str!("material/examples/wood.ron")),
        (
            "emissive_runes",
            include_str!("material/examples/emissive_runes.ron"),
        ),
        ("water", include_str!("material/examples/water.ron")),
    ],
};

/// Adds the material editor in a panel of the dock, compiled again on
/// every change, its preview floating over the canvas.
pub(super) fn plugin(app: &mut App) {
    app.add_panel(
        "materials",
        DockWindowKind::new("MATERIAL", || flush_panel(screen()))
            .icon(ICON.image(styles::ICON)),
    )
    .init_resource::<Got>()
    .add_systems(Update, run)
    .add_plugins((
        hamlet_graph_editor::plugin::<Material>(SETUP),
        preview::plugin,
        swatch::plugin,
    ));
}

/// The editor's panel: the toolbar with whether the graph compiled,
/// and the parameters and the preview floating over the canvas.
fn screen() -> AnyView {
    let status = description(
        derived(|world: &World| {
            status(
                world.resource::<Editor<Material>>(),
                world.resource::<Got>(),
            )
        })
        .when(moved::<Got>()),
    );
    let toolbar = toolbar::bar(
        vec![toolbar::asset::<Material>()],
        vec![
            toolbar::file_note::<Material>(),
            status.boxed(),
            toolbar::history::<Material>(),
            toolbar::fit::<Material>(),
            toolbar::cards::<Material>(ICON),
        ],
    );
    editor::<Material>(
        toolbar,
        vec![
            parameters::<Material>(Vec::new()),
            preview_card::<Material>(ICON, preview::view()),
        ],
        None,
    )
    .boxed()
}

/// The open graph as last compiled.
#[derive(Resource, Default)]
pub(crate) struct Got {
    /// The editor's changes the last compile saw.
    ran: Option<u64>,
    /// The graph compiled, or why it could not be.
    pub compiled: Option<Result<Compiled, Error>>,
    /// Changes when the shader's code changes, not when only a value
    /// does.
    pub version: u64,
}

impl Got {
    /// The compiled graph, if it compiled.
    pub fn compiled(&self) -> Option<&Compiled> {
        self.compiled.as_ref().and_then(|got| got.as_ref().ok())
    }
}

/// Compiles the open graph again when it changed.
fn run(mut editor: ResMut<Editor<Material>>, mut got: ResMut<Got>) {
    if got.ran != Some(editor.changes) {
        evaluate(&mut editor, &mut got);
    }
}

/// What the end of the toolbar says of the last compile.
fn status(editor: &Editor<Material>, got: &Got) -> String {
    match (&got.compiled, editor.errors.first()) {
        (Some(Ok(_)), None) => "Compiled".to_owned(),
        (_, Some(error)) => {
            format!(
                "{} did not compile: {error}",
                editor.errors.len()
            )
        }
        (Some(Err(error)), None) => error.to_string(),
        (None, None) => String::new(),
    }
}

/// Compiles the open graph into `got`, and writes its errors to the
/// editor.
fn evaluate(editor: &mut Editor<Material>, got: &mut Got) {
    got.ran = Some(editor.changes);
    let compiled = compile(&editor.sheet.graph);
    let old = got.compiled().map(|old| old.code.clone());
    editor.errors = match &compiled {
        Ok(compiled) => compiled
            .errors
            .iter()
            .map(|error| GraphError {
                graph: editor.name.clone(),
                error: error.clone(),
            })
            .collect(),
        Err(error) => vec![GraphError {
            graph: editor.name.clone(),
            error: error.clone(),
        }],
    };
    let new = compiled.as_ref().ok().map(|new| new.code.clone());
    if new.is_some() && new != old {
        got.version += 1;
    }
    // A graph that fails keeps the last shader that compiled.
    if compiled.is_ok() || got.compiled.is_none() {
        got.compiled = Some(compiled);
    }
}

#[cfg(test)]
mod tests {
    use hamlet_graph_editor::library::{lone, parse_book};

    use super::SETUP;

    #[test]
    fn every_example_compiles() {
        for (name, text) in SETUP.examples {
            let sheet = lone(parse_book(text).unwrap());
            let compiled = hamlet_material::compile(&sheet.graph)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(compiled.errors, [], "{name}");
        }
    }
}
