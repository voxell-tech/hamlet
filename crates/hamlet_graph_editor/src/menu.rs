//! The graph's menus and keys. A right click on the canvas, or Space,
//! opens the groups of what can be added, each a page of its own;
//! its search field, which takes the keyboard focus while it is open,
//! searches every group, and Enter adds the first found. A right
//! click on a card opens the actions on it.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::math::{Rect, Vec2};
use bevy::picking::Pickable;
use bevy::text::{EditableText, TextEdit, TextEditChange};
use bevy::ui::{
    AlignItems, ComputedNode, Display, PositionType,
    UiGlobalTransform, UiRect, Val, px,
};
use bevy::window::Window;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, column, frame, label,
    menu_item, overlay, popup, row, scroll, text_field,
};
use bevy_fynix::{ScopedExt, Signal, ViewExt, keyed, resource};
use hamlet_graph::{Constant, Does, Edit, NodeId, Setting};
use hamlet_ui::styles::{ICON, note, section, text_input};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View};

use super::canvas::Input;
use super::flavor::Flavor;
use super::{Editor, canvas, controls, files};

/// The group the book's other graphs are listed under.
const GRAPHS: &str = "Graphs";

/// The groups in the order they are listed, before any others.
const GROUPS: [&str; 7] = [
    "Objects",
    "Transforms",
    "Modifiers",
    "Curves",
    "Fields",
    "Math",
    GRAPHS,
];

/// The most a long list in a menu grows before it scrolls.
const TALLEST: f32 = 320.0;
/// The room the caret takes before the search's hint.
const CARET: f32 = 4.0;

pub(super) fn plugin<F: Flavor>(app: &mut App) {
    app.init_resource::<Menu<F>>()
        .init_resource::<Search<F>>()
        .add_systems(Update, keys::<F>);
}

/// The menu open over one editor's graph.
#[derive(Resource)]
pub(super) struct Menu<F: Flavor>(Which, PhantomData<F>);

impl<F: Flavor> Default for Menu<F> {
    fn default() -> Self {
        Self(Which::Closed, PhantomData)
    }
}

impl<F: Flavor> Deref for Menu<F> {
    type Target = Which;

    fn deref(&self) -> &Which {
        &self.0
    }
}

impl<F: Flavor> DerefMut for Menu<F> {
    fn deref_mut(&mut self) -> &mut Which {
        &mut self.0
    }
}

/// The menu open over the graph, at its place in the window.
#[derive(Default, Clone, PartialEq, Debug)]
pub(super) enum Which {
    #[default]
    Closed,
    /// The list of kinds, adding a card at `on_sheet`.
    Create { at: Vec2, on_sheet: Vec2 },
    /// The actions on a card.
    Card { node: NodeId, at: Vec2 },
    /// The actions on the group at `group`.
    Group { group: usize, at: Vec2 },
    /// The ways to set an input of a card.
    Input {
        node: NodeId,
        input: Input,
        at: Vec2,
    },
    /// The choices an input of a card can take.
    Choice {
        node: NodeId,
        input: String,
        choices: Vec<String>,
        at: Vec2,
    },
}

/// Where the list of kinds stands: what is typed, and the group open,
/// none for the list of groups.
#[derive(Default, Clone, PartialEq)]
struct Lookup {
    typed: String,
    group: Option<&'static str>,
}

/// The lookup in one editor's list of kinds.
#[derive(Resource)]
struct Search<F: Flavor>(Lookup, PhantomData<F>);

impl<F: Flavor> Default for Search<F> {
    fn default() -> Self {
        Self(Lookup::default(), PhantomData)
    }
}

impl<F: Flavor> Deref for Search<F> {
    type Target = Lookup;

    fn deref(&self) -> &Lookup {
        &self.0
    }
}

impl<F: Flavor> DerefMut for Search<F> {
    fn deref_mut(&mut self) -> &mut Lookup {
        &mut self.0
    }
}

/// On the search field of the list of kinds.
#[derive(Component)]
struct SearchField<F: Flavor>(PhantomData<F>);

/// On the layer the menus are placed in.
#[derive(Component)]
struct Layer<F: Flavor>(PhantomData<F>);

/// One row of the list of what can be added.
#[derive(Clone)]
struct Entry {
    group: &'static str,
    name: String,
    doc: String,
    does: Does,
    /// How the node starts set, as by the choice that names it.
    settings: BTreeMap<String, Setting>,
}

