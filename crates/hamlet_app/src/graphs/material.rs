//! The material editor: material graphs edited with the same canvas,
//! menu and parameters as blueprints, compiled to a shader on every
//! change. The preview shades the material on a mesh with the
//! realm's camera and lights, and each card shows its node's output
//! on a small sphere.

mod preview;
mod swatch;

use bevy::app::App;
use hamlet_blueprint::{GraphProblem, Registry};
use hamlet_material::{Compiled, compile};
use hamlet_ui::AnyView;
use hamlet_ui::widgets::icons::Glyph;

use super::Editor;
use super::flavor::Flavor;

/// Material graphs.
pub(crate) struct Material;

/// The open graph as last compiled.
#[derive(Default)]
pub(crate) struct Got {
    /// The graph compiled, or why it could not be.
    pub compiled: Option<Result<Compiled, String>>,
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
    const ICON: Glyph = Glyph::Drop;
    const FOLDER: &'static str = "materials";
    const EXAMPLES: &'static [(&'static str, &'static str)] = &[
        ("Plaster", include_str!("material/examples/plaster.ron")),
        (
            "Stone wall",
            include_str!("material/examples/stone_wall.ron"),
        ),
        ("Wood", include_str!("material/examples/wood.ron")),
        (
            "Emissive runes",
            include_str!("material/examples/emissive_runes.ron"),
        ),
        ("Water", include_str!("material/examples/water.ron")),
    ];
    const ROOM: f32 = swatch::SIZE + 8.0;

    type Got = Got;

    fn registry() -> Registry {
        hamlet_material::registry()
    }

    fn run(editor: &mut Editor<Self>) {
        let compiled = compile(&editor.sheet.graph);
        let old = editor.got.compiled().map(|old| old.code.clone());
        editor.problems = match &compiled {
            Ok(compiled) => compiled
                .problems
                .iter()
                .map(|problem| GraphProblem {
                    graph: editor.name.clone(),
                    problem: problem.clone(),
                })
                .collect(),
            Err(problem) => vec![GraphProblem {
                graph: editor.name.clone(),
                problem: problem.clone(),
            }],
        };
        let compiled =
            compiled.map_err(|problem| problem.to_string());
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
        match (&editor.got.compiled, editor.problems.first()) {
            (Some(Ok(_)), None) => "Compiled".to_owned(),
            (_, Some(problem)) => format!(
                "{} did not compile: {problem}",
                editor.problems.len()
            ),
            (Some(Err(problem)), None) => problem.clone(),
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
    use hamlet_blueprint::Book;

    use super::{Flavor, Material};

    #[test]
    fn every_example_compiles() {
        for (name, text) in Material::EXAMPLES {
            let book = Book::from_ron(text).unwrap();
            for (sheet_name, sheet) in &book.sheets {
                let compiled = hamlet_material::compile(&sheet.graph)
                    .unwrap_or_else(|problem| {
                        panic!("{name}, {sheet_name}: {problem}")
                    });
                assert_eq!(compiled.problems, [], "{name}");
            }
        }
    }
}
