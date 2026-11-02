//! The blueprint editor: graphs that put objects down, run again on
//! every change and drawn in the preview beside the canvas.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::Context;
use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy_fynix::ViewExt;
use hamlet_blueprint::{
    Bundle, Object, Seed, Seeds, Value, evaluate_partly,
};
use hamlet_desktop::file::{ChooseFile, FileChosen, FileMode};
use hamlet_graph::{
    Constant, Does, Graph, GraphError, GraphOutput, NodeId, Port,
    Registry, Setting, Shape, ValueType,
};
use hamlet_graph_editor::files::Files;
use hamlet_graph_editor::library::{
    Item, Kind, Library, mesh_path, write_atomic,
};
use hamlet_graph_editor::{Editor, Flavor, Nesting};
use hamlet_ui::AnyView;
use hamlet_ui::styles::tool;
use hamlet_ui::widgets::icons::Glyph;

use super::assets::Meshes;
use super::{assets, gizmo, overlay, preview};

/// Blueprint graphs.
pub(crate) struct Blueprint;

/// The result of the last run of a blueprint graph.
pub(crate) struct Got {
    /// The seeds the random values are picked from.
    pub seeds: Seeds,
    /// The objects the open graph put down, or why it could not run
    /// at all.
    pub outcome: Result<Vec<Object>, GraphError>,
    /// The outputs each selected node gave in the last run, by name.
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
    const KIND: Kind = Kind::Blueprint;
    const NESTING: Nesting = Nesting::Runs;
    /// The small examples first, each one idea; the house runs the
    /// three after it.
    const EXAMPLES: &'static [(&'static str, &'static str)] = &[
        ("fence", include_str!("examples/fence.ron")),
        ("stone_wall", include_str!("examples/stone_wall.ron")),
        ("window", include_str!("examples/window.ron")),
        ("stone_tower", include_str!("examples/stone_tower.ron")),
        ("garden_bed", include_str!("examples/garden_bed.ron")),
        ("house_wall", include_str!("examples/house_wall.ron")),
        ("house_roof", include_str!("examples/house_roof.ron")),
        ("house_window", include_str!("examples/house_window.ron")),
        (
            "medieval_house",
            include_str!("examples/medieval_house.ron"),
        ),
    ];

    type Got = Got;

    fn registry() -> Registry {
        hamlet_blueprint::registry()
    }

    fn upgrade(graph: &mut Graph) {
        hamlet_blueprint::upgrade(graph);
    }

    /// Runs the open graph with every unwired output shown too, and
    /// its inputs at their defaults.
    fn run(editor: &mut Editor<Self>) {
        let mut graph = editor.sheet.graph.clone();
        // Each selected node's outputs too, by names kept apart, so
        // the preview can show what it gives.
        let mut watch = BTreeMap::new();
        for node in &editor.selected {
            let Some(found) = editor.sheet.graph.node(*node) else {
                continue;
            };
            let Ok((_, outputs)) = hamlet_graph::ports(
                &editor.registry,
                &editor.rules,
                &editor.sheet.graph,
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
            let Ok((_, outputs)) = hamlet_graph::ports(
                &editor.registry,
                &editor.rules,
                &editor.sheet.graph,
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
                // The sheet's own outputs, not the watch ones added
                // above, which give the preview nothing to draw.
                let declared = editor.sheet.graph.outputs.iter().any(
                    |declared| {
                        declared.from.node == node.id
                            && declared.from.name == output.name
                    },
                );
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
        editor.errors.clear();
        let evaluated = evaluate_partly(
            &editor.registry,
            &ruleset,
            editor.got.seeds,
            &BTreeMap::new(),
        );
        editor.got.outcome = evaluated.map(|(outputs, errors)| {
            editor.errors = errors;
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
        });
    }

    fn status(editor: &Editor<Self>) -> String {
        match (&editor.got.outcome, editor.errors.first()) {
            (Ok(objects), None) => {
                format!("{} put down", objects.len())
            }
            (Ok(objects), Some(error)) => format!(
                "{} put down; {} did not run: {error}",
                objects.len(),
                editor.errors.len()
            ),
            (Err(error), _) => error.to_string(),
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

    fn tabs() -> Vec<(Glyph, &'static str, fn() -> AnyView)> {
        vec![(Glyph::Folder, "ASSETS", assets::tab)]
    }

    /// A mesh dropped on the canvas becomes an object node putting it
    /// down.
    fn dropped(
        editor: &mut Editor<Self>,
        item: &Item,
        at: Vec2,
    ) -> bool {
        if item.kind != Kind::Mesh {
            return false;
        }
        editor.add(
            Does::Kind("object".to_owned()),
            BTreeMap::from([(
                "asset".to_owned(),
                Setting::Fixed(Constant::Choice(item.name.clone())),
            )]),
            at,
        );
        true
    }

    fn plugin(app: &mut App) {
        app.init_resource::<Exporter>()
            .add_observer(exported)
            .add_observer(forget)
            .add_systems(Update, (watch, mirror))
            .add_plugins((
                assets::plugin,
                overlay::plugin,
                preview::plugin,
                gizmo::plugin,
            ));
    }
}

/// Rolls again every random value picked once per item, as if this
/// were another of them.
pub(super) fn roll_item(editor: &mut Editor<Blueprint>) {
    editor.got.seeds.item = editor.got.seeds.item.mix(1);
    Blueprint::run(editor);
}

/// Rolls again every random value picked once per hamlet, as if the
/// item were in another hamlet.
pub(super) fn roll_hamlet(editor: &mut Editor<Blueprint>) {
    editor.got.seeds.hamlet = editor.got.seeds.hamlet.mix(1);
    Blueprint::run(editor);
}

/// The objects an output shows.
fn shown(value: Value) -> Vec<Object> {
    match value {
        Value::Objects(objects) => objects,
        _ => Vec::new(),
    }
}

/// Gives the graphs the library's meshes, each at its file's size.
fn mirror(
    library: Res<Library>,
    mut editor: ResMut<Editor<Blueprint>>,
    mut meshes: ResMut<Meshes>,
) {
    if library.is_changed() || editor.is_added() {
        let mut catalogue = library.catalogue.clone();
        if let Some(root) = library.root() {
            meshes.measure(&mut catalogue, root);
        }
        editor.set_catalogue(catalogue);
    }
}

/// Runs the graph again when the selection is not what the last run
/// watched, so the preview shows what the selected nodes give.
fn watch(mut editor: ResMut<Editor<Blueprint>>) {
    if editor.got.watching != editor.selected {
        Blueprint::run(&mut editor);
    }
}

/// The entity the export dialog answers to.
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

/// Writes the bundle of the open graph and the graphs it runs to
/// `path`, with the library's meshes they name, their files read from
/// the library at `library`. A graph that fails is not exported.
fn write_bundle(
    path: &Path,
    editor: &Editor<Blueprint>,
    library: &Path,
) -> anyhow::Result<()> {
    if let Err(error) = &editor.got.outcome {
        anyhow::bail!("Not exported, the graph fails: {error}");
    }
    let mut ruleset = editor.reached();
    ruleset.assets =
        editor.rules.assets.named_in(ruleset.graphs.values());
    let mut files = BTreeMap::new();
    for asset in ruleset.assets.0.values() {
        let Shape::Mesh(file) = &asset.shape else {
            continue;
        };
        let bytes = fs::read(mesh_path(library, file)?)
            .with_context(|| format!("reading {file}"))?;
        files.insert(file.clone(), bytes);
    }
    let text = Bundle { ruleset, files }
        .to_ron()
        .context("writing the bundle")?;
    write_atomic(path, &text)
}

/// Forgets the meshes read before a graph file is imported.
fn forget(
    chosen: On<FileChosen>,
    files: Res<Files<Blueprint>>,
    mut meshes: ResMut<Meshes>,
) {
    if files.is_asker(chosen.asker)
        && matches!(chosen.mode, FileMode::Open)
    {
        meshes.forget();
    }
}

/// Writes the bundle to the file the chief chose.
fn exported(
    chosen: On<FileChosen>,
    exporter: Res<Exporter>,
    library: Res<Library>,
    editor: Res<Editor<Blueprint>>,
    mut files: ResMut<Files<Blueprint>>,
) {
    if exporter.0 != Some(chosen.asker) {
        return;
    }
    let kept = library.root();
    let path = match chosen.path.extension() {
        None => chosen.path.with_extension("ron"),
        Some(_) => chosen.path.clone(),
    };
    let name = path.file_name().map_or(String::new(), |name| {
        name.to_string_lossy().into_owned()
    });
    files.note =
        match kept.map(|kept| write_bundle(&path, &editor, kept)) {
            Some(Ok(())) => format!("Exported {name}"),
            Some(Err(error)) => format!("{error:#}"),
            None => String::new(),
        };
}

#[cfg(test)]
mod tests {
    use hamlet_graph_editor::library::{Source, lone, parse_book};

    use super::*;

    #[test]
    fn every_example_runs_with_the_others_available_and_puts_something_down()
     {
        let sheets = Blueprint::EXAMPLES
            .iter()
            .map(|(name, text)| {
                let book = parse_book(text).unwrap_or_else(|| {
                    panic!("{name} does not parse")
                });
                ((*name).to_owned(), lone(book))
            })
            .collect::<BTreeMap<_, _>>();
        let mut editor = Editor::<Blueprint>::new();
        for (name, text) in Blueprint::EXAMPLES {
            editor.open(
                (*name).to_owned(),
                Source::Example(text),
                sheets[*name].clone(),
            );
            editor.set_others(
                sheets
                    .iter()
                    .filter(|(other, _)| *other != name)
                    .map(|(other, sheet)| {
                        (other.clone(), sheet.graph.clone())
                    })
                    .collect(),
            );
            match &editor.got.outcome {
                Ok(objects) => {
                    assert!(!objects.is_empty(), "{name}");
                }
                Err(error) => panic!("{name}: {error}"),
            }
        }
    }
}
