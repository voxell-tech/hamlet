//! The blueprint editor: graphs that put objects down, run again on
//! every change and drawn in the preview beside the canvas.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::Context;
use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy_fynix::ViewExt;
use hamlet_blueprint::{
    Bundle, GraphOutput, NodeId, Object, Port, Registry, Seed, Seeds,
    Shape, Value, ValueType, evaluate_partly,
};
use hamlet_client::ViewFolder;
use hamlet_desktop::file::{ChooseFile, FileChosen, FileMode};
use hamlet_ui::AnyView;
use hamlet_ui::widgets::icons::Glyph;

use super::assets::Meshes;
use super::files::{self, Files};
use super::flavor::Flavor;
use super::{Editor, assets, gizmo, overlay, preview, tool};

/// Blueprint graphs.
pub(crate) struct Blueprint;

/// What the last run of a blueprint graph gave.
pub(crate) struct Got {
    /// The seeds the random values are picked from.
    pub seeds: Seeds,
    /// What the open graph put down, or why it could not run at all.
    pub outcome: Result<Vec<Object>, String>,
    /// What each selected node gave in the last run, each output by
    /// name, for the preview to show.
    pub watched: BTreeMap<NodeId, Vec<(String, Value)>>,
    /// The nodes the last run watched.
    watching: BTreeSet<NodeId>,
}

impl Default for Got {
    fn default() -> Self {
        Self {
            seeds: Seeds {
                realm: Seed(1),
                hamlet: Seed(2),
                item: Seed(3),
            },
            outcome: Ok(Vec::new()),
            watched: BTreeMap::new(),
            watching: BTreeSet::new(),
        }
    }
}

impl Flavor for Blueprint {
    const PANEL: &'static str = "graphs";
    const TITLE: &'static str = "BLUEPRINT";
    const ICON: Glyph = Glyph::Signpost;
    const FOLDER: &'static str = "graphs";
    /// A sheet alone, or a book of sheets. The small ones first, each
    /// one idea; the house puts several together.
    const EXAMPLES: &'static [(&'static str, &'static str)] = &[
        ("Fence", include_str!("examples/fence.ron")),
        ("Stone wall", include_str!("examples/stone_wall.ron")),
        ("Window", include_str!("examples/window.ron")),
        ("Stone tower", include_str!("examples/stone_tower.ron")),
        ("Garden bed", include_str!("examples/garden_bed.ron")),
        (
            "Medieval house",
            include_str!("examples/medieval_house.ron"),
        ),
    ];

    type Got = Got;

    fn registry() -> Registry {
        Registry::builtin()
    }

    /// Runs the open graph with every output nothing is wired to
    /// shown too, so a graph is seen as it is built. Its inputs take
    /// their defaults.
    fn run(editor: &mut Editor<Self>) {
        let mut graph = editor.sheet.graph.clone();
        // Each selected node's outputs too, by names kept apart, so
        // the preview can show what it gives.
        let mut watch = BTreeMap::new();
        for node in &editor.selected {
            let Some(found) = editor.sheet.graph.node(*node) else {
                continue;
            };
            let Ok((_, outputs)) = hamlet_blueprint::ports(
                &editor.registry,
                &editor.rules,
                found,
            ) else {
                continue;
            };
            for output in outputs {
                let name = format!("watch {node}.{}", output.name);
                watch.insert(
                    name.clone(),
                    (*node, output.name.clone()),
                );
                graph.outputs.push(GraphOutput {
                    name,
                    value_type: output.value_type,
                    from: Port {
                        node: *node,
                        name: output.name,
                    },
                });
            }
        }
        editor.got.watching = editor.selected.clone();
        editor.got.watched.clear();
        for node in &editor.sheet.graph.nodes {
            let Ok((_, outputs)) = hamlet_blueprint::ports(
                &editor.registry,
                &editor.rules,
                node,
            ) else {
                continue;
            };
            for output in outputs {
                let shown = output.value_type == ValueType::Objects;
                let wired = graph.wires.iter().any(|wire| {
                    wire.from.node == node.id
                        && wire.from.name == output.name
                });
                let declared = graph.outputs.iter().any(|declared| {
                    declared.from.node == node.id
                        && declared.from.name == output.name
                });
                if shown && !wired && !declared {
                    graph.outputs.push(GraphOutput {
                        name: format!("{}.{}", node.id, output.name),
                        value_type: output.value_type,
                        from: Port {
                            node: node.id,
                            name: output.name,
                        },
                    });
                }
            }
        }
        let mut ruleset = editor.rules.clone();
        ruleset.graphs.insert(editor.name.clone(), graph);
        // A node that fails stops only what needs it, so the rest is
        // still drawn while a graph is half built.
        editor.problems.clear();
        let evaluated = evaluate_partly(
            &editor.registry,
            &ruleset,
            editor.got.seeds,
            &BTreeMap::new(),
        );
        editor.got.outcome = evaluated
            .map(|(outputs, problems)| {
                editor.problems = problems;
                let mut put = Vec::new();
                for (name, value) in outputs {
                    match watch.get(&name) {
                        Some((node, port)) => editor
                            .got
                            .watched
                            .entry(*node)
                            .or_default()
                            .push((port.clone(), value)),
                        None => put.extend(shown(value)),
                    }
                }
                put
            })
            .map_err(|problem| problem.to_string());
    }

    fn status(editor: &Editor<Self>) -> String {
        match (&editor.got.outcome, editor.problems.first()) {
            (Ok(objects), None) => {
                format!("{} put down", objects.len())
            }
            (Ok(objects), Some(problem)) => format!(
                "{} put down; {} did not run: {problem}",
                objects.len(),
                editor.problems.len()
            ),
            (Err(problem), _) => problem.clone(),
        }
    }

    fn preview() -> AnyView {
        preview::view().boxed()
    }

    fn tools() -> Vec<AnyView> {
        vec![
            tool(
                Glyph::Book,
                "Export a bundle: the ruleset and its assets' files",
                export,
            )
            .boxed(),
        ]
    }

    fn sections() -> Vec<AnyView> {
        assets::part()
    }

    fn plugin(app: &mut App) {
        app.init_resource::<Exporter>()
            .add_observer(exported)
            .add_observer(forget)
            .add_systems(Update, watch)
            .add_plugins((
                assets::plugin,
                overlay::plugin,
                preview::plugin,
                gizmo::plugin,
            ));
    }
}

