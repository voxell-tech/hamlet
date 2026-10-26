//! The graph editor: a sheet of nodes and wires on a canvas, the
//! selected node's inputs, and a preview of what the graph puts down,
//! run again on every change.

mod canvas;
mod inspect;
mod preview;

use std::collections::BTreeMap;
use std::mem::discriminant;

use bevy::app::App;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::math::{Vec2, Vec3};
use bevy::ui::{AlignItems, percent, px};
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::views::{FrameProps, column, dropdown, row};
use bevy_fynix::{ScopedExt, resource};
use hamlet_graph::{
    Does, Edit, Frame, GraphOutput, Instance, Node, NodeId, Port,
    Registry, Ruleset, Seed, Seeds, Sheet, Value, ValueType,
    evaluate,
};

use crate::dock::{PanelAppExt, panel};
use crate::icons::Glyph;
use crate::looks::{Fills, ICON, command, description, well};
use crate::{View, with_cx};

/// The name of the one graph the editor holds, in the ruleset it
/// runs.
const GRAPH: &str = "main";

/// The sheet the editor opens with.
const START: &str = include_str!("graphs/start.ron");

/// The width of the column beside the canvas.
const SIDE: f32 = 320.0;

pub(crate) fn plugin(app: &mut App) {
    app.add_panel(
        "graphs",
        DockWindowKind::new("GRAPHS", || panel(screen()))
            .icon(Glyph::Sliders.image(ICON)),
    )
    .insert_resource(Editor::new())
    .add_plugins((canvas::plugin, preview::plugin));
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

    /// Adds a node of `kind` near the middle of what is in sight.
    fn add(&mut self, kind: &str, at: Vec2) {
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

    fn remove_selected(&mut self) {
        if let Some(node) = self.selected.take() {
            self.apply(Edit::RemoveNode(node));
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

/// The panel: the canvas, and beside it the tools, the selected node
/// and the preview.
fn screen() -> impl View {
    with_cx(|cx| {
        let gap = cx.theme().space.gap;
        row((
            canvas::view(),
            column((
                tools(),
                status(),
                inspect::view(),
                preview::view(),
            ))
            .width(px(SIDE))
            .shrink(0.0)
            .min_height(px(0.0))
            .gap(gap),
        ))
        .fills_width()
        .fills_height()
        .gap(gap)
    })
}

fn tools() -> impl View {
    with_cx(|cx| {
        let kinds = cx
            .world
            .resource::<Editor>()
            .registry
            .kinds()
            .map(|kind| kind.name)
            .collect::<Vec<_>>();
        let names = kinds.clone();
        column((
            dropdown(
                names,
                usize::MAX,
                Glyph::Chevron.image(ICON),
                move |world: &mut World, at| {
                    let middle = canvas::middle(world);
                    world
                        .resource_mut::<Editor>()
                        .add(kinds[at], middle);
                },
            )
            .placeholder("Add a node")
            .width(percent(100.0))
            .rules(well),
            row((
                command("Undo", |world| {
                    world.resource_mut::<Editor>().undo();
                }),
                command("Redo", |world| {
                    world.resource_mut::<Editor>().redo();
                }),
                command("Delete", |world| {
                    world.resource_mut::<Editor>().remove_selected();
                }),
                command("Reroll", |world| {
                    world.resource_mut::<Editor>().reroll();
                }),
            ))
            .gap(cx.theme().space.gap / 2.0)
            .align(AlignItems::Center),
        ))
        .gap(cx.theme().space.gap)
    })
}

/// How many assets the graph put down, or what is wrong with it.
fn status() -> impl View {
    description(resource::<Editor, _>(|editor| {
        match &editor.outcome {
            Ok(instances) => format!("{} put down", instances.len()),
            Err(problem) => problem.clone(),
        }
    }))
}
