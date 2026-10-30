//! The graph's menus and keys. A right click on the canvas, or Space,
//! opens the groups of what can be added, each a page of its own;
//! its search field, which has the keys while it is open, searches
//! every group, and Enter adds the first found. A right click on a
//! card opens what can be done to it.

use std::collections::BTreeMap;

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
use hamlet_blueprint::{Constant, Does, Edit, NodeId, Setting};
use hamlet_ui::styles::{ICON, note, section, text_input};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View};

use super::canvas::Input;
use super::{Editor, canvas, controls, files};

/// The group the book's other graphs are listed under.
const GRAPHS: &str = "Graphs";

/// The groups in the order they are listed, before any others.
const GROUPS: [&str; 7] = [
    GRAPHS,
    "Curves",
    "Placements",
    "Modifiers",
    "Fields",
    "Math",
    "Output",
];

/// The most the list of what can be added grows before it scrolls.
const TALLEST: f32 = 320.0;
/// The room the caret takes before the search's hint.
const CARET: f32 = 4.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Menu>()
        .init_resource::<Search>()
        .add_systems(Update, keys);
}

/// The menu open over the graph, at its place in the window.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub(super) enum Menu {
    #[default]
    Closed,
    /// The list of kinds, adding a card at `on_sheet`.
    Create { at: Vec2, on_sheet: Vec2 },
    /// What can be done to a card.
    Card { node: NodeId, at: Vec2 },
    /// What sets an input of a card.
    Input {
        node: NodeId,
        input: Input,
        at: Vec2,
    },
    /// The choices an input of a card can take.
    Choice {
        node: NodeId,
        input: String,
        choices: &'static [&'static str],
        at: Vec2,
    },
}

/// Where the list of kinds stands: what is typed, and the group open,
/// none for the list of groups.
#[derive(Resource, Default, Clone, PartialEq)]
struct Search {
    typed: String,
    group: Option<&'static str>,
}

/// On the search field of the list of kinds.
#[derive(Component)]
struct SearchField;

/// On the layer the menus are placed in.
#[derive(Component)]
struct Layer;

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
pub(super) fn create_at_pointer(world: &mut World) {
    let at = world
        .query::<&Window>()
        .iter(world)
        .find_map(Window::cursor_position)
        .unwrap_or_default();
    let on_sheet = canvas::on_sheet(world, at)
        .unwrap_or_else(|| canvas::middle(world));
    open(world, Menu::Create { at, on_sheet });
}

pub(super) fn open(world: &mut World, menu: Menu) {
    *world.resource_mut::<Search>() = Search::default();
    *world.resource_mut::<Menu>() = menu;
}

fn close(world: &mut World) {
    *world.resource_mut::<Menu>() = Menu::Closed;
}

/// Where the window's point `at` is in the layer the menus are
/// placed in.
fn in_layer(world: &mut World, at: Vec2) -> Vec2 {
    world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<Layer>>()
        .iter(world)
        .next()
        .map_or(at, |(node, transform)| {
            at - canvas::corner(node, transform)
        })
}

/// Whether `search` finds the kind `name` of `group`: either holds
/// it.
fn holds(group: &str, name: &str, search: &str) -> bool {
    let search = search.trim().to_lowercase().replace(' ', "_");
    name.replace(' ', "_").to_lowercase().contains(&search)
        || group.to_lowercase().contains(&search)
}