/// Opens the list of kinds at the pointer, adding where it is on the
/// canvas, or in the middle of what is in sight when it is elsewhere.
pub(super) fn create_at_pointer<F: Flavor>(world: &mut World) {
    let at = world
        .query::<&Window>()
        .iter(world)
        .find_map(Window::cursor_position)
        .unwrap_or_default();
    let on_sheet = canvas::on_sheet::<F>(world, at)
        .unwrap_or_else(|| canvas::middle::<F>(world));
    open::<F>(world, Which::Create { at, on_sheet });
}

pub(super) fn open<F: Flavor>(world: &mut World, menu: Which) {
    **world.resource_mut::<Search<F>>() = Lookup::default();
    **world.resource_mut::<Menu<F>>() = menu;
}

fn close<F: Flavor>(world: &mut World) {
    **world.resource_mut::<Menu<F>>() = Which::Closed;
}

/// Where the window's point `at` is in the layer the menus are
/// placed in.
fn in_layer<F: Flavor>(world: &mut World, at: Vec2) -> Vec2 {
    world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<Layer<F>>>()
        .iter(world)
        .next()
        .map_or(at, |(node, transform)| {
            at - canvas::corner(node, transform)
        })
}

/// Whether `search` matches the name or the group of a kind.
fn is_found(group: &str, name: &str, search: &str) -> bool {
    let search = search.trim().to_lowercase().replace(' ', "_");
    name.replace(' ', "_").to_lowercase().contains(&search)
        || group.to_lowercase().contains(&search)
}

/// Everything that can be added, by group. A graph that runs the
/// open one is left out, as it would run itself.
fn entries<F: Flavor>(editor: &Editor<F>) -> Vec<Entry> {
    let rank = |group: &str| {
        GROUPS
            .iter()
            .position(|g| *g == group)
            .unwrap_or(GROUPS.len())
    };
    // A kind named by one of its inputs is listed once for each
    // choice of it.
    let kinds = editor.registry.kinds().flat_map(|(group, kind)| {
        let entry = |name: &str, settings| Entry {
            group,
            name: name.to_owned(),
            doc: kind.doc.to_owned(),
            does: Does::Kind(kind.name.to_owned()),
            settings,
        };
        match kind.inputs.iter().find(|spec| spec.names) {
            Some(spec) => spec
                .choices
                .iter()
                .map(|choice| {
                    let set = Setting::Fixed(Constant::Choice(
                        (*choice).to_owned(),
                    ));
                    entry(
                        choice,
                        BTreeMap::from([(spec.name.to_owned(), set)]),
                    )
                })
                .collect(),
            None => vec![entry(kind.name, BTreeMap::new())],
        }
    });
    let graphs = editor
        .names()
        .into_iter()
        .filter(|name| !editor.runs(name, &editor.name))
        .map(|name| Entry {
            group: GRAPHS,
            doc: format!("The graph {name} of this book"),
            does: Does::Graph(name.clone()),
            settings: BTreeMap::new(),
            name,
        });
    let mut found = graphs.chain(kinds).collect::<Vec<_>>();
    found.sort_by(|a, b| {
        (rank(a.group), a.group, &a.name).cmp(&(
            rank(b.group),
            b.group,
            &b.name,
        ))
    });
    found
}

/// Whether the kind `name` of `group` is in the list where it
/// stands: found by what is typed, else in the open group.
fn is_listed(
    group: &'static str,
    name: &str,
    search: &Lookup,
) -> bool {
    if search.typed.trim().is_empty() {
        search.group == Some(group)
    } else {
        is_found(group, name, &search.typed)
    }
}

/// The entries the list shows, in order.
fn shown<F: Flavor>(
    editor: &Editor<F>,
    search: &Lookup,
) -> Vec<Entry> {
    entries(editor)
        .into_iter()
        .filter(|entry| is_listed(entry.group, &entry.name, search))
        .collect()
}

fn add<F: Flavor>(world: &mut World, entry: Entry, on_sheet: Vec2) {
    close::<F>(world);
    world.resource_mut::<Editor<F>>().add(
        entry.does,
        entry.settings,
        on_sheet,
    );
}

