//! The graph editor: three panels on one sheet of nodes and wires.
//! The Graph panel is the canvas with its tools, the Properties panel
//! the selected node's inputs, and the Preview panel what the graph
//! puts down, run again on every change.

mod canvas;
mod inspect;
mod menu;
mod preview;

use std::collections::BTreeMap;
use std::mem::discriminant;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Local, ResMut};
use bevy::ecs::world::World;
use bevy::math::{Vec2, Vec3};
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::dock::{DockTree, DockWindowKind, Edge};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame, row,
};
use bevy_fynix::{ScopedExt, resource};
use hamlet_graph::{
    Does, Edit, Frame, GraphOutput, Instance, Node, NodeId, Port,
    Registry, Ruleset, Seed, Seeds, Sheet, Value, ValueType,
    evaluate,
};

use crate::dock::{PanelAppExt, panel};
use crate::feel::Mood;
use crate::icons::{Glyph, glyph};
use crate::looks::{
    CONTROL, Fills, ICON, brass, description, note, raised,
};
use crate::{View, with_cx};

/// The name of the one graph the editor holds, in the ruleset it
/// runs.
const GRAPH: &str = "main";

/// The sheet the editor opens with.
const START: &str = include_str!("graphs/start.ron");

const PROPERTIES: &str = "graph_properties";
const PREVIEW: &str = "graph_preview";

pub(crate) fn plugin(app: &mut App) {
    app.add_panel(
        "graphs",
        DockWindowKind::new("GRAPH", || panel(screen()))
            .icon(Glyph::Signpost.image(ICON)),
    )
    .add_panel(
        PROPERTIES,
        DockWindowKind::new("PROPERTIES", || panel(inspect::view()))
            .icon(Glyph::Sliders.image(ICON)),
    )
    .add_panel(
        PREVIEW,
        DockWindowKind::new("PREVIEW", || panel(preview::view()))
            .icon(Glyph::Hamlet.image(ICON)),
    )
    .insert_resource(Editor::new())
    .add_plugins((canvas::plugin, menu::plugin, preview::plugin))
    .add_systems(Update, arrange);
}

/// The sheet being edited, its history, and what it last gave.
#[derive(Resource)]
pub(crate) struct Editor {
    pub sheet: Sheet,
    pub selected: Option<NodeId>,
    pub registry: Registry,
    pub seeds: Seeds,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// Changes whenever the nodes, the wires or the way an input is
    /// set change, so what is built from them is built again.
    pub shape: u64,
    /// What the graph put down, or why it could not.
    pub outcome: Result<Vec<Instance>, String>,
}

impl Editor {
    fn new() -> Self {
        let sheet = Sheet::from_ron(START).unwrap_or_default();
        let mut editor = Self {
            sheet,
            selected: None,
            registry: Registry::builtin(),
            seeds: Seeds {
                realm: Seed(1),
                hamlet: Seed(2),
                house: Seed(3),
            },
            undo: Vec::new(),
            redo: Vec::new(),
            shape: 0,
            outcome: Ok(Vec::new()),
        };
        editor.run();
        editor
    }

    /// Applies `edit` and keeps the edit that undoes it. Edits of one
    /// input in a row, as a number dragged, are undone as one.
    pub fn apply(&mut self, edit: Edit) {
        let reshapes = reshapes(&self.sheet, &edit);
        let Ok(inverse) = self.sheet.apply(edit) else {
            return;
        };
        let merges = matches!(
            (&inverse, self.undo.last()),
            (
                Edit::Set { node, input, .. },
                Some(Edit::Set { node: last, input: last_input, .. }),
            ) if node == last && input == last_input
        );
        if !merges {
            self.undo.push(inverse);
        }
        self.redo.clear();
        self.changed(reshapes);
    }

    /// Moves a node while it is dragged, kept out of the history
    /// until the drag ends.
    pub fn drag(&mut self, node: NodeId, to: Vec2) {
        self.sheet.layout.insert(node, to);
    }