/// Everything that can be added, by group. A graph that runs the
/// open one is left out, as it would run itself.
fn entries(editor: &Editor) -> Vec<Entry> {
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
fn listed(group: &'static str, name: &str, search: &Search) -> bool {
    if search.typed.trim().is_empty() {
        search.group == Some(group)
    } else {
        holds(group, name, &search.typed)
    }
}

/// What the list shows, in order.
fn shown(editor: &Editor, search: &Search) -> Vec<Entry> {
    entries(editor)
        .into_iter()
        .filter(|entry| listed(entry.group, &entry.name, search))
        .collect()
}

fn add(world: &mut World, entry: Entry, on_sheet: Vec2) {
    close(world);
    world.resource_mut::<Editor>().add(
        entry.does,
        entry.settings,
        on_sheet,
    );
}

/// The open menu, over the panel.
pub(super) fn view() -> impl View {
    keyed(resource::<Menu, _>(Menu::clone), |menu| match menu {
        Menu::Closed => frame().boxed(),
        Menu::Create { at, on_sheet } => create(*at, *on_sheet),
        Menu::Card { node, at } => card(*node, *at),
        Menu::Input { node, input, at } => {
            input_menu(*node, input.clone(), *at)
        }
        Menu::Choice {
            node,
            input,
            choices,
            at,
        } => choice_menu(*node, input.clone(), choices, *at),
    })
    .within(overlay(()).tagged(Layer))
}

fn create(at: Vec2, on_sheet: Vec2) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let at = in_layer(cx.world, at);
        let width = cx.theme().space.menu_width + 60.0;
        let list = list(cx.world.resource::<Editor>(), on_sheet);
        let root = cx.build(
            popup(
                Rect::from_corners(at, at),
                vec![
                    column((
                        section("Create node"),
                        search_bar(),
                        scroll((list,))
                            .gap(0.0)
                            .max_height(px(TALLEST)),
                    ))
                    .gap(4.0)
                    .boxed(),
                ],
            )
            .on_dismiss(|world, _| close(world))
            .min_width(px(width))
            .max_width(px(width)),
        );
        if let Some(input) = search_input(cx.world) {
            cx.world
                .entity_mut(input)
                .observe(edited)
                .observe(pressed);
            focus_search(cx.world);
        }
        root
    })
}

/// Shown while `show` holds for where the list stands.
fn shown_while(
    show: impl Fn(&Search) -> bool + Send + Sync + 'static,
) -> Signal<Display> {
    resource::<Search, _>(move |search| {
        if show(search) {
            Display::Flex
        } else {
            Display::None
        }
    })
}

/// `view`, shown while `show` holds for where the list stands.
fn shown_with(
    view: impl View + 'static,
    show: impl Fn(&Search) -> bool + Send + Sync + 'static,
) -> AnyView {
    row((view,)).gap(0.0).display(shown_while(show)).boxed()
}