/// The open menu, over the panel.
pub(super) fn view<F: Flavor>() -> impl View {
    keyed(resource::<Menu<F>, _>(|menu| menu.0.clone()), |menu| {
        match menu {
            Which::Closed => frame().boxed(),
            Which::Create { at, on_sheet } => {
                create::<F>(*at, *on_sheet)
            }
            Which::Card { node, at } => card::<F>(*node, *at),
            Which::Group { group, at } => {
                group_menu::<F>(*group, *at)
            }
            Which::Input { node, input, at } => {
                input_menu::<F>(*node, input.clone(), *at)
            }
            Which::Choice {
                node,
                input,
                choices,
                at,
            } => choice_menu::<F>(
                *node,
                input.clone(),
                choices.clone(),
                *at,
            ),
        }
    })
    .within(overlay(()).tagged(Layer::<F>(PhantomData)))
}

fn create<F: Flavor>(at: Vec2, on_sheet: Vec2) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let at = in_layer::<F>(cx.world, at);
        let width = cx.theme().space.menu_width + 60.0;
        let list = list(cx.world.resource::<Editor<F>>(), on_sheet);
        let root = cx.build(
            popup(
                Rect::from_corners(at, at),
                vec![
                    group_selection::<F>(),
                    paste_here::<F>(on_sheet),
                    column((
                        section("Create node"),
                        search_bar::<F>(),
                        scroll((list,))
                            .gap(0.0)
                            .max_height(px(TALLEST)),
                    ))
                    .gap(4.0)
                    .boxed(),
                ],
            )
            .on_dismiss(|world, _| close::<F>(world))
            .min_width(px(width))
            .max_width(px(width)),
        );
        if let Some(input) = search_input::<F>(cx.world) {
            cx.world
                .entity_mut(input)
                .observe(edited::<F>)
                .observe(pressed::<F>);
            focus_search::<F>(cx.world);
        }
        root
    })
}

/// The item that groups the selected cards, shown while any are
/// selected.
fn group_selection<F: Flavor>() -> AnyView {
    menu_item(
        row((
            label("Group selection").wrap(false),
            frame().grow(1.0),
            note("Cmd+G"),
        ))
        .gap(16.0),
    )
    .display(resource::<Editor<F>, _>(|editor| {
        if editor.selected.is_empty() {
            Display::None
        } else {
            Display::Flex
        }
    }))
    .on_activate(|world| {
        close::<F>(world);
        world.resource_mut::<Editor<F>>().group();
    })
    .boxed()
}

/// The item that pastes the clipboard at `on_sheet`, shown while it
/// holds nodes.
fn paste_here<F: Flavor>(on_sheet: Vec2) -> AnyView {
    menu_item(
        row((
            label("Paste").wrap(false),
            frame().grow(1.0),
            note("Cmd+V"),
        ))
        .gap(16.0),
    )
    .display(resource::<Editor<F>, _>(|editor| {
        if editor.clipboard.is_some() {
            Display::Flex
        } else {
            Display::None
        }
    }))
    .on_activate(move |world| {
        close::<F>(world);
        world.resource_mut::<Editor<F>>().paste(Some(on_sheet));
    })
    .boxed()
}

/// A display that shows while `show` holds for where the list
/// stands.
fn shown_while<F: Flavor>(
    show: impl Fn(&Lookup) -> bool + Send + Sync + 'static,
) -> Signal<Display> {
    resource::<Search<F>, _>(move |search| {
        if show(search) {
            Display::Flex
        } else {
            Display::None
        }
    })
}

/// `view`, shown while `show` holds for where the list stands.
fn shown_with<F: Flavor>(
    view: impl View + 'static,
    show: impl Fn(&Lookup) -> bool + Send + Sync + 'static,
) -> AnyView {
    row((view,))
        .gap(0.0)
        .display(shown_while::<F>(show))
        .boxed()
}

/// The search field, with a hint over it while nothing is typed.
fn search_bar<F: Flavor>() -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        // The hint starts past the caret, which blinks where typing
        // begins.
        let hint = row((note("Search"),))
            .position(PositionType::Absolute)
            .inset(UiRect::new(
                px(gap / 2.0 + CARET),
                Val::Auto,
                px(0.0),
                px(0.0),
            ))
            .align(AlignItems::Center)
            .display(shown_while::<F>(|search| {
                search.typed.is_empty()
            }))
            .with(Pickable::IGNORE);
        cx.build(
            row((
                glyph(Glyph::Lens, ICON).tone(Tone::Dim),
                row((
                    text_field(String::new(), |_, _| {})
                        .grow(1.0)
                        .tagged(SearchField::<F>(PhantomData))
                        .rules(text_input),
                    hint,
                ))
                .gap(0.0)
                .grow(1.0),
            ))
            .gap(gap / 2.0)
            .align(AlignItems::Center),
        )
    })
}

