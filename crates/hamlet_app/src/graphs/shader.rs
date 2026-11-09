//! The shader editor: shader graphs edited with the same canvas, menu
//! and parameters as blueprints, compiled to WGSL on every change.
//! The preview shades a mesh with the shader under the realm's camera
//! and lights, and each card shows its node's output on a small
//! sphere.

mod preview;
pub(crate) mod sample;
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
use hamlet_shader::{Compiled, compile};
use hamlet_ui::AnyView;
use hamlet_ui::styles::{self, description};
use hamlet_ui::widgets::icons::Glyph;

use crate::dock::{PanelAppExt, flush_panel};
use crate::roll::moved;

/// Shader graphs.
pub(crate) struct ShaderGraph;

/// The icon of the shader editor's panel and preview.
const ICON: Glyph = Glyph::Sphere;

/// Shader graphs, and how the editor keeps them.
pub(crate) const SETUP: Setup = Setup {
    kind: Kind::Shader,
    folder: "shaders",
    registry: hamlet_shader::registry,
    upgrade: keep,
    nesting: Nesting::Never,
    outputs: false,
    room: swatch::SIZE + 8.0,
    examples: || EXAMPLES,
};

const EXAMPLES: &[(&str, &str)] = &[
    ("plaster", include_str!("shader/examples/plaster.ron")),
    ("stone_wall", include_str!("shader/examples/stone_wall.ron")),
    ("wood", include_str!("shader/examples/wood.ron")),
    (
        "emissive_runes",
        include_str!("shader/examples/emissive_runes.ron"),
    ),
    ("water", include_str!("shader/examples/water.ron")),
];

/// Adds the shader editor in a panel of the dock, compiled again on
/// every change, its preview floating over the canvas.
pub(super) fn plugin(app: &mut App) {
    app.add_panel(
        "shaders",
        DockWindowKind::new("SHADER", || flush_panel(screen()))
            .icon(ICON.image(styles::ICON)),
    )
    .init_resource::<Got>()
    .add_systems(Update, run)
    .add_plugins((
        hamlet_graph_editor::plugin::<ShaderGraph>(SETUP),
        preview::plugin,
        sample::plugin,
        swatch::plugin,
    ));
}

/// The editor's panel: the toolbar with whether the graph compiled,
/// and the parameters and the preview floating over the canvas.
fn screen() -> AnyView {
    let status = description(
        derived(|world: &World| {
            status(
                world.resource::<Editor<ShaderGraph>>(),
                world.resource::<Got>(),
            )
        })
        .when(moved::<Got>()),
    );
    let toolbar = toolbar::bar(
        vec![toolbar::asset::<ShaderGraph>()],
        vec![
            toolbar::file_note::<ShaderGraph>(),
            status.boxed(),
            toolbar::history::<ShaderGraph>(),
            toolbar::fit::<ShaderGraph>(),
            toolbar::cards::<ShaderGraph>(ICON),
        ],
    );
    editor::<ShaderGraph>(
        toolbar,
        vec![
            parameters::<ShaderGraph>(Vec::new()),
            preview_card::<ShaderGraph>(ICON, preview::view()),
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
fn run(
    mut editor: ResMut<Editor<ShaderGraph>>,
    mut got: ResMut<Got>,
) {
    if got.ran != Some(editor.changes) {
        evaluate(&mut editor, &mut got);
    }
}

/// What the end of the toolbar says of the last compile.
fn status(editor: &Editor<ShaderGraph>, got: &Got) -> String {
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
fn evaluate(editor: &mut Editor<ShaderGraph>, got: &mut Got) {
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

    use super::EXAMPLES;

    #[test]
    fn every_example_compiles() {
        for (name, text) in EXAMPLES {
            let sheet = lone(parse_book(text).unwrap());
            let compiled = hamlet_shader::compile(&sheet.graph)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(compiled.errors, [], "{name}");
        }
    }
}