/// The search field, over what it is for while nothing is typed in
/// it.
fn search_bar() -> AnyView {
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
            .display(shown_while(|search| search.typed.is_empty()))
            .with(Pickable::IGNORE);
        cx.build(
            row((
                glyph(Glyph::Lens, ICON).tone(Tone::Dim),
                row((
                    text_field(String::new(), |_, _| {})
                        .grow(1.0)
                        .tagged(SearchField)
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
fn search_input(world: &mut World) -> Option<Entity> {
    world
        .query_filtered::<&Children, With<SearchField>>()
        .iter(world)
        .next()
        .and_then(|kids| kids.first().copied())
}

/// Gives the search field the keys, with the caret at its end.
fn focus_search(world: &mut World) {
    let Some(input) = search_input(world) else {
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
fn edited(
    change: On<TextEditChange>,
    texts: Query<&EditableText>,
    mut search: ResMut<Search>,
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
fn pressed(
    key: On<FocusedInput<KeyboardInput>>,
    mut commands: Commands,
) {
    if !key.input.state.is_pressed() {
        return;
    }
    match key.input.logical_key {
        Key::Enter => commands.queue(add_first),
        Key::Backspace => commands.queue(|world: &mut World| {
            let mut search = world.resource_mut::<Search>();
            if search.typed.is_empty() {
                search.group = None;
            }
        }),
        _ => {}
    }
}

/// Adds the first kind the list shows.
fn add_first(world: &mut World) {
    let Menu::Create { on_sheet, .. } = *world.resource::<Menu>()
    else {
        return;
    };
    let first =
        shown(world.resource::<Editor>(), world.resource::<Search>())
            .into_iter()
            .next();
    if let Some(entry) = first {
        add(world, entry, on_sheet);
    }
}

/// Every row the list can show, built once, each shown while it
/// belongs where the list stands: the groups' doors, then each
/// group's way back, its name over what is found and its kinds.
fn list(editor: &Editor, on_sheet: Vec2) -> AnyView {
    let entries = entries(editor);
    let mut groups: Vec<&'static str> = Vec::new();
    for entry in &entries {
        if groups.last() != Some(&entry.group) {
            groups.push(entry.group);
        }
    }
    let mut items: Vec<AnyView> =
        groups.iter().copied().map(door).collect();
    for &group in &groups {
        items.push(back(group));
        let names = entries
            .iter()
            .filter(|entry| entry.group == group)
            .map(|entry| entry.name.clone())
            .collect::<Vec<_>>();
        items.push(shown_with(note(group), move |search| {
            !search.typed.trim().is_empty()
                && names
                    .iter()
                    .any(|name| holds(group, name, &search.typed))
        }));
        for entry in
            entries.iter().filter(|entry| entry.group == group)
        {
            items.push(kind_row(entry, on_sheet));
        }
    }
    let names = entries
        .iter()
        .map(|entry| (entry.group, entry.name.clone()))
        .collect::<Vec<_>>();
    items.push(shown_with(
        note("Nothing is called that"),
        move |search| {
            !search.typed.trim().is_empty()
                && !names.iter().any(|(group, name)| {
                    holds(group, name, &search.typed)
                })
        },
    ));
    column(items).gap(0.0).boxed()
}

/// The row that adds `entry`.
fn kind_row(entry: &Entry, on_sheet: Vec2) -> AnyView {
    let (group, name) = (entry.group, entry.name.clone());
    let (doc, added) = (entry.doc.clone(), entry.clone());
    menu_item(label(entry.name.replace('_', " ")).wrap(false))
        .display(shown_while(move |search| {
            listed(group, &name, search)
        }))
        .tooltip(move || note(doc.clone()))
        .on_activate(move |world| add(world, added.clone(), on_sheet))
        .boxed()
}

/// The row that opens `group`'s page, shown with the other groups.
fn door(group: &'static str) -> AnyView {
    menu_item(
        row((
            label(group).wrap(false),
            frame().grow(1.0),
            glyph(Glyph::Into, ICON).tone(Tone::Dim),
        ))
        .align(AlignItems::Center),
    )
    .display(shown_while(|search| {
        search.typed.trim().is_empty() && search.group.is_none()
    }))
    .on_activate(move |world| {
        world.resource_mut::<Search>().group = Some(group);
        focus_search(world);
    })
    .boxed()
}

/// The row at the head of `group`'s page, back to the groups.
fn back(group: &'static str) -> AnyView {
    menu_item(
        row((
            glyph(Glyph::Back, ICON).tone(Tone::Dim),
            label(group).bold(true).wrap(false),
        ))
        .gap(6.0)
        .align(AlignItems::Center),
    )
    .display(shown_while(move |search| {
        search.typed.trim().is_empty() && search.group == Some(group)
    }))
    .on_activate(|world| {
        world.resource_mut::<Search>().group = None;
        focus_search(world);
    })
    .boxed()
}

fn card(node: NodeId, at: Vec2) -> AnyView {
    let item = move |text: &'static str,
                     keys: &'static str,
                     act: fn(&mut World, NodeId)| {
        menu_item(
            row((
                label(text).wrap(false),
                frame().grow(1.0),
                note(keys),
            ))
            .gap(16.0),
        )
        .on_activate(move |world| {
            close(world);
            act(world, node);
        })
        .boxed()
    };
    AnyView::new(move |cx: &mut Cx| {
        let at = in_layer(cx.world, at);
        let width = cx.theme().space.menu_width;
        let runs_graph = matches!(
            cx.world
                .resource::<Editor>()
                .sheet
                .graph
                .node(node)
                .map(|found| &found.does),
            Some(Does::Graph(_))
        );
        let mut items = Vec::new();
        let named = {
            let editor = cx.world.resource::<Editor>();
            editor.sheet.graph.node(node).and_then(|found| {
                super::naming(&editor.registry, found)
                    .map(|(spec, _)| (spec.name, spec.choices))
            })
        };
        if let Some((input, choices)) = named {
            items.push(
                menu_item(label("Switch to…").wrap(false))
                    .on_activate(move |world| {
                        close(world);
                        choose(
                            world,
                            node,
                            input.to_owned(),
                            choices,
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
                    open_graph(world, node);
                },
            ));
        }
        items.extend([
            item("Expose as output", "", |world, node| {
                world.resource_mut::<Editor>().expose_output(node);
            }),
            item("Duplicate", "Cmd+D", |world, _| {
                world.resource_mut::<Editor>().duplicate();
            }),
            item("Disconnect", "", |world, node| {
                world.resource_mut::<Editor>().unwire(node);
            }),
            item("Delete", "Delete", |world, _| {
                world.resource_mut::<Editor>().remove();
            }),
        ]);
        cx.build(
            popup(Rect::from_corners(at, at), items)
                .on_dismiss(|world, _| close(world))
                .min_width(px(width)),
        )
    })
}

/// Opens the choices of `node`'s `input` at the pointer.
pub(super) fn choose(
    world: &mut World,
    node: NodeId,
    input: String,
    choices: &'static [&'static str],
) {
    let at = world
        .query::<&Window>()
        .iter(world)
        .find_map(Window::cursor_position)
        .unwrap_or_default();
    open(
        world,
        Menu::Choice {
            node,
            input,
            choices,
            at,
        },
    );
}

fn choice_menu(
    node: NodeId,
    input: String,
    choices: &'static [&'static str],
    at: Vec2,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let at = in_layer(cx.world, at);
        let items = choices
            .iter()
            .map(|choice| {
                let input = input.clone();
                menu_item(label(choice.replace('_', " ")).wrap(false))
                    .on_activate(move |world| {
                        close(world);
                        controls::put(
                            world,
                            node,
                            &input,
                            Some(Setting::Fixed(Constant::Choice(
                                (*choice).to_owned(),
                            ))),
                        );
                    })
                    .boxed()
            })
            .collect::<Vec<_>>();
        cx.build(
            popup(Rect::from_corners(at, at), items)
                .on_dismiss(|world, _| close(world))
                .min_width(px(cx.theme().space.menu_width * 0.6)),
        )
    })
}

/// What can be done to how an input of `node` is set: made a
/// parameter of the graph, given a value of its own again, or put
/// back to its default, and its wire cut.
fn input_menu(node: NodeId, input: Input, at: Vec2) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let at = in_layer(cx.world, at);
        let editor = cx.world.resource::<Editor>();
        let setting = controls::current(editor, node, &input.name);
        let wired = editor.sheet.graph.wires.iter().any(|wire| {
            wire.to.node == node && wire.to.name == input.name
        });
        let written = controls::written(input.value_type);
        let mut items: Vec<AnyView> = Vec::new();
        let mut item =
            |text: &'static str,
             act: Box<dyn Fn(&mut World) + Send + Sync>| {
                items.push(
                    menu_item(label(text).wrap(false))
                        .on_activate(move |world| {
                            close(world);
                            act(world);
                        })
                        .boxed(),
                );
            };
        let name = input.name.clone();
        match (&setting, written) {
            (Some(Setting::FromParent(_)), _) => {
                item(
                    "Use a value",
                    Box::new(move |world| {
                        controls::put(world, node, &name, None);
                    }),
                );
            }
            (_, Some(value_type)) if !wired => {
                let read = input.clone();
                item(
                    "Make a parameter",
                    Box::new(move |world| {
                        let default = controls::value_of(
                            world.resource::<Editor>(),
                            node,
                            &read,
                        );
                        world.resource_mut::<Editor>().expose_input(
                            node, &read.name, value_type, default,
                        );
                    }),
                );
            }
            _ => {}
        }
        if matches!(
            setting,
            Some(Setting::Fixed(_) | Setting::Random(_))
        ) {
            let name = input.name.clone();
            item(
                "Reset to default",
                Box::new(move |world| {
                    controls::put(world, node, &name, None);
                }),
            );
        }
        if wired {
            let name = input.name.clone();
            item(
                "Disconnect",
                Box::new(move |world| {
                    let mut editor = world.resource_mut::<Editor>();
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
                }),
            );
        }
        if items.is_empty() {
            items.push(note("It can only be wired").boxed());
        }
        cx.build(
            popup(Rect::from_corners(at, at), items)
                .on_dismiss(|world, _| close(world))
                .min_width(px(cx.theme().space.menu_width)),
        )
    })
}

/// Opens the graph `node` runs, if it runs one.
pub(super) fn open_graph(world: &mut World, node: NodeId) {
    let mut editor = world.resource_mut::<Editor>();
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
    canvas::fit(world);
}

/// The graph's keys, while the pointer is on the canvas and nothing
/// is being typed in.
fn keys(world: &mut World) {
    if *world.resource::<Menu>() != Menu::Closed
        || world.resource::<InputFocus>().get().is_some()
        || !canvas::is_hovered(world)
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
    let save = command && pressed(KeyCode::KeyS);
    let open = command && pressed(KeyCode::KeyO);
    let fit = !command && pressed(KeyCode::KeyF);
    let selected = !world.resource::<Editor>().selected.is_empty();
    if space {
        create_at_pointer(world);
    } else if save && shift {
        files::save_as(world);
    } else if save {
        files::save(world);
    } else if open {
        files::open(world);
    } else if undo && shift {
        world.resource_mut::<Editor>().redo();
    } else if undo {
        world.resource_mut::<Editor>().undo();
    } else if duplicate && selected {
        world.resource_mut::<Editor>().duplicate();
    } else if delete && selected {
        world.resource_mut::<Editor>().remove();
    } else if fit {
        canvas::fit(world);
    }
}
