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
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::{ViewExt, derived};
use hamlet_blueprint::{
    Bundle, Object, Seed, Seeds, Value, evaluate_partly,
};
use hamlet_desktop::file::{ChooseFile, FileChosen, FileMode};
use hamlet_graph::{
    Constant, Does, GraphError, GraphOutput, NodeId, Port, Setting,
    Shape, ValueType,
};
use hamlet_graph_editor::files::Files;
use hamlet_graph_editor::library::{
    Kind, Library, mesh_path, write_atomic,
};
use hamlet_graph_editor::{
    Dropped, Editor, Nesting, Setup, editor, toolbar,
};
use hamlet_realm::objects::Models;
use hamlet_ui::AnyView;
use hamlet_ui::styles::{self, description, tool};
use hamlet_ui::widgets::icons::Glyph;

use super::{assets, gizmo, overlay, preview};
use crate::dock::{PanelAppExt, flush_panel};
use crate::roll::moved;

/// Blueprint graphs.
pub(crate) struct Blueprint;

/// The result of the last run of the open blueprint graph.
#[derive(Resource)]
pub(crate) struct Got {
    /// The editor's changes the last run saw; none to run again.
    ran: Option<u64>,
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
            ran: None,
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

/// The icon of the blueprint editor's panel and preview.
const ICON: Glyph = Glyph::Signpost;

/// Blueprint graphs, and how the editor keeps them.
pub(crate) const SETUP: Setup = Setup {
    kind: Kind::Blueprint,
    folder: "graphs",
    registry: hamlet_blueprint::registry,
    upgrade: hamlet_blueprint::upgrade,
    nesting: Nesting::Runs,
    outputs: true,
    room: 0.0,
    // The small examples first, each one idea; the house runs the
    // three after it.
    examples: &[
        ("fence", include_str!("examples/fence.ron")),
        ("stone_wall", include_str!("examples/stone_wall.ron")),
        ("window", include_str!("examples/window.ron")),
        ("stone_tower", include_str!("examples/stone_tower.ron")),
        (
            "stepping_stones",
            include_str!("examples/stepping_stones.ron"),
        ),
        ("garden_bed", include_str!("examples/garden_bed.ron")),
        ("house_wall", include_str!("examples/house_wall.ron")),
        ("house_roof", include_str!("examples/house_roof.ron")),
        ("house_window", include_str!("examples/house_window.ron")),
        (
            "medieval_house",
            include_str!("examples/medieval_house.ron"),
        ),
        // The book the realm builds every house from.
        ("house", hamlet_realm::generator::house::BOOK),
    ],
};

/// Adds the blueprint editor in a panel of the dock, run again on
/// every change and drawn in the preview beside the canvas.
pub(super) fn plugin(app: &mut App) {
    app.add_panel(
        "graphs",
        DockWindowKind::new("BLUEPRINT", || flush_panel(screen()))
            .icon(ICON.image(styles::ICON)),
    )
    .init_resource::<Got>()
    .init_resource::<Exporter>()
    .add_observer(exported)
    .add_observer(forget)
    .add_observer(dropped)
    .add_systems(Update, (run, mirror))
    .add_plugins((
        hamlet_graph_editor::plugin::<Blueprint>(SETUP),
        assets::plugin,
        overlay::plugin,
        preview::plugin,
        gizmo::plugin,
    ));
}

/// The editor's panel: the toolbar with the export tool and what the
/// last run put down, the parameters card, and the preview beside
/// the canvas.
fn screen() -> AnyView {
    let status = description(
        derived(|world: &World| {
            status(
                world.resource::<Editor<Blueprint>>(),
                world.resource::<Got>(),
            )
        })
        .when(moved::<Got>()),
    );
    let toolbar = toolbar::bar(
        vec![
            toolbar::asset::<Blueprint>(),
            tool(
                Glyph::Book,
                "Export a bundle: the ruleset and its assets' files",
                export,
            )
            .boxed(),
        ],
        vec![
            toolbar::file_note::<Blueprint>(),
            status.boxed(),
            toolbar::history::<Blueprint>(),
            toolbar::fit::<Blueprint>(),
            toolbar::cards::<Blueprint>(ICON),
        ],
    );
    let parameters =
        hamlet_graph_editor::parameters::<Blueprint>(Vec::new());
    editor::<Blueprint>(
        toolbar,
        vec![parameters],
        Some(preview::view().boxed()),
    )
    .boxed()
}

/// Runs the open graph again when it changed, when the selection is
/// not what the last run watched, or when the seeds were rolled, so
/// the preview shows what the selected nodes give.
fn run(mut editor: ResMut<Editor<Blueprint>>, mut got: ResMut<Got>) {
    if got.ran != Some(editor.changes)
        || got.watching != editor.selected
    {
        evaluate(&mut editor, &mut got);
    }
}

/// What the end of the toolbar says of the last run.
fn status(editor: &Editor<Blueprint>, got: &Got) -> String {
    match (&got.outcome, editor.errors.first()) {
        (Ok(objects), None) => format!("{} put down", objects.len()),
        (Ok(objects), Some(error)) => format!(
            "{} put down; {} did not run: {error}",
            objects.len(),
            editor.errors.len()
        ),
        (Err(error), _) => error.to_string(),
    }
}

/// A mesh dropped on the canvas becomes an object node putting it
/// down.
fn dropped(
    dropped: On<Dropped<Blueprint>>,
    mut editor: ResMut<Editor<Blueprint>>,
) {
    if dropped.item.kind != Kind::Mesh {
        return;
    }
    editor.add(
        Does::Kind("object".to_owned()),
        BTreeMap::from([(
            "asset".to_owned(),
            Setting::Fixed(Constant::Choice(
                dropped.item.name.clone(),
            )),
        )]),
        dropped.at,
    );
}

/// Runs the open graph with every unwired output shown too, and its
/// inputs at their defaults, into `got`.
fn evaluate(editor: &mut Editor<Blueprint>, got: &mut Got) {
    got.ran = Some(editor.changes);
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
            watch.insert(name.clone(), (*node, output.name.clone()));
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
    got.watching = editor.selected.clone();
    got.watched.clear();
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
            let declared =
                editor.sheet.graph.outputs.iter().any(|declared| {
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
    editor.errors.clear();
    let evaluated = evaluate_partly(
        &editor.registry,
        &ruleset,
        got.seeds,
        &BTreeMap::new(),
    );
    got.outcome = evaluated.map(|(outputs, errors)| {
        editor.errors = errors;
        let mut put = Vec::new();
        for (name, value) in outputs {
            match watch.get(&name) {
                Some((node, port)) => got
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

/// Rolls again every random value picked once per item, as if this
/// were another of them.
pub(super) fn roll_item(got: &mut Got) {
    got.seeds.item = got.seeds.item.mix(1);
    got.ran = None;
}

/// Rolls again every random value picked once per hamlet, as if the
/// item were in another hamlet.
pub(super) fn roll_hamlet(got: &mut Got) {
    got.seeds.hamlet = got.seeds.hamlet.mix(1);
    got.ran = None;
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
    mut meshes: ResMut<Models>,
) {
    if library.is_changed() || editor.is_added() {
        let mut catalogue = library.catalogue.clone();
        if let Some(root) = library.root() {
            meshes.measure(&mut catalogue, root);
        }
        editor.set_catalogue(catalogue);
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
/// the library at `library`. A graph that fails, as `got` says, is
/// not exported.
fn write_bundle(
    path: &Path,
    editor: &Editor<Blueprint>,
    got: &Got,
    library: &Path,
) -> anyhow::Result<()> {
    if let Err(error) = &got.outcome {
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
    mut meshes: ResMut<Models>,
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
    got: Res<Got>,
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
    files.note = match kept
        .map(|kept| write_bundle(&path, &editor, &got, kept))
    {
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
        let sheets = SETUP
            .examples
            .iter()
            .map(|(name, text)| {
                let book = parse_book(text).unwrap_or_else(|| {
                    panic!("{name} does not parse")
                });
                ((*name).to_owned(), lone(book))
            })
            .collect::<BTreeMap<_, _>>();
        let mut editor = Editor::<Blueprint>::new(SETUP);
        let mut got = Got::default();
        for (name, text) in SETUP.examples {
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
            evaluate(&mut editor, &mut got);
            match &got.outcome {
                Ok(objects) => {
                    assert!(!objects.is_empty(), "{name}");
                }
                Err(error) => panic!("{name}: {error}"),
            }
        }
    }
}