/// The search field's input, while the list of kinds is open.
fn search_input<F: Flavor>(world: &mut World) -> Option<Entity> {
    world
        .query_filtered::<&Children, With<SearchField<F>>>()
        .iter(world)
        .next()
        .and_then(|children| children.first().copied())
}

/// Focuses the search field, with the caret at its end.
fn focus_search<F: Flavor>(world: &mut World) {
    let Some(input) = search_input::<F>(world) else {
        return;
    };
    world
        .resource_mut::<InputFocus>()
        .set(input, FocusCause::Pressed);
    if let Some(mut text) = world.get_mut::<EditableText>(input) {
        text.queue_edit(TextEdit::TextEnd(false));
    }
}

/// Searches by what is in the search field as it is typed.
fn edited<F: Flavor>(
    change: On<TextEditChange>,
    texts: Query<&EditableText>,
    mut search: ResMut<Search<F>>,
) {
    let Ok(text) = texts.get(change.event_target()) else {
        return;
    };
    let typed = text.value().to_string();
    if search.typed != typed {
        search.typed = typed;
    }
}

/// Adds the first found on Enter in the search field, and goes back
/// to the groups on Backspace with nothing in it.
fn pressed<F: Flavor>(
    key: On<FocusedInput<KeyboardInput>>,
    mut commands: Commands,
) {
    if !key.input.state.is_pressed() {
        return;
    }
    match key.input.logical_key {
        Key::Enter => commands.queue(add_first::<F>),
        Key::Backspace => commands.queue(|world: &mut World| {
            let mut search = world.resource_mut::<Search<F>>();
            if search.typed.is_empty() {
                search.group = None;
            }
        }),
        _ => {}
    }
}

/// Adds the first kind the list shows.
fn add_first<F: Flavor>(world: &mut World) {
    let Which::Create { on_sheet, .. } =
        **world.resource::<Menu<F>>()
    else {
        return;
    };
    let first = shown(
        world.resource::<Editor<F>>(),
        world.resource::<Search<F>>(),
    )
    .into_iter()
    .next();
    if let Some(entry) = first {
        add::<F>(world, entry, on_sheet);
    }
}

/// Every row the list can show, built once, each shown while it
/// belongs where the list stands: the groups' doors, then each
/// group's way back, its name over what is found and its kinds.
fn list<F: Flavor>(editor: &Editor<F>, on_sheet: Vec2) -> AnyView {
    let entries = entries(editor);
    let mut groups = Vec::<&'static str>::new();
    for entry in &entries {
        if groups.last() != Some(&entry.group) {
            groups.push(entry.group);
        }
    }
    let mut items =
        groups.iter().copied().map(door::<F>).collect::<Vec<_>>();
    for &group in &groups {
        items.push(back::<F>(group));
        let names = entries
            .iter()
            .filter(|entry| entry.group == group)
            .map(|entry| entry.name.clone())
            .collect::<Vec<_>>();
        items.push(shown_with::<F>(note(group), move |search| {
            !search.typed.trim().is_empty()
                && names
                    .iter()
                    .any(|name| is_found(group, name, &search.typed))
        }));
        for entry in
            entries.iter().filter(|entry| entry.group == group)
        {
            items.push(kind_row::<F>(entry, on_sheet));
        }
    }
    let names = entries
        .iter()
        .map(|entry| (entry.group, entry.name.clone()))
        .collect::<Vec<_>>();
    items.push(shown_with::<F>(
        note("Nothing is called that"),
        move |search| {
            !search.typed.trim().is_empty()
                && !names.iter().any(|(group, name)| {
                    is_found(group, name, &search.typed)
                })
        },
    ));
    column(items).gap(0.0).boxed()
}

