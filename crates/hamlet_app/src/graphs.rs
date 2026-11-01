//! The graph editor: one panel holding the canvas with its tools, and
//! beside it two inner panels, the preview of what the graph puts
//! down, run again on every change, and the selected node's
//! properties.

mod assets;
mod blueprint;
mod canvas;
mod controls;
mod files;
mod flavor;
mod gizmo;
mod handles;
mod material;
mod menu;
mod overlay;
mod params;
mod preview;
mod split;
mod wire;

use std::collections::{BTreeMap, BTreeSet};
use std::mem::discriminant;

use bevy::app::App;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::ui::{AlignItems, Display, Overflow, UiRect, percent, px};
use bevy::ui_render::UiMaterialPlugin;
use bevy_fynix::dock::DockWindowKind;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, row,
};
use bevy_fynix::{
    Bevy, ScopedExt, ViewExt, ViewSeq, keyed, resource,
};
use hamlet_blueprint::{
    Book, Catalogue, Constant, Does, Edit, GraphInput, GraphOutput,
    GraphProblem, Group, Initial, Node, NodeId, Port, PortSpec,
    Registry, Ruleset, Setting, Sheet, ValueType,
};
use hamlet_ui::styles::{
    CONTROL, Fills, ICON, description, hairline, note, raised,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, View, styles, with_cx};

pub(crate) use self::blueprint::Blueprint;
use self::flavor::Flavor;
pub(crate) use self::material::Material;
use self::split::{Layout, Pane, divider, floating, outline};
use crate::dock::{PanelAppExt, flush_panel};
use crate::library::{Item, Kind, Source};

/// The book an example's text holds.
fn example(text: &str) -> Option<Book> {
    Book::from_ron(text).ok().or_else(|| {
        Sheet::from_ron(text)
            .ok()
            .map(|sheet| Book::of("main", sheet))
    })
}

/// Adds the editor for `F`: its dock panel, its book and what the
/// editing needs, and whatever `F` adds itself.
pub(crate) fn plugin<F: Flavor>(app: &mut App) {
    app.add_panel(
        F::PANEL,
        DockWindowKind::new(F::TITLE, || flush_panel(screen::<F>()))
            .icon(F::ICON.image(ICON)),
    )
    .insert_resource(Editor::<F>::new())
    .add_plugins((
        canvas::plugin::<F>,
        menu::plugin::<F>,
        split::plugin::<F>,
        files::plugin::<F>,
    ));
    // The wires' material is shared by every editor.
    if !app.is_plugin_added::<UiMaterialPlugin<wire::WireMaterial>>()
    {
        app.add_plugins(wire::plugin);
    }
    F::plugin(app);
}

/// The blueprint examples, for the library to list.
pub(crate) fn blueprint_examples()
-> &'static [(&'static str, &'static str)] {
    Blueprint::EXAMPLES
}

/// The material examples, for the library to list.
pub(crate) fn material_examples()
-> &'static [(&'static str, &'static str)] {
    Material::EXAMPLES
}

pub(crate) use self::assets::add_mesh;

/// Opens the library's `item` in the editor of its kind: a book in
/// the blueprint or material editor, saved back to its file. A mesh
/// or a texture is not opened; it is dragged onto a canvas.
pub(crate) fn open_item(
    world: &mut World,
    item: &Item,
) -> Result<(), String> {
    match item.kind {
        Kind::Blueprint => open_book::<Blueprint>(world, item),
        Kind::Material => open_book::<Material>(world, item),
        Kind::Mesh | Kind::Texture => {
            Err(format!("Drag {} onto a canvas to use it", item.name))
        }
    }
}

