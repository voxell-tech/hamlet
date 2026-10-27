//! The graph editor: one panel holding the canvas with its tools, and
//! beside it two inner panels, the preview of what the graph puts
//! down, run again on every change, and the selected node's
//! properties.

mod canvas;
mod files;
mod inspect;
mod menu;
mod params;
mod preview;
mod split;
mod wire;

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
use bevy_fynix::tokens::Motion;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, icon_button, row,
};
use bevy_fynix::{
    Bevy, EntityCursor, ScopedExt, ViewExt, ViewSeq, keyed, resource,
};
use hamlet_blueprint::{
    Book, Constant, Does, Edit, Frame, GraphInput, GraphOutput,
    Instance, Node, NodeId, Port, Registry, Ruleset, Seed, Seeds,
    Setting, Sheet, Value, ValueType, evaluate,
};

use self::split::{Floating, Handle, Layout, divider, inset_at};
use crate::dock::{PanelAppExt, flush_panel};
use crate::feel::Mood;
use crate::icons::{Glyph, glyph};
use crate::looks::{
    CONTROL, Fills, ICON, brass, description, hairline, note, raised,
    section,
};
use crate::theme::HamletTheme;
use crate::{View, with_cx};

/// The book the editor opens with.
const START: &str =
    include_str!("graphs/examples/medieval_house.ron");

/// Books to start from, by name: a sheet alone, or a book of sheets.
const EXAMPLES: [(&str, &str); 1] = [("Medieval house", START)];

/// The book an example's text holds.
fn example(text: &str) -> Option<Book> {
    Book::from_ron(text).ok().or_else(|| {
        Sheet::from_ron(text)
            .ok()
            .map(|sheet| Book::of("main", sheet))
    })
}

pub(crate) fn plugin(app: &mut App) {
    app.add_panel(
        "graphs",
        DockWindowKind::new("BLUEPRINT", || flush_panel(screen()))
            .icon(Glyph::Signpost.image(ICON)),
    )
    .insert_resource(Editor::new())
    .add_plugins((
        canvas::plugin,
        menu::plugin,
        preview::plugin,
        split::plugin,
        files::plugin,
        wire::plugin,
    ));
}

/// A sheet that is not open, with its history.
struct Kept {
    sheet: Sheet,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

/// The book being edited: the open sheet with its history, the others
/// it may use as nodes, and what the open one last gave.
#[derive(Resource)]
pub(crate) struct Editor {
    /// The open sheet.
    pub sheet: Sheet,
    /// Its name in the book.
    pub name: String,
    others: BTreeMap<String, Kept>,
    /// Every graph of the book, for the ports of nodes that run one.
    pub rules: Ruleset,
    pub selected: Option<NodeId>,
    pub registry: Registry,
    pub seeds: Seeds,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// Changes whenever the nodes, the wires or the way an input is
    /// set change, or another sheet is opened, so what is built from
    /// them is built again.
    pub shape: u64,
    /// Changes with every change to the book, so it is saved again.
    pub revision: u64,
    /// What the open graph put down, or why it could not.
    pub outcome: Result<Vec<Instance>, String>,
}

impl Editor {
    fn new() -> Self {
        let mut editor = Self {
            sheet: Sheet::default(),
            name: String::new(),
            others: BTreeMap::new(),
            rules: Ruleset::default(),
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
            revision: 0,
            outcome: Ok(Vec::new()),
        };
        editor.load(example(START).unwrap_or_default());
        editor
    }

    /// The names of every sheet in the book, the open one included.
    pub fn names(&self) -> Vec<String> {
        let mut names =
            self.others.keys().cloned().collect::<Vec<_>>();
        names.push(self.name.clone());
        names.sort();
        names
    }

    /// Whether the graph `name` runs `target`, itself or through
    /// others.
    pub fn runs(&self, name: &str, target: &str) -> bool {
        if name == target {
            return true;
        }
        let Some(graph) = self.rules.graphs.get(name) else {
            return false;
        };
        graph.nodes.iter().any(|node| match &node.does {
            Does::Graph(child) => self.runs(child, target),
            Does::Kind(_) => false,
        })
    }

