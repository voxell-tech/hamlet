//! The graph's menus and keys. A right click on the canvas, or Space,
//! opens a list of every kind of node and every other graph of the
//! book, by group, narrowed by what is typed while it is open; Enter
//! adds the first left. A right click on a card opens what can be
//! done to it.

use bevy::app::{App, Update};
use bevy::ecs::message::MessageReader;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::ResMut;
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input_focus::InputFocus;
use bevy::math::{Rect, Vec2};
use bevy::ui::px;
use bevy::window::Window;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, column, frame, label,
    menu_item, overlay, popup, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_graph::{Does, NodeId};

use super::{Editor, canvas, files};
use crate::looks::{note, section};
use crate::{AnyView, Cx, View};

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

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Menu>()
        .init_resource::<Search>()
        .add_systems(Update, (type_search, keys).chain());
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
}

/// What is typed while the list of kinds is open.
#[derive(Resource, Default, Clone, PartialEq)]
struct Search(String);

/// One row of the list of what can be added.
struct Entry {
    group: &'static str,
    name: String,
    doc: String,
    does: Does,
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
    world.resource_mut::<Search>().0.clear();
    *world.resource_mut::<Menu>() = menu;
}

fn close(world: &mut World) {
    *world.resource_mut::<Menu>() = Menu::Closed;
}

/// What can be added whose name or group holds `search`, by group. A
/// graph that runs the open one is left out, as it would run itself.
fn matches(editor: &Editor, search: &str) -> Vec<Entry> {
    let search = search.trim().to_lowercase().replace(' ', "_");
    let rank = |group: &str| {
        GROUPS
            .iter()
            .position(|g| *g == group)
            .unwrap_or(GROUPS.len())
    };
    let kinds = editor.registry.kinds().map(|(group, kind)| Entry {
        group,
        name: kind.name.to_owned(),
        doc: kind.doc.to_owned(),
        does: Does::Kind(kind.name.to_owned()),
    });
    let graphs = editor
        .names()
        .into_iter()
        .filter(|name| !editor.runs(name, &editor.name))
        .map(|name| Entry {
            group: GRAPHS,
            doc: format!("The graph {name} of this book"),
            does: Does::Graph(name.clone()),
            name,
        });
    let mut found = graphs
        .chain(kinds)
        .filter(|entry| {
            entry
                .name
                .replace(' ', "_")
                .to_lowercase()
                .contains(&search)
                || entry.group.to_lowercase().contains(&search)
        })
        .collect::<Vec<_>>();
    found.sort_by(|a, b| {
        (rank(a.group), a.group, &a.name).cmp(&(
            rank(b.group),
            b.group,
            &b.name,
        ))
    });
    found
}

fn add(world: &mut World, does: Does, on_sheet: Vec2) {
    close(world);
    world.resource_mut::<Editor>().add(does, on_sheet);
}

/// The open menu, over the panel.
pub(super) fn view() -> impl View {
    keyed(resource::<Menu, _>(Menu::clone), |menu| match *menu {
        Menu::Closed => frame().boxed(),
        Menu::Create { at, on_sheet } => create(at, on_sheet),
        Menu::Card { node, at } => card(node, at),
    })
    .within(overlay(()))
}

fn create(at: Vec2, on_sheet: Vec2) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let width = cx.theme().space.menu_width + 60.0;
        let typed = label(resource::<Search, _>(|search| {
            if search.0.is_empty() {
                "Type to search".to_owned()
            } else {
                format!("{}|", search.0)
            }
        }));
        let list = keyed(
            resource::<Search, _>(Search::clone),
            move |search| list(search.0.clone(), on_sheet),
        )
        .within(column(()).gap(0.0));
        cx.build(
            popup(
                Rect::from_corners(at, at),
                vec![
                    column((section("Create node"), typed, list))
                        .gap(4.0)
                        .boxed(),
                ],
            )
            .on_dismiss(|world, _| close(world))
            .min_width(px(width)),
        )
    })
}

fn list(search: String, on_sheet: Vec2) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let editor = cx.world.resource::<Editor>();
        let mut items: Vec<AnyView> = Vec::new();
        let mut last = None;
        for entry in matches(editor, &search) {
            if last != Some(entry.group) {
                last = Some(entry.group);
                items.push(note(entry.group).boxed());
            }
            let Entry {
                name, doc, does, ..
            } = entry;
            items.push(
                menu_item(label(name.replace('_', " ")).wrap(false))
                    .tooltip(move || note(doc.clone()))
                    .on_activate(move |world| {
                        add(world, does.clone(), on_sheet);
                    })
                    .boxed(),
            );
        }
        if items.is_empty() {
            items.push(note("Nothing is called that").boxed());
        }
        cx.build(column(items).gap(0.0))
    })
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
            item("Duplicate", "Cmd+D", |world, node| {
                world.resource_mut::<Editor>().duplicate(node);
            }),
            item("Disconnect", "", |world, node| {
                world.resource_mut::<Editor>().unwire(node);
            }),
            item("Delete", "Delete", |world, node| {
                world.resource_mut::<Editor>().remove(node);
            }),
        ]);
        cx.build(
            popup(Rect::from_corners(at, at), items)
                .on_dismiss(|world, _| close(world))
                .min_width(px(width)),
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
    editor.enter(&name);
    canvas::fit(world);
}

/// Narrows the list of kinds by what is typed while it is open, and
/// adds the first kind left on Enter.
fn type_search(
    mut typed: MessageReader<KeyboardInput>,
    mut menu: ResMut<Menu>,
    mut search: ResMut<Search>,
    mut editor: ResMut<Editor>,
) {
    let Menu::Create { on_sheet, .. } = *menu else {
        typed.clear();
        return;
    };
    for input in typed.read() {
        if !input.state.is_pressed() {
            continue;
        }
        match &input.logical_key {
            Key::Character(text) => {
                let letters = text
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == ' ');
                search.0.extend(letters);
            }
            Key::Space => search.0.push(' '),
            Key::Backspace => {
                search.0.pop();
            }
            Key::Enter => {
                let first = matches(&editor, &search.0)
                    .into_iter()
                    .next()
                    .map(|entry| entry.does);
                if let Some(does) = first {
                    *menu = Menu::Closed;
                    editor.add(does, on_sheet);
                    return;
                }
            }
            _ => {}
        }
    }
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
    let selected = world.resource::<Editor>().selected;
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
    } else if let (true, Some(node)) = (duplicate, selected) {
        world.resource_mut::<Editor>().duplicate(node);
    } else if let (true, Some(node)) = (delete, selected) {
        world.resource_mut::<Editor>().remove(node);
    } else if fit {
        canvas::fit(world);
    }
}
