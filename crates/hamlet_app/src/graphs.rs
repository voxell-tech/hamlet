//! The graph editor: one panel holding the canvas with its tools, and
//! beside it two inner panels, the preview of what the graph puts
//! down, run again on every change, and the selected node's
//! properties.

mod canvas;
mod inspect;
mod menu;
mod preview;
mod split;

use std::collections::BTreeMap;
use std::mem::discriminant;

use bevy::app::App;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::math::{Vec2, Vec3};
use bevy::ui::{
    AlignItems, Display, Overflow, PositionType, UiRect, percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, icon_button, row,
};
use bevy_fynix::{EntityCursor, ScopedExt, ViewExt, keyed, resource};
use hamlet_graph::{
    Does, Edit, Frame, GraphOutput, Instance, Node, NodeId, Port,
    Registry, Ruleset, Seed, Seeds, Sheet, Value, ValueType,
    evaluate,
};

use self::split::{Floating, Handle, Layout, divider, inset_at};
use crate::dock::{PanelAppExt, panel};
use crate::feel::Mood;
use crate::icons::{Glyph, glyph};
use crate::looks::{
    CONTROL, Fills, ICON, brass, description, note, raised, section,
    well,
};
use crate::{View, with_cx};

/// The name of the one graph the editor holds, in the ruleset it
/// runs.
const GRAPH: &str = "main";

/// The sheet the editor opens with.
const START: &str = include_str!("graphs/start.ron");

/// Sheets to start from, by name.
const EXAMPLES: [(&str, &str); 2] = [
    ("Fence of pillars", START),
    (
        "Medieval house",
        include_str!("graphs/examples/medieval_house.ron"),
    ),
];

pub(crate) fn plugin(app: &mut App) {
    app.add_panel(
        "graphs",
        DockWindowKind::new("GRAPH", || panel(screen()))
            .icon(Glyph::Signpost.image(ICON)),
    )
    .insert_resource(Editor::new())
    .add_plugins((
        canvas::plugin,
        menu::plugin,
        preview::plugin,
        split::plugin,
    ));
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

    /// Puts `sheet` in place of the one being edited, with no
    /// history.
    fn load(&mut self, sheet: Sheet) {
        self.sheet = sheet;
        self.selected = None;
        self.undo.clear();
        self.redo.clear();
        self.changed(true);
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

/// The width of the properties floating over the canvas.
const FLOATING: f32 = 300.0;

/// The Graph panel: its tools over the canvas, the properties
/// floating over the canvas's corner, and the preview beside it,
/// sized by dragging the line between them.
fn screen() -> impl View {
    with_cx(|cx| {
        let gap = cx.theme().space.gap;
        let preview =
            inner(Glyph::Hamlet, "PREVIEW", preview::view())
                .width(resource::<Layout, _>(|layout| {
                    px(layout.preview)
                }))
                .shrink(0.0);
        row((
            column((
                tools(),
                // The properties are placed over the canvas, in its
                // frame.
                column((
                    canvas::view(),
                    dock_outline(),
                    properties(),
                ))
                .fills_width()
                .fills_height()
                .gap(0.0),
                menu::view(),
            ))
            .fills_width()
            .gap(gap),
            divider(),
            preview,
        ))
        .fills_width()
        .fills_height()
        .gap(gap / 2.0)
    })
}

/// A header over `content` in a card, for a part of the Graph panel.
fn inner(
    which: Glyph,
    title: &'static str,
    content: impl View,
) -> impl View + FrameProps {
    let gap = 8.0;
    column((
        row((glyph(which, ICON), section(title)))
            .gap(gap / 2.0)
            .align(AlignItems::Center),
        content,
    ))
    .min_height(px(0.0))
    .padding(UiRect::all(px(gap)))
    .gap(gap)
}

/// A lit outline of the corner dragged properties will settle in.
fn dock_outline() -> impl View {
    with_cx(|cx| {
        let theme = cx.theme();
        frame()
            .position(PositionType::Absolute)
            .inset(resource::<Layout, _>(|layout| {
                inset_at(layout.ghost.unwrap_or(layout.shown))
            }))
            .width(px(FLOATING))
            .height(resource::<Layout, _>(|layout| px(layout.size.y)))
            .display(resource::<Layout, _>(|layout| {
                if layout.ghost.is_some() {
                    Display::Flex
                } else {
                    Display::None
                }
            }))
            .fill(theme.palette.chosen)
            .border(2.0)
            .border_color(theme.palette.lamp)
            .radius(theme.space.round)
    })
}

/// The selected node's properties, floating over a corner of the
/// canvas. Its header drags it and its chevron folds it.
fn properties() -> impl View {
    with_cx(|cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        // Open, the chevron points up to fold; folded, it points on.
        let fold = keyed(
            resource::<Layout, _>(|layout| layout.properties),
            |open| {
                let which =
                    if *open { Glyph::Chevron } else { Glyph::Into };
                button(glyph(which, ICON))
                    .rules(icon_button)
                    .tooltip(|| note("Fold or open the properties"))
                    .on_activate(|world| {
                        let mut layout =
                            world.resource_mut::<Layout>();
                        layout.properties = !layout.properties;
                    })
                    .boxed()
            },
        )
        .within(row(()).gap(0.0));
        let header = row((
            glyph(Glyph::Sliders, ICON),
            section("PROPERTIES"),
            frame().grow(1.0),
            fold,
        ))
        .gap(gap / 2.0)
        .align(AlignItems::Center)
        .tagged(Handle)
        .tagged(EntityCursor(SystemCursorIcon::Grab));
        let body =
            column((inspect::view(),)).display(
                resource::<Layout, _>(|layout| {
                    if layout.properties {
                        Display::Flex
                    } else {
                        Display::None
                    }
                }),
            );
        column((header, body))
            .position(PositionType::Absolute)
            .inset(resource::<Layout, _>(|layout| {
                inset_at(layout.shown)
            }))
            .width(px(FLOATING))
            .max_height(percent(100.0))
            .overflow(Overflow::clip())
            .padding(UiRect::all(px(gap)))
            .gap(gap / 2.0)
            .fill(theme.palette.card)
            .border(theme.space.border)
            .border_color(theme.palette.seam)
            .radius(theme.space.round)
            .tagged(Floating)
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
            dropdown(
                EXAMPLES.map(|(name, _)| name),
                usize::MAX,
                Glyph::Chevron.image(ICON),
                |world: &mut World, at| {
                    let Ok(sheet) = Sheet::from_ron(EXAMPLES[at].1)
                    else {
                        return;
                    };
                    world.resource_mut::<Editor>().load(sheet);
                    canvas::fit(world);
                },
            )
            .placeholder("Open an example")
            .width(px(190.0))
            .rules(well),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_example_opens_and_puts_something_down() {
        let mut editor = Editor::new();
        for (name, text) in EXAMPLES {
            let sheet = Sheet::from_ron(text)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            editor.load(sheet);
            match &editor.outcome {
                Ok(instances) => {
                    assert!(!instances.is_empty(), "{name}");
                }
                Err(problem) => panic!("{name}: {problem}"),
            }
        }
    }
}