fn open_book<F: Flavor>(
    world: &mut World,
    item: &Item,
) -> Result<(), String> {
    let (book, path) = match &item.source {
        Source::Example(text) => (example(text), None),
        Source::File(path) => (
            std::fs::read_to_string(path)
                .ok()
                .and_then(|text| example(&text)),
            Some(path.clone()),
        ),
        Source::BuiltIn => (None, None),
    };
    let book =
        book.ok_or_else(|| format!("{} is not a book", item.name))?;
    world.resource_mut::<Editor<F>>().load(book);
    world.resource_mut::<files::Files<F>>().path = path;
    canvas::fit::<F>(world);
    Ok(())
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
pub(crate) struct Editor<F: Flavor> {
    /// The open sheet.
    pub sheet: Sheet,
    /// Its name in the book.
    pub name: String,
    others: BTreeMap<String, Kept>,
    /// Every graph of the book, for the ports of nodes that run one,
    /// and the assets the book adds.
    pub rules: Ruleset,
    /// The nodes picked to move, copy or delete together.
    pub selected: BTreeSet<NodeId>,
    pub registry: Registry,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// Changes whenever the nodes, the wires or the way an input is
    /// set change, or another sheet is opened, so what is built from
    /// them is built again.
    pub shape: u64,
    /// Changes with every change to the book, so it is saved again.
    pub revision: u64,
    /// The nodes that failed in the last run, each with its problem;
    /// the rest of the graph ran without them.
    pub problems: Vec<GraphProblem>,
    /// What the flavor keeps of the last run.
    pub got: F::Got,
    /// The meshes the book last opened carried with it, not yet
    /// moved into the library.
    pub carried: Catalogue,
}

impl<F: Flavor> Editor<F> {
    fn new() -> Self {
        let mut editor = Self {
            sheet: Sheet::default(),
            name: String::new(),
            others: BTreeMap::new(),
            rules: Ruleset::default(),
            selected: BTreeSet::new(),
            registry: F::registry(),
            undo: Vec::new(),
            redo: Vec::new(),
            shape: 0,
            revision: 0,
            problems: Vec::new(),
            got: F::Got::default(),
            carried: Catalogue::default(),
        };
        let start = F::EXAMPLES
            .first()
            .and_then(|(_, text)| example(text))
            .unwrap_or_default();
        editor.load(start);
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
        self.selected.clear();
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

    /// Takes the open sheet out of the book and opens another, or
    /// says why it stays: it is the only one, or another graph runs
    /// it.
    pub fn delete_graph(&mut self) -> Result<(), String> {
        if let Some(user) =
            self.others.iter().find_map(|(name, kept)| {
                kept.sheet
                    .graph
                    .nodes
                    .iter()
                    .any(|node| {
                        node.does == Does::Graph(self.name.clone())
                    })
                    .then_some(name)
            })
        {
            return Err(format!("{user} uses {}", self.name));
        }
        let Some(next) = self.others.keys().next().cloned() else {
            return Err("The only graph cannot be deleted".to_owned());
        };
        let Some(kept) = self.others.remove(&next) else {
            return Ok(());
        };
        self.sheet = kept.sheet;
        self.undo = kept.undo;
        self.redo = kept.redo;
        self.name = next;
        self.selected.clear();
        self.changed(true);
        Ok(())
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
            // The meshes it uses are the library's.
            assets: Catalogue::default(),
        }
    }

    /// Puts `book` in place of the one being edited, with no history,
    /// open at its entry.
    pub fn load(&mut self, mut book: Book) {
        // A book saved before removing an input cleared the nodes set
        // from it may still name it.
        for sheet in book.sheets.values_mut() {
            let graph = &mut sheet.graph;
            let names: BTreeSet<String> = graph
                .inputs
                .iter()
                .map(|input| input.name.clone())
                .collect();
            for node in &mut graph.nodes {
                node.settings.retain(|_, setting| match setting {
                    Setting::FromParent(name) => names.contains(name),
                    _ => true,
                });
            }
        }
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
        self.carried = book.assets;
        self.selected.clear();
        self.undo.clear();
        self.redo.clear();
        self.changed(true);
    }

    /// Applies `edit` and keeps the edit that undoes it. Edits of one
    /// input in a row, as a number dragged, are undone as one.
    pub fn apply(&mut self, edit: Edit) {
        let reshapes = reshapes(&self.sheet, &self.registry, &edit);
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
        ) || matches!(
            (&inverse, self.undo.last()),
            (
                Edit::ChangeGroup { at, group },
                Some(Edit::ChangeGroup { at: last, group: was }),
            ) if at == last && group.nodes == was.nodes
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
    /// Keeps the moves of a finished drag, each node with where it
    /// began, as one step.
    pub fn dropped(&mut self, moved: Vec<(NodeId, Vec2)>) {
        self.undo.push(Edit::Batch(
            moved
                .into_iter()
                .map(|(node, from)| Edit::Move {
                    node,
                    to: Some(from),
                })
                .collect(),
        ));
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
        let graph = &self.sheet.graph;
        self.selected.retain(|node| graph.node(*node).is_some());
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
            assets: std::mem::take(&mut self.rules.assets),
        };
        // A sheet with no places, as an example, is laid out once.
        if self.sheet.layout.is_empty() {
            self.sheet.layout = canvas::arrange(self);
        }
        F::run(self);
    }

    /// Adds a node that does `does`, set by `settings`, with its card
    /// at `at`.
    pub fn add(
        &mut self,
        does: Does,
        settings: BTreeMap<String, Setting>,
        at: Vec2,
    ) {
        let id = self.sheet.graph.next_id();
        self.apply(Edit::AddNode {
            node: Node { id, does, settings },
            at: Some(at),
        });
        self.selected = BTreeSet::from([id]);
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
        let Some(port) = outputs
            .into_iter()
            .find(|port| port.value_type == ValueType::Objects)
        else {
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

    /// Adds a copy of each selected node, a little below and to the
    /// right, with the wires between them, and selects the copies.
    pub fn duplicate(&mut self) {
        let mut next = self.sheet.graph.next_id();
        let mut copies = BTreeMap::new();
        let mut edits = Vec::new();
        for node in &self.selected {
            let Some(original) =
                self.sheet.graph.node(*node).cloned()
            else {
                continue;
            };
            let at = self
                .sheet
                .layout
                .get(node)
                .copied()
                .unwrap_or_default();
            copies.insert(*node, next);
            edits.push(Edit::AddNode {
                node: Node {
                    id: next,
                    ..original
                },
                at: Some(at + Vec2::splat(28.0)),
            });
            next = NodeId(next.0 + 1);
        }
        edits.extend(self.sheet.graph.wires.iter().filter_map(
            |wire| {
                let from = copies.get(&wire.from.node)?;
                let to = copies.get(&wire.to.node)?;
                let mut wire = wire.clone();
                wire.from.node = *from;
                wire.to.node = *to;
                Some(Edit::Connect(wire))
            },
        ));
        if edits.is_empty() {
            return;
        }
        self.apply(Edit::Batch(edits));
        self.selected = copies.into_values().collect();
    }

    /// Deletes every selected node, and each group left empty.
    pub fn remove(&mut self) {
        let mut edits: Vec<Edit> = self
            .selected
            .iter()
            .copied()
            .map(Edit::RemoveNode)
            .collect();
        if edits.is_empty() {
            return;
        }
        edits.extend(
            self.leave().1.into_iter().rev().map(Edit::RemoveGroup),
        );
        self.apply(Edit::Batch(edits));
    }

    /// Takes `catalogue` as the meshes the graphs may put down.
    pub fn set_catalogue(&mut self, catalogue: Catalogue) {
        if self.rules.assets != catalogue {
            self.rules.assets = catalogue;
            self.changed(true);
        }
    }

    /// Frames the selected nodes in a new group, out of any group
    /// they were in.
    pub fn group(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let (mut edits, emptied) = self.leave();
        let at = self.sheet.groups.len() - emptied.len();
        edits
            .extend(emptied.into_iter().rev().map(Edit::RemoveGroup));
        edits.push(Edit::AddGroup {
            at,
            group: Group {
                title: "Group".to_owned(),
                nodes: self.selected.clone(),
            },
        });
        self.apply(Edit::Batch(edits));
    }

    /// Takes the selected nodes out of their groups.
    pub fn ungroup_selected(&mut self) {
        let (mut edits, emptied) = self.leave();
        edits
            .extend(emptied.into_iter().rev().map(Edit::RemoveGroup));
        if !edits.is_empty() {
            self.apply(Edit::Batch(edits));
        }
    }

    /// The changes that take the selected nodes out of their groups,
    /// and the groups that leaves empty, in order, to remove after.
    fn leave(&self) -> (Vec<Edit>, Vec<usize>) {
        let mut edits = Vec::new();
        let mut emptied = Vec::new();
        for (at, group) in self.sheet.groups.iter().enumerate() {
            if group.nodes.is_disjoint(&self.selected) {
                continue;
            }
            let nodes: BTreeSet<NodeId> = group
                .nodes
                .difference(&self.selected)
                .copied()
                .collect();
            if nodes.is_empty() {
                emptied.push(at);
            } else {
                edits.push(Edit::ChangeGroup {
                    at,
                    group: Group {
                        title: group.title.clone(),
                        nodes,
                    },
                });
            }
        }
        (edits, emptied)
    }

    /// Whether any selected node is in a group.
    pub fn any_grouped(&self) -> bool {
        self.sheet
            .groups
            .iter()
            .any(|group| !group.nodes.is_disjoint(&self.selected))
    }

    pub fn ungroup(&mut self, at: usize) {
        self.apply(Edit::RemoveGroup(at));
    }

    pub fn retitle(&mut self, at: usize, title: String) {
        let Some(group) = self.sheet.groups.get(at) else {
            return;
        };
        if group.title != title {
            let nodes = group.nodes.clone();
            self.apply(Edit::ChangeGroup {
                at,
                group: Group { title, nodes },
            });
        }
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

/// The input that names `node`, as math's operation does, and the
/// choice it is set to.
pub(super) fn naming<'a>(
    registry: &'a Registry,
    node: &Node,
) -> Option<(&'a PortSpec, String)> {
    let Does::Kind(kind) = &node.does else {
        return None;
    };
    let spec =
        registry.get(kind)?.inputs.iter().find(|spec| spec.names)?;
    let chosen = match node.settings.get(spec.name) {
        Some(Setting::Fixed(Constant::Choice(choice))) => {
            choice.clone()
        }
        _ => match spec.initial {
            Some(Initial::Choice(choice)) => choice.to_owned(),
            _ => (*spec.choices.first()?).to_owned(),
        },
    };
    Some((spec, chosen))
}

/// Whether `edit` changes what the canvas or the parameters are built
/// from, rather than only a value in them.
fn reshapes(sheet: &Sheet, registry: &Registry, edit: &Edit) -> bool {
    match edit {
        Edit::Set {
            node,
            input,
            setting,
        } => {
            let found = sheet.graph.node(*node);
            // The input that names a node changes its title and rows.
            if found
                .and_then(|node| naming(registry, node))
                .is_some_and(|(spec, _)| spec.name == input)
            {
                return true;
            }
            let old = found.and_then(|node| node.settings.get(input));
            // A default and a fixed value show the same controls.
            let kind = |setting: Option<&Setting>| match setting {
                None | Some(Setting::Fixed(_)) => None,
                Some(other) => Some(discriminant(other)),
            };
            kind(old) != kind(setting.as_ref())
        }
        Edit::Move { .. } => false,
        // A title is drawn where it is.
        Edit::ChangeGroup { at, group } => sheet
            .groups
            .get(*at)
            .is_none_or(|old| old.nodes != group.nodes),
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

/// The Graph panel: its toolbar over the canvas, with the parameters
/// floating over the canvas's corner, and the preview floating there
/// too or beside it, sized by dragging the line between them. The
/// canvas and the preview fill their sides edge to edge.
fn screen<F: Flavor>() -> impl View {
    let parameters = floating::<F>(
        Pane::Parameters,
        Glyph::Sliders,
        "PARAMETERS",
        params::view::<F>().boxed(),
    );
    let floating_preview = if F::PREVIEW_FLOATS {
        floating::<F>(Pane::Preview, F::ICON, "PREVIEW", F::preview())
    } else {
        frame().display(Display::None).boxed()
    };
    // The cards are placed over the canvas, in its frame.
    let over = column((
        canvas::view::<F>(),
        outline::<F>(),
        parameters,
        floating_preview,
    ))
    .fills_width()
    .fills_height()
    .min_width(px(0.0))
    .gap(0.0);
    let body: AnyView = if F::PREVIEW_FLOATS {
        row((over,)).boxed()
    } else {
        // Not `fills_height`: its grow would be along the row,
        // sharing the width with the canvas.
        let preview = column((F::preview(),))
            .gap(0.0)
            .min_height(px(0.0))
            .width(resource::<Layout<F>, _>(|layout| {
                px(layout.preview)
            }))
            .shrink(0.0);
        row((over, divider::<F>(), preview)).boxed()
    };
    column((
        tools::<F>(),
        with_cx(|cx| hairline(cx.theme().palette.border)),
        column((body,))
            .width(percent(100.0))
            .fills_height()
            .gap(0.0),
        menu::view::<F>(),
    ))
    .fills_width()
    .fills_height()
    .gap(0.0)
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
        .rules(styles::button)
        .tagged(Variant::Default)
        .on_activate(act)
}

/// Tools side by side, set apart from the next group.
fn cluster(
    tools: impl ViewSeq<Bevy, HamletTheme> + 'static,
) -> impl View {
    row(tools).gap(2.0).align(AlignItems::Center)
}

/// The bar over the canvas: groups of tools for editing, for the
/// graphs of the book and for its file, and what is known of both
/// at the end.
fn tools<F: Flavor>() -> impl View {
    with_cx(|cx| {
        let theme = cx.theme();
        let gap = theme.space.gap;
        let own = F::tools();
        let own_shown = if own.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        row((
            cluster((
                tool(Glyph::Undo, "Undo (Cmd+Z)", |world| {
                    world.resource_mut::<Editor<F>>().undo();
                }),
                tool(Glyph::Redo, "Redo (Shift+Cmd+Z)", |world| {
                    world.resource_mut::<Editor<F>>().redo();
                }),
            )),
            cluster((tool(
                Glyph::Aim,
                "Fit every node in sight (F)",
                canvas::fit::<F>,
            ),)),
            cluster((
                switcher::<F>(),
                tool(Glyph::Plus, "New graph", |world| {
                    world.resource_mut::<Editor<F>>().new_graph();
                    canvas::fit::<F>(world);
                }),
                tool(Glyph::Delete, "Delete this graph", |world| {
                    let deleted = world
                        .resource_mut::<Editor<F>>()
                        .delete_graph();
                    world.resource_mut::<files::Files<F>>().note =
                        match deleted {
                            Ok(()) => String::new(),
                            Err(why) => why,
                        };
                    canvas::fit::<F>(world);
                }),
            )),
            cluster((
                tool(
                    Glyph::Folder,
                    "Open a graph book (Cmd+O)",
                    files::open::<F>,
                ),
                tool(
                    Glyph::Save,
                    "Save the graph book (Cmd+S; Shift for Save as)",
                    files::save::<F>,
                ),
                dropdown(
                    F::EXAMPLES.iter().map(|(name, _)| *name),
                    usize::MAX,
                    Glyph::Chevron.image(ICON),
                    |world: &mut World, at| {
                        let Some(book) = example(F::EXAMPLES[at].1)
                        else {
                            return;
                        };
                        world.resource_mut::<Editor<F>>().load(book);
                        canvas::fit::<F>(world);
                    },
                )
                .placeholder("Open an example")
                .width(px(190.0)),
            )),
            row(own)
                .gap(2.0)
                .align(AlignItems::Center)
                .display(own_shown),
            frame().grow(1.0),
            description(resource::<files::Files<F>, _>(|files| {
                if files.note.is_empty() {
                    files.name()
                } else {
                    files.note.clone()
                }
            })),
            description(resource::<Editor<F>, _>(F::status)),
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

/// The graph open, chosen from every graph of the book.
fn switcher<F: Flavor>() -> impl View {
    keyed(
        resource::<Editor<F>, _>(|editor| {
            (editor.names(), editor.name.clone())
        }),
        |(names, open)| {
            let at = names
                .iter()
                .position(|name| name == open)
                .unwrap_or(0);
            let options = names.clone();
            let names = names.clone();
            dropdown(
                options,
                at,
                Glyph::Chevron.image(ICON),
                move |world: &mut World, chosen| {
                    if let Some(name) = names.get(chosen) {
                        world.resource_mut::<Editor<F>>().go_to(name);
                    }
                    canvas::fit::<F>(world);
                },
            )
            .width(px(150.0))
            .boxed()
        },
    )
    .within(row(()).gap(0.0))
}