/// The row that adds `entry`.
fn kind_row<F: Flavor>(entry: &Entry, on_sheet: Vec2) -> AnyView {
    let (group, name) = (entry.group, entry.name.clone());
    let (doc, added) = (entry.doc.clone(), entry.clone());
    menu_item(label(entry.name.replace('_', " ")).wrap(false))
        .display(shown_while::<F>(move |search| {
            is_listed(group, &name, search)
        }))
        .tooltip(move || note(doc.clone()))
        .on_activate(move |world| {
            add::<F>(world, added.clone(), on_sheet);
        })
        .boxed()
}

/// The row that opens `group`'s page, shown with the other groups.
fn door<F: Flavor>(group: &'static str) -> AnyView {
    menu_item(
        row((
            label(group).wrap(false),
            frame().grow(1.0),
            glyph(Glyph::Into, ICON).tone(Tone::Dim),
        ))
        .align(AlignItems::Center),
    )
    .display(shown_while::<F>(|search| {
        search.typed.trim().is_empty() && search.group.is_none()
    }))
    .on_activate(move |world| {
        world.resource_mut::<Search<F>>().group = Some(group);
        focus_search::<F>(world);
    })
    .boxed()
}

/// The row at the head of `group`'s page, back to the groups.
fn back<F: Flavor>(group: &'static str) -> AnyView {
    menu_item(
        row((
            glyph(Glyph::Back, ICON).tone(Tone::Dim),
            label(group).bold(true).wrap(false),
        ))
        .gap(6.0)
        .align(AlignItems::Center),
    )
    .display(shown_while::<F>(move |search| {
        search.typed.trim().is_empty() && search.group == Some(group)
    }))
    .on_activate(|world| {
        world.resource_mut::<Search<F>>().group = None;
        focus_search::<F>(world);
    })
    .boxed()
}

/// An item that closes the menu and runs `act`, with the keys that
/// do the same beside its text.
fn keyed_item<F: Flavor>(
    text: &'static str,
    keys: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    menu_item(
        row((label(text).wrap(false), frame().grow(1.0), note(keys)))
            .gap(16.0),
    )
    .on_activate(move |world| {
        close::<F>(world);
        act(world);
    })
    .boxed()
}

fn card<F: Flavor>(node: NodeId, opened_at: Vec2) -> AnyView {
    let item = move |text: &'static str,
                     keys: &'static str,
                     act: fn(&mut World, NodeId)| {
        keyed_item::<F>(text, keys, move |world| act(world, node))
    };
    AnyView::new(move |cx: &mut Cx| {
        let at = in_layer::<F>(cx.world, opened_at);
        let width = cx.theme().space.menu_width;
        let runs_graph = matches!(
            cx.world
                .resource::<Editor<F>>()
                .sheet
                .graph
                .node(node)
                .map(|found| &found.does),
            Some(Does::Graph(_))
        );
        let mut items = Vec::new();
        let named = {
            let editor = cx.world.resource::<Editor<F>>();
            editor.sheet.graph.node(node).and_then(|found| {
                super::naming(&editor.registry, found)
                    .map(|(spec, _)| (spec.name, spec.choices))
            })
        };
        if let Some((input, choices)) = named {
            items.push(
                menu_item(label("Switch to…").wrap(false))
                    .on_activate(move |world| {
                        close::<F>(world);
                        choose::<F>(
                            world,
                            node,
                            input.to_owned(),
                            choices
                                .iter()
                                .map(|c| (*c).to_owned())
                                .collect(),
                        );
                    })
                    .boxed(),
            );
        }
        if runs_graph {
            items.push(item(
                "Open graph",
                "Double-click",
                |world, node| {
                    open_graph::<F>(world, node);
                },
            ));
        }
        items.push(item("Group", "Cmd+G", |world, _| {
            world.resource_mut::<Editor<F>>().group();
        }));
        if cx.world.resource::<Editor<F>>().has_grouped() {
            items.push(item(
                "Take out of group",
                "Cmd+Shift+G",
                |world, _| {
                    world
                        .resource_mut::<Editor<F>>()
                        .ungroup_selected();
                },
            ));
        }
        items.extend([
            item("Expose as output", "", |world, node| {
                world.resource_mut::<Editor<F>>().expose_output(node);
            }),
            item("Copy", "Cmd+C", |world, _| {
                world.resource_mut::<Editor<F>>().copy();
            }),
            item("Cut", "Cmd+X", |world, _| {
                world.resource_mut::<Editor<F>>().cut();
            }),
        ]);
        if cx.world.resource::<Editor<F>>().clipboard.is_some() {
            items.push(keyed_item::<F>(
                "Paste",
                "Cmd+V",
                move |world| paste_at::<F>(world, opened_at),
            ));
        }
        items.extend([
            item("Duplicate", "Cmd+D", |world, _| {
                world.resource_mut::<Editor<F>>().duplicate();
            }),
            item("Disconnect", "", |world, node| {
                world.resource_mut::<Editor<F>>().unwire(node);
            }),
            item("Delete", "Delete", |world, _| {
                world.resource_mut::<Editor<F>>().remove();
            }),
        ]);
        cx.build(
            popup(Rect::from_corners(at, at), items)
                .on_dismiss(|world, _| close::<F>(world))
                .min_width(px(width)),
        )
    })
}