    /// Opens the sheet `name`, chosen directly.
    pub fn go_to(&mut self, name: &str) {
        self.open(name);
    }

    /// Opens the sheet `name`, keeping the open one with its history.
    fn open(&mut self, name: &str) {
        let Some(kept) = self.others.remove(name) else {
            return;
        };
        let was = Kept {
            sheet: std::mem::replace(&mut self.sheet, kept.sheet),
            undo: std::mem::replace(&mut self.undo, kept.undo),
            redo: std::mem::replace(&mut self.redo, kept.redo),
        };
        self.others.insert(
            std::mem::replace(&mut self.name, name.to_owned()),
            was,
        );
        self.selected = None;
        self.changed(true);
    }

    /// Adds an empty sheet to the book and opens it.
    pub fn new_graph(&mut self) {
        let names = self.names();
        let name = (1..)
            .map(|n| format!("graph {n}"))
            .find(|name| !names.contains(name))
            .unwrap_or_default();
        self.others.insert(
            name.clone(),
            Kept {
                sheet: Sheet::default(),
                undo: Vec::new(),
                redo: Vec::new(),
            },
        );
        self.open(&name);
    }

    /// The book as it is now, its entry the open sheet.
    pub fn book(&self) -> Book {
        Book {
            entry: self.name.clone(),
            sheets: self
                .others
                .iter()
                .map(|(name, kept)| {
                    (name.clone(), kept.sheet.clone())
                })
                .chain([(self.name.clone(), self.sheet.clone())])
                .collect(),
        }
    }

    /// Puts `book` in place of the one being edited, with no history,
    /// open at its entry.
    pub fn load(&mut self, mut book: Book) {
        let entry =
            book.sheets.remove(&book.entry).unwrap_or_default();
        self.others = book
            .sheets
            .into_iter()
            .map(|(name, sheet)| {
                let kept = Kept {
                    sheet,
                    undo: Vec::new(),
                    redo: Vec::new(),
                };
                (name, kept)
            })
            .collect();
        self.sheet = entry;
        self.name = book.entry;
        self.selected = None;
        self.undo.clear();
        self.redo.clear();
        self.changed(true);
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
        ) || matches!(
            (&inverse, self.undo.last()),
            (
                Edit::ChangeInput { name, .. },
                Some(Edit::ChangeInput { name: last, .. }),
            ) if name == last
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
        self.revision += 1;
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
        self.revision += 1;
        if self
            .selected
            .is_some_and(|node| self.sheet.graph.node(node).is_none())
        {
            self.selected = None;
        }
        self.rules = Ruleset {
            entry: self.name.clone(),
            graphs: self
                .others
                .iter()
                .map(|(name, kept)| {
                    (name.clone(), kept.sheet.graph.clone())
                })
                .chain([(
                    self.name.clone(),
                    self.sheet.graph.clone(),
                )])
                .collect(),
        };
        self.run();
    }