impl Editor<Blueprint> {
    /// Rolls again every random value picked once per item, as if
    /// this were another of them.
    pub fn roll_item(&mut self) {
        self.got.seeds.item = self.got.seeds.item.mix(1);
        Blueprint::run(self);
    }

    /// Rolls again every random value picked once per hamlet, as if
    /// the item were in another hamlet.
    pub fn roll_hamlet(&mut self) {
        self.got.seeds.hamlet = self.got.seeds.hamlet.mix(1);
        Blueprint::run(self);
    }
}

/// The objects an output shows.
fn shown(value: Value) -> Vec<Object> {
    match value {
        Value::Objects(objects) => objects,
        _ => Vec::new(),
    }
}

/// Runs the graph again when the selection is not what the last run
/// watched, so the preview shows what the selected nodes give.
fn watch(mut editor: ResMut<Editor<Blueprint>>) {
    if editor.got.watching != editor.selected {
        Blueprint::run(&mut editor);
    }
}

/// What the export dialog answers to.
#[derive(Resource, Default)]
struct Exporter(Option<Entity>);

/// Asks for a file to export the bundle to.
fn export(world: &mut World) {
    let exporter = match world.resource::<Exporter>().0 {
        Some(exporter) => exporter,
        None => {
            let exporter = world.spawn_empty().id();
            world.resource_mut::<Exporter>().0 = Some(exporter);
            exporter
        }
    };
    let name = format!(
        "{}-bundle.ron",
        world.resource::<Editor<Blueprint>>().name
    );
    world.trigger(ChooseFile {
        asker: exporter,
        mode: FileMode::Save,
        title: "Export a bundle".to_owned(),
        extension: "ron".to_owned(),
        name,
    });
}

/// Writes the bundle of `editor`'s book to `path`, with the files of
/// its assets read from `kept`. A graph that fails is not exported.
fn write_bundle(
    path: &Path,
    editor: &Editor<Blueprint>,
    kept: &Path,
) -> anyhow::Result<()> {
    if let Err(problem) = &editor.got.outcome {
        anyhow::bail!("Not exported, the graph fails: {problem}");
    }
    let ruleset = editor.book().export();
    let mut files = BTreeMap::new();
    for asset in ruleset.assets.0.values() {
        let Shape::Mesh(file) = &asset.shape else {
            continue;
        };
        let bytes = fs::read(kept.join(file))
            .with_context(|| format!("reading {file}"))?;
        files.insert(file.clone(), bytes);
    }
    let text = Bundle { ruleset, files }
        .to_ron()
        .context("writing the bundle")?;
    fs::write(path, text)
        .with_context(|| format!("writing {}", path.display()))
}

/// Forgets the meshes read for the last book when another is opened.
fn forget(
    chosen: On<FileChosen>,
    files: Res<Files<Blueprint>>,
    mut meshes: ResMut<Meshes>,
) {
    if files.asks(chosen.asker)
        && matches!(chosen.mode, FileMode::Open)
    {
        meshes.forget();
    }
}

/// Writes the bundle to the file the chief chose.
fn exported(
    chosen: On<FileChosen>,
    exporter: Res<Exporter>,
    folder: Option<Res<ViewFolder>>,
    editor: Res<Editor<Blueprint>>,
    mut files: ResMut<Files<Blueprint>>,
) {
    if exporter.0 != Some(chosen.asker) {
        return;
    }
    let kept = folder
        .map(|folder| files::book_folder::<Blueprint>(&folder.0));
    let path = match chosen.path.extension() {
        None => chosen.path.with_extension("ron"),
        Some(_) => chosen.path.clone(),
    };
    let name = path.file_name().map_or(String::new(), |name| {
        name.to_string_lossy().into_owned()
    });
    files.note = match kept
        .as_deref()
        .map(|kept| write_bundle(&path, &editor, kept))
    {
        Some(Ok(())) => format!("Exported {name}"),
        Some(Err(error)) => format!("{error:#}"),
        None => String::new(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::example;

    #[test]
    fn every_example_opens_and_puts_something_down() {
        let mut editor = Editor::<Blueprint>::new();
        for (name, text) in Blueprint::EXAMPLES {
            let book = example(text)
                .unwrap_or_else(|| panic!("{name} does not parse"));
            editor.load(book);
            // Each graph also runs alone, its inputs at their
            // defaults.
            for graph in editor.names() {
                editor.go_to(&graph);
                match &editor.got.outcome {
                    Ok(objects) => {
                        assert!(
                            !objects.is_empty(),
                            "{name}: {graph}"
                        );
                    }
                    Err(problem) => {
                        panic!("{name}: {graph}: {problem}")
                    }
                }
            }
            match &editor.got.outcome {
                Ok(objects) => {
                    assert!(!objects.is_empty(), "{name}");
                }
                Err(problem) => panic!("{name}: {problem}"),
            }
        }
    }
}