/// Pastes the clipboard with its top left at the window's point
/// `at`, or off the originals when that is not on the canvas.
fn paste_at<F: Flavor>(world: &mut World, at: Vec2) {
    let on_sheet = canvas::on_sheet::<F>(world, at);
    world.resource_mut::<Editor<F>>().paste(on_sheet);
}

/// The menu on a group, which takes its cards out of it.
fn group_menu<F: Flavor>(group: usize, at: Vec2) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let at = in_layer::<F>(cx.world, at);
        let items = vec![
            menu_item(label("Ungroup").wrap(false))
                .on_activate(move |world| {
                    close::<F>(world);
                    world.resource_mut::<Editor<F>>().ungroup(group);
                })
                .boxed(),
        ];
        cx.build(
            popup(
                Rect::from_corners(at, at),
                (scroll(items).gap(0.0).max_height(px(TALLEST)),),
            )
            .on_dismiss(|world, _| close::<F>(world))
            .min_width(px(cx.theme().space.menu_width * 0.6)),
        )
    })
}

/// Opens the choices of `node`'s `input` at the pointer.
pub(super) fn choose<F: Flavor>(
    world: &mut World,
    node: NodeId,
    input: String,
    choices: Vec<String>,
) {
    let at = world
        .query::<&Window>()
        .iter(world)
        .find_map(Window::cursor_position)
        .unwrap_or_default();
    open::<F>(
        world,
        Which::Choice {
            node,
            input,
            choices,
            at,
        },
    );
}

fn choice_menu<F: Flavor>(
    node: NodeId,
    input: String,
    choices: Vec<String>,
    at: Vec2,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let at = in_layer::<F>(cx.world, at);
        let items = choices
            .iter()
            .cloned()
            .map(|choice| {
                let input = input.clone();
                menu_item(label(choice.replace('_', " ")).wrap(false))
                    .on_activate(move |world| {
                        close::<F>(world);
                        controls::put::<F>(
                            world,
                            node,
                            &input,
                            Some(Setting::Fixed(Constant::Choice(
                                choice.clone(),
                            ))),
                        );
                    })
                    .boxed()
            })
            .collect::<Vec<_>>();
        cx.build(
            popup(Rect::from_corners(at, at), items)
                .on_dismiss(|world, _| close::<F>(world))
                .min_width(px(cx.theme().space.menu_width * 0.6)),
        )
    })
}

/// An item that closes the menu and runs `act`.
fn action_item<F: Flavor>(
    text: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    menu_item(label(text).wrap(false))
        .on_activate(move |world| {
            close::<F>(world);
            act(world);
        })
        .boxed()
}