    /// Runs the open graph with every output nothing is wired to
    /// shown too, so a graph is seen as it is built. Its inputs take
    /// their defaults.
    fn run(&mut self) {
        let mut graph = self.sheet.graph.clone();
        for node in &self.sheet.graph.nodes {
            let Ok((_, outputs)) = hamlet_blueprint::ports(
                &self.registry,
                &self.rules,
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
        let mut ruleset = self.rules.clone();
        ruleset.graphs.insert(self.name.clone(), graph);
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

    /// Adds a node that does `does`, with its card at `at`.
    pub fn add(&mut self, does: Does, at: Vec2) {
        let id = self.sheet.graph.next_id();
        self.apply(Edit::AddNode {
            node: Node {
                id,
                does,
                settings: BTreeMap::new(),
            },
            at: Some(at),
        });
        self.selected = Some(id);
    }

    /// Makes the first of `node`'s outputs that puts something down
    /// an output of the open graph, named after the port.
    pub fn expose_output(&mut self, node: NodeId) {
        let Some(found) = self.sheet.graph.node(node) else {
            return;
        };
        let Ok((_, outputs)) = hamlet_blueprint::ports(
            &self.registry,
            &self.rules,
            found,
        ) else {
            return;
        };
        let Some(port) = outputs.into_iter().find(|port| {
            matches!(
                port.value_type,
                ValueType::Geometry | ValueType::Placements
            )
        }) else {
            return;
        };
        let name = self.free_name(&port.name, |graph| {
            graph
                .outputs
                .iter()
                .map(|output| output.name.clone())
                .collect()
        });
        self.apply(Edit::AddOutput(GraphOutput {
            name,
            value_type: port.value_type,
            from: Port {
                node,
                name: port.name,
            },
        }));
    }

    /// Sets `input` of `node` from an input of the open graph that
    /// takes `value_type`, giving the graph one named after the port,
    /// with `default`, when it has none.
    pub fn expose_input(
        &mut self,
        node: NodeId,
        input: &str,
        value_type: ValueType,
        default: Option<Constant>,
    ) {
        let found = self
            .sheet
            .graph
            .inputs
            .iter()
            .find(|graph_input| {
                graph_input.name == input
                    && value_type.accepts(graph_input.value_type)
            })
            .map(|graph_input| graph_input.name.clone());
        let mut edits = Vec::new();
        let name = match found {
            Some(name) => name,
            None => {
                let name = self.free_name(input, |graph| {
                    graph
                        .inputs
                        .iter()
                        .map(|input| input.name.clone())
                        .collect()
                });
                edits.push(Edit::AddInput(GraphInput {
                    name: name.clone(),
                    value_type,
                    default,
                    vary: None,
                }));
                name
            }
        };
        edits.push(Edit::Set {
            node,
            input: input.to_owned(),
            setting: Some(Setting::FromParent(name)),
        });
        self.apply(Edit::Batch(edits));
    }

    /// `wanted`, or it with a number after it, unused among `taken`.
    fn free_name(
        &self,
        wanted: &str,
        taken: impl Fn(&hamlet_blueprint::Graph) -> Vec<String>,
    ) -> String {
        let taken = taken(&self.sheet.graph);
        std::iter::once(wanted.to_owned())
            .chain((2..).map(|n| format!("{wanted} {n}")))
            .find(|name| !taken.contains(name))
            .unwrap_or_default()
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
        // A new value is drawn where it is; a new name, type or way
        // of varying changes what is drawn.
        Edit::ChangeInput { name, input } => sheet
            .graph
            .inputs
            .iter()
            .find(|old| old.name == *name)
            .is_none_or(|old| {
                old.name != input.name
                    || old.value_type != input.value_type
                    || old.vary.is_some() != input.vary.is_some()
            }),
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

/// The Graph panel: its toolbar over the canvas, with the
/// properties floating over the canvas's corner, and the preview
/// beside it, sized by dragging the line between them. The canvas and
/// the preview fill their sides edge to edge.
fn screen() -> impl View {
    let preview = column((preview::view(),))
        .gap(0.0)
        .fills_height()
        .width(resource::<Layout, _>(|layout| px(layout.preview)))
        .shrink(0.0);
    column((
        tools(),
        with_cx(|cx| hairline(cx.theme().palette.seam)),
        row((
            // The properties are placed over the canvas, in its
            // frame.
            column((canvas::view(), dock_outline(), properties()))
                .fills_width()
                .fills_height()
                .min_width(px(0.0))
                .gap(0.0),
            divider(),
            preview,
        ))
        .width(percent(100.0))
        .fills_height()
        .gap(0.0),
        menu::view(),
    ))
    .fills_width()
    .fills_height()
    .gap(0.0)
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
        // Open, the chevron points up to fold; folded, it turns to
        // point on.
        let fold = button(
            glyph(Glyph::Chevron, ICON)
                .rotation(resource::<Layout, _>(|layout| {
                    if layout.properties { 0.0 } else { 90.0 }
                }))
                .transition(Motion::Interact),
        )
        .rules(icon_button)
        .tooltip(|| note("Fold or open the properties"))
        .on_activate(|world| {
            let mut layout = world.resource_mut::<Layout>();
            layout.properties = !layout.properties;
        });
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

/// Tools side by side on one plate, apart from the next plate.
fn cluster(
    tools: impl ViewSeq<Bevy, HamletTheme> + 'static,
) -> impl View {
    with_cx(move |cx| {
        let theme = cx.theme();
        row(tools)
            .gap(2.0)
            .padding(UiRect::all(px(3.0)))
            .align(AlignItems::Center)
            .fill(theme.palette.well)
            .radius(theme.space.round)
    })
}

/// The bar over the canvas: plates of tools for editing, for the
/// graphs of the book and for its file, and what is known of both
/// at the end.
fn tools() -> impl View {
    with_cx(|cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        row((
            cluster((
                tool(Glyph::Undo, "Undo (Cmd+Z)", |world| {
                    world.resource_mut::<Editor>().undo();
                }),
                tool(Glyph::Redo, "Redo (Shift+Cmd+Z)", |world| {
                    world.resource_mut::<Editor>().redo();
                }),
            )),
            cluster((
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
            )),
            cluster((switcher(),)),
            cluster((
                tool(
                    Glyph::Folder,
                    "Open a graph book (Cmd+O)",
                    files::open,
                ),
                tool(
                    Glyph::Save,
                    "Save the graph book (Cmd+S; Shift for Save as)",
                    files::save,
                ),
                dropdown(
                    EXAMPLES.map(|(name, _)| name),
                    usize::MAX,
                    Glyph::Chevron.image(ICON),
                    |world: &mut World, at| {
                        let Some(book) = example(EXAMPLES[at].1)
                        else {
                            return;
                        };
                        world.resource_mut::<Editor>().load(book);
                        canvas::fit(world);
                    },
                )
                .placeholder("Open an example")
                .width(px(190.0)),
            )),
            frame().grow(1.0),
            description(resource::<files::Files, _>(|files| {
                if files.note.is_empty() {
                    files.name()
                } else {
                    files.note.clone()
                }
            })),
            description(resource::<Editor, _>(
                |editor| match &editor.outcome {
                    Ok(instances) => {
                        format!("{} put down", instances.len())
                    }
                    Err(problem) => problem.clone(),
                },
            )),
        ))
        .width(percent(100.0))
        .min_width(px(0.0))
        .overflow(Overflow::clip())
        .padding(UiRect::all(px(gap)))
        .gap(gap)
        .align(AlignItems::Center)
        .fill(theme.palette.card)
    })
}

/// The graph open, chosen from every graph of the book, and a way to
/// start a new one.
fn switcher() -> impl View {
    keyed(
        resource::<Editor, _>(|editor| {
            (editor.names(), editor.name.clone())
        }),
        |(names, open)| {
            let at = names
                .iter()
                .position(|name| name == open)
                .unwrap_or(0);
            let mut options = names.clone();
            options.push("New graph".to_owned());
            let names = names.clone();
            dropdown(
                options,
                at,
                Glyph::Chevron.image(ICON),
                move |world: &mut World, chosen| {
                    let mut editor = world.resource_mut::<Editor>();
                    match names.get(chosen) {
                        Some(name) => editor.go_to(name),
                        None => editor.new_graph(),
                    }
                    canvas::fit(world);
                },
            )
            .width(px(150.0))
            .boxed()
        },
    )
    .within(row(()).gap(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_example_opens_and_puts_something_down() {
        let mut editor = Editor::new();
        for (name, text) in EXAMPLES {
            let book = example(text)
                .unwrap_or_else(|| panic!("{name} does not parse"));
            editor.load(book);
            // Each graph also runs alone, its inputs at their
            // defaults.
            for graph in editor.names() {
                editor.go_to(&graph);
                match &editor.outcome {
                    Ok(instances) => {
                        assert!(
                            !instances.is_empty(),
                            "{name}: {graph}"
                        );
                    }
                    Err(problem) => {
                        panic!("{name}: {graph}: {problem}")
                    }
                }
            }
            match &editor.outcome {
                Ok(instances) => {
                    assert!(!instances.is_empty(), "{name}");
                }
                Err(problem) => panic!("{name}: {problem}"),
            }
        }
    }
}
