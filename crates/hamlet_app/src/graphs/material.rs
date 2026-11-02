//! The material editor: material graphs edited with the same canvas,
//! menu and parameters as blueprints, compiled to a shader on every
//! change. The preview shades the material on a mesh with the
//! realm's camera and lights, and each card shows its node's output
//! on a small sphere.

mod preview;
mod swatch;

use bevy::app::App;
use hamlet_graph::{Error, GraphError, Registry};
use hamlet_graph_editor::library::Kind;
use hamlet_graph_editor::{Editor, Flavor, Nesting};
use hamlet_material::{Compiled, compile};
use hamlet_ui::AnyView;
use hamlet_ui::widgets::icons::Glyph;

/// Material graphs.
pub(crate) struct Material;

/// The open graph as last compiled.
#[derive(Default)]
pub(crate) struct Got {
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

impl Flavor for Material {
    const PANEL: &'static str = "materials";
    const TITLE: &'static str = "MATERIAL";
    const ICON: Glyph = Glyph::Sphere;
    const FOLDER: &'static str = "materials";
    const KIND: Kind = Kind::Material;
    const NESTING: Nesting = Nesting::Never;
    const EXAMPLES: &'static [(&'static str, &'static str)] = &[
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
    ];
    const ROOM: f32 = swatch::SIZE + 8.0;
    const PREVIEW_FLOATS: bool = true;

    type Got = Got;

    fn registry() -> Registry {
        hamlet_material::registry()
    }

    fn run(editor: &mut Editor<Self>) {
        let compiled = compile(&editor.sheet.graph);
        let old = editor.got.compiled().map(|old| old.code.clone());
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
            editor.got.version += 1;
        }
        // A graph that fails keeps the last shader that compiled.
        if compiled.is_ok() || editor.got.compiled.is_none() {
            editor.got.compiled = Some(compiled);
        }
    }

    fn status(editor: &Editor<Self>) -> String {
        match (&editor.got.compiled, editor.errors.first()) {
            (Some(Ok(_)), None) => "Compiled".to_owned(),
            (_, Some(error)) => format!(
                "{} did not compile: {error}",
                editor.errors.len()
            ),
            (Some(Err(error)), None) => error.to_string(),
            (None, None) => String::new(),
        }
    }

    fn preview() -> AnyView {
        preview::view()
    }

    fn plugin(app: &mut App) {
        app.add_plugins((preview::plugin, swatch::plugin));
    }
}

#[cfg(test)]
mod tests {
    use hamlet_graph_editor::library::{lone, parse_book};

    use super::{Flavor, Material};

    #[test]
    fn every_example_compiles() {
        for (name, text) in Material::EXAMPLES {
            let sheet = lone(parse_book(text).unwrap());
            let compiled = hamlet_material::compile(&sheet.graph)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(compiled.errors, [], "{name}");
        }
    }
}