/// The menu on how an input of `node` is set, which exposes it as a
/// parameter of the graph, gives it a value of its own, resets it to
/// its default, or cuts its wire.
fn input_menu<F: Flavor>(
    node: NodeId,
    input: Input,
    at: Vec2,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let at = in_layer::<F>(cx.world, at);
        let editor = cx.world.resource::<Editor<F>>();
        let setting = controls::current(editor, node, &input.name);
        let wired = editor.sheet.graph.wires.iter().any(|wire| {
            wire.to.node == node && wire.to.name == input.name
        });
        let written = controls::written(&input);
        let mut items = Vec::<AnyView>::new();
        let name = input.name.clone();
        match (&setting, written) {
            (Some(Setting::FromParent(_)), _) => {
                items.push(action_item::<F>(
                    "Use a value",
                    move |world| {
                        controls::put::<F>(world, node, &name, None);
                    },
                ));
            }
            (_, Some(value_type)) if !wired => {
                let read = input.clone();
                items.push(action_item::<F>(
                    "Make a parameter",
                    move |world| {
                        let default = controls::value_of(
                            world.resource::<Editor<F>>(),
                            node,
                            &read,
                        );
                        world
                            .resource_mut::<Editor<F>>()
                            .expose_input(
                                node, &read.name, value_type, default,
                            );
                    },
                ));
            }
            _ => {}
        }
        if matches!(
            setting,
            Some(Setting::Fixed(_) | Setting::Random(_))
        ) {
            let name = input.name.clone();
            items.push(action_item::<F>(
                "Reset to default",
                move |world| {
                    controls::put::<F>(world, node, &name, None);
                },
            ));
        }
        if wired {
            let name = input.name.clone();
            items.push(action_item::<F>(
                "Disconnect",
                move |world| {
                    let mut editor =
                        world.resource_mut::<Editor<F>>();
                    let cut = editor
                        .sheet
                        .graph
                        .wires
                        .iter()
                        .filter(|wire| {
                            wire.to.node == node
                                && wire.to.name == name
                        })
                        .cloned()
                        .map(Edit::Disconnect)
                        .collect::<Vec<_>>();
                    editor.apply(Edit::Batch(cut));
                },
            ));
        }
        if items.is_empty() {
            items.push(note("It can only be wired").boxed());
        }
        cx.build(
            popup(Rect::from_corners(at, at), items)
                .on_dismiss(|world, _| close::<F>(world))
                .min_width(px(cx.theme().space.menu_width)),
        )
    })
}

/// Opens the graph `node` runs, if it runs one.
pub(super) fn open_graph<F: Flavor>(world: &mut World, node: NodeId) {
    let mut editor = world.resource_mut::<Editor<F>>();
    let name = match editor
        .sheet
        .graph
        .node(node)
        .map(|found| &found.does)
    {
        Some(Does::Graph(name)) => name.clone(),
        _ => return,
    };
    editor.go_to(&name);
    canvas::fit::<F>(world);
}

/// The graph's keys, while the pointer is on this canvas and nothing
/// is being typed in.
fn keys<F: Flavor>(world: &mut World) {
    if **world.resource::<Menu<F>>() != Which::Closed
        || world.resource::<InputFocus>().get().is_some()
        || !canvas::is_hovered::<F>(world)
    {
        return;
    }
    let keys = world.resource::<ButtonInput<KeyCode>>();
    let command = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    let shift =
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let pressed = |code| keys.just_pressed(code);
    let space = pressed(KeyCode::Space);
    let delete =
        pressed(KeyCode::Delete) || pressed(KeyCode::Backspace);
    let undo = command && pressed(KeyCode::KeyZ);
    let duplicate = command && pressed(KeyCode::KeyD);
    let copy = command && pressed(KeyCode::KeyC);
    let cut = command && pressed(KeyCode::KeyX);
    let paste = command && pressed(KeyCode::KeyV);
    let group = command && pressed(KeyCode::KeyG);
    let save = command && pressed(KeyCode::KeyS);
    let open = command && pressed(KeyCode::KeyO);
    let fit = !command && pressed(KeyCode::KeyF);
    let selected = !world.resource::<Editor<F>>().selected.is_empty();
    if space {
        create_at_pointer::<F>(world);
    } else if save && shift {
        files::save_as::<F>(world);
    } else if save {
        files::save::<F>(world);
    } else if open {
        files::open::<F>(world);
    } else if undo && shift {
        world.resource_mut::<Editor<F>>().redo();
    } else if undo {
        world.resource_mut::<Editor<F>>().undo();
    } else if group && shift {
        world.resource_mut::<Editor<F>>().ungroup_selected();
    } else if group && selected {
        world.resource_mut::<Editor<F>>().group();
    } else if duplicate && selected {
        world.resource_mut::<Editor<F>>().duplicate();
    } else if copy && selected {
        world.resource_mut::<Editor<F>>().copy();
    } else if cut && selected {
        world.resource_mut::<Editor<F>>().cut();
    } else if paste {
        let at = world
            .query::<&Window>()
            .iter(world)
            .find_map(Window::cursor_position)
            .unwrap_or_default();
        paste_at::<F>(world, at);
    } else if delete && selected {
        world.resource_mut::<Editor<F>>().remove();
    } else if fit {
        canvas::fit::<F>(world);
    }
}