    /// Keeps the move of a finished drag that began at `from`.
    pub fn dropped(&mut self, node: NodeId, from: Vec2) {
        self.undo.push(Edit::Move {
            node,
            to: Some(from),
        });
        self.redo.clear();
    }

    pub fn undo(&mut self) {
        if let Some(edit) = self.undo.pop()
            && let Ok(inverse) = self.sheet.apply(edit)
        {
            self.redo.push(inverse);
            self.changed(true);
        }
    }

    pub fn redo(&mut self) {
        if let Some(edit) = self.redo.pop()
            && let Ok(inverse) = self.sheet.apply(edit)
        {
            self.undo.push(inverse);
            self.changed(true);
        }
    }

    fn changed(&mut self, reshaped: bool) {
        if reshaped {
            self.shape += 1;
        }
        if self
            .selected
            .is_some_and(|node| self.sheet.graph.node(node).is_none())
        {
            self.selected = None;
        }
        self.run();
    }

    /// Runs the graph with every output nothing is wired to shown
    /// too, so a graph is seen as it is built.
    fn run(&mut self) {
        let mut graph = self.sheet.graph.clone();
        for node in &self.sheet.graph.nodes {
            let Ok((_, outputs)) = hamlet_graph::ports(
                &self.registry,
                &Ruleset::default(),
                node,
            ) else {
                continue;
            };
            for output in outputs {
                let shown = matches!(
                    output.value_type,
                    ValueType::Geometry | ValueType::Placements
                );
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
        let ruleset = Ruleset {
            entry: GRAPH.to_owned(),
            graphs: BTreeMap::from([(GRAPH.to_owned(), graph)]),
        };
        self.outcome = evaluate(
            &self.registry,
            &ruleset,
            self.seeds,
            &BTreeMap::new(),
        )
        .map(|outputs| {
            outputs.into_values().flat_map(shown).collect()
        })
        .map_err(|problem| problem.to_string());
    }

    /// Rolls every random value of the house again.
    fn reroll(&mut self) {
        self.seeds.house = self.seeds.house.mix(1);
        self.run();
    }

    /// Adds a node of `kind` with its card at `at`.
    pub fn add(&mut self, kind: &str, at: Vec2) {
        let id = self.sheet.graph.next_id();
        self.apply(Edit::AddNode {
            node: Node {
                id,
                does: Does::Kind(kind.to_owned()),
                settings: BTreeMap::new(),
            },
            at: Some(at),
        });
        self.selected = Some(id);
    }

    /// Adds a copy of `node`, unwired, a little below and to the
    /// right.
    pub fn duplicate(&mut self, node: NodeId) {
        let Some(original) = self.sheet.graph.node(node).cloned()
        else {
            return;
        };
        let id = self.sheet.graph.next_id();
        let at =
            self.sheet.layout.get(&node).copied().unwrap_or_default();
        self.apply(Edit::AddNode {
            node: Node { id, ..original },
            at: Some(at + Vec2::splat(28.0)),
        });
        self.selected = Some(id);
    }

    pub fn remove(&mut self, node: NodeId) {
        self.apply(Edit::RemoveNode(node));
    }

    /// Cuts every wire into or out of `node`.
    pub fn unwire(&mut self, node: NodeId) {
        let wires = self
            .sheet
            .graph
            .wires
            .iter()
            .filter(|wire| {
                wire.from.node == node || wire.to.node == node
            })
            .cloned()
            .map(Edit::Disconnect)
            .collect::<Vec<_>>();
        if !wires.is_empty() {
            self.apply(Edit::Batch(wires));
        }
    }

    fn remove_selected(&mut self) {
        if let Some(node) = self.selected.take() {
            self.remove(node);
        }
    }
}

/// Whether `edit` changes what the canvas or the inspector are built
/// from, rather than only a value in them.
fn reshapes(sheet: &Sheet, edit: &Edit) -> bool {
    match edit {
        Edit::Set {
            node,
            input,
            setting,
        } => {
            let old = sheet
                .graph
                .node(*node)
                .and_then(|node| node.settings.get(input));
            match (old, setting) {
                (Some(old), Some(new)) => {
                    discriminant(old) != discriminant(new)
                }
                (old, new) => old.is_some() != new.is_some(),
            }
        }
        Edit::Move { .. } => false,
        _ => true,
    }
}

/// The assets an output shows: its own, or a marker at each
/// placement.
fn shown(value: Value) -> Vec<Instance> {
    match value {
        Value::Geometry(instances) => instances,
        Value::Placements(placements) => placements
            .into_iter()
            .map(|placement| Instance {
                asset: preview::MARKER.to_owned(),
                frame: Frame {
                    scale: Vec3::ONE,
                    ..placement.frame
                },
                size: Vec3::splat(0.25),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Opens the Properties and Preview panels beside the Graph panel the
/// first time it is opened, each in a pane of its own whose divider
/// can be dragged. Closed again, they stay closed.
fn arrange(mut tree: ResMut<DockTree>, mut was_open: Local<bool>) {
    if !tree.is_changed() {
        return;
    }
    let graph = tree.find_leaf_with_window("graphs");
    let open = graph.is_some();
    let first = open && !*was_open;
    *was_open = open;
    let Some(graph) = graph.filter(|_| first) else {
        return;
    };
    if tree.find_leaf_with_window(PROPERTIES).is_some()
        || tree.find_leaf_with_window(PREVIEW).is_some()
    {
        return;
    }
    let Some((side, _)) =
        tree.split(graph, Edge::Right, PROPERTIES.into())
    else {
        return;
    };
    if let Some(parent) = tree.parent_of(side) {
        tree.set_fraction(parent, 0.7);
    }
    if let Some((above, _)) =
        tree.split(side, Edge::Top, PREVIEW.into())
        && let Some(parent) = tree.parent_of(above)
    {
        tree.set_fraction(parent, 0.5);
    }
}

/// The Graph panel: its tools over the canvas.
fn screen() -> impl View {
    with_cx(|cx| {
        let gap = cx.theme().space.gap;
        column((tools(), canvas::view(), menu::view()))
            .fills_width()
            .fills_height()
            .gap(gap)
    })
}

/// A button of one icon that runs `act`, named by `tip` under the
/// pointer.
fn tool(
    which: Glyph,
    tip: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    button(raised(glyph(which, ICON)))
        .padding(UiRect::axes(px(9.0), px(0.0)))
        .min_height(px(CONTROL))
        .tooltip(move || note(tip))
        .rules(brass)
        .tagged(Mood::Plain)
        .on_activate(act)
}

fn tools() -> impl View {
    with_cx(|cx| {
        let gap = cx.theme().space.gap;
        row((
            tool(
                Glyph::Plus,
                "Add a node (Space)",
                menu::create_at_pointer,
            ),
            tool(Glyph::Undo, "Undo (Cmd+Z)", |world| {
                world.resource_mut::<Editor>().undo();
            }),
            tool(Glyph::Redo, "Redo (Shift+Cmd+Z)", |world| {
                world.resource_mut::<Editor>().redo();
            }),
            tool(
                Glyph::Delete,
                "Delete the selected node (Delete)",
                |world| {
                    world.resource_mut::<Editor>().remove_selected();
                },
            ),
            tool(
                Glyph::Aim,
                "Fit every node in sight (F)",
                canvas::fit,
            ),
            tool(
                Glyph::Dice,
                "Roll the house's random values again",
                |world| {
                    world.resource_mut::<Editor>().reroll();
                },
            ),
            frame().grow(1.0),
            description(resource::<Editor, _>(
                |editor| match &editor.outcome {
                    Ok(instances) => {
                        format!("{} put down", instances.len())
                    }
                    Err(problem) => problem.clone(),
                },
            )),
        ))
        .gap(gap / 2.0)
        .align(AlignItems::Center)
    })
}
