//! The menus of the bar, gathered from the menu entries there are: a
//! title each, lit while its menu is open.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, MENU_Z, frame, label, menu_item,
    overlay, popup, row,
};
use bevy_fynix::{Hovered, ModifierExt, ViewExt, keyed, resource};
use hamlet_engine_api::CommandId;

use crate::command::{self, Command, MenuEntry, Shortcut};
use crate::style::{BottomLight, title_button};
use crate::{AnyView, Cx};

/// The engine's own menus, first and in this order.
const OWN: [&str; 3] = ["Hamlet", "Workspace", "View"];

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<MenuBar>()
        .init_resource::<Placed>()
        .init_resource::<OpenMenu>()
        .add_observer(place)
        .add_systems(Update, (gather, light_titles, follow_pointer));
}

/// The menus as the bar shows them.
#[derive(Resource, Clone, PartialEq, Default)]
struct MenuBar(Vec<Menu>);

#[derive(Clone, PartialEq)]
struct Menu {
    title: String,
    items: Vec<Item>,
}

#[derive(Clone, PartialEq)]
struct Item {
    label: String,
    /// The key bound to it, as the platform writes it.
    keys: Option<String>,
    command: CommandId,
}

/// On a menu entry: when it came, among all of them.
#[derive(Component, Clone, Copy)]
struct Order(u64);

/// How many menu entries have come.
#[derive(Resource, Default)]
struct Placed(u64);

/// The menu open, and the rect of its title in the window's logical
/// pixels.
#[derive(Resource, Clone, PartialEq, Default)]
struct OpenMenu(Option<(Menu, Rect)>);

/// On a menu's title: the title of the menu it opens.
#[derive(Component, Clone)]
struct MenuTitle(String);

/// The titles of the menus, for the bar.
pub fn view() -> AnyView {
    keyed(resource::<MenuBar, _>(Clone::clone), |bar| {
        row(bar.0.iter().map(title).collect::<Vec<_>>())
            .gap(0.0)
            .height(percent(100.0))
            .boxed()
    })
    .boxed()
}

/// A menu's title. It sits over the backdrop of an open menu, so it
/// still takes the pointer while one is open: a press opens its menu,
/// or closes it if it is the one open.
fn title(menu: &Menu) -> AnyView {
    let name = menu.title.clone();
    title_button(menu.title.to_uppercase(), false)
        .tagged(MenuTitle(name.clone()))
        .tagged(GlobalZIndex(MENU_Z))
        .on_activate_with(move |world, node| {
            let shown = world
                .resource::<OpenMenu>()
                .0
                .as_ref()
                .is_some_and(|(menu, _)| menu.title == name);
            if shown {
                world.resource_mut::<OpenMenu>().0 = None;
            } else {
                open(world, &name, node);
            }
        })
        .boxed()
}

/// While a menu is open, opens the menu of the title the pointer
/// moves onto in its place.
fn follow_pointer(
    open_menu: Res<OpenMenu>,
    titles: Query<(Entity, &MenuTitle), Added<Hovered>>,
    mut commands: Commands,
) {
    let Some((menu, _)) = &open_menu.0 else {
        return;
    };
    for (node, title) in &titles {
        if title.0 != menu.title {
            let name = title.0.clone();
            commands.queue(move |world: &mut World| {
                open(world, &name, node);
            });
        }
    }
}

/// The open menu's list, over everything, while one is open.
pub fn open_view() -> AnyView {
    keyed(resource::<OpenMenu, _>(Clone::clone), |open| {
        let Some((menu, anchor)) = open.0.clone() else {
            return frame().boxed();
        };
        let items = menu.items.iter().map(item).collect::<Vec<_>>();
        AnyView::new(move |cx: &mut Cx| {
            let width = cx.theme().space.menu_width;
            cx.build(
                popup(anchor, items)
                    .on_dismiss(|world, _| dismiss(world))
                    .min_width(px(width)),
            )
        })
    })
    .within(overlay(()))
    .boxed()
}

/// An entry: its command's label, and its first key at the end.
fn item(item: &Item) -> AnyView {
    let command = item.command.clone();
    let words = item.label.clone();
    let keys = item.keys.clone().unwrap_or_default();
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.menu_gap;
        cx.build(
            menu_item(
                row((
                    label(words).wrap(false).grown(1.0),
                    label(keys).wrap(false).tone(Tone::Dim),
                ))
                .gap(gap)
                .grow(1.0)
                .align(AlignItems::Center),
            )
            .on_activate(move |world| {
                world.resource_mut::<OpenMenu>().0 = None;
                command::run(world, &command);
            }),
        )
    })
}

/// The rect of `node`, in the window's logical pixels.
fn rect_of(world: &World, node: Entity) -> Option<Rect> {
    let (computed, at) = world
        .get::<ComputedNode>(node)
        .zip(world.get::<UiGlobalTransform>(node))?;
    let scale = computed.inverse_scale_factor();
    let (_, _, centre) = at.to_scale_angle_translation();
    Some(Rect::from_center_size(
        centre * scale,
        computed.size() * scale,
    ))
}

/// Opens the menu titled `name` under its title, `node`.
fn open(world: &mut World, name: &str, node: Entity) {
    let Some(rect) = rect_of(world, node) else {
        return;
    };
    let menu = world
        .resource::<MenuBar>()
        .0
        .iter()
        .find(|menu| menu.title == name)
        .cloned();
    if let Some(menu) = menu {
        world.resource_mut::<OpenMenu>().0 = Some((menu, rect));
    }
}

/// Closes the open menu: a press anywhere but a title, or Escape.
fn dismiss(world: &mut World) {
    world.resource_mut::<OpenMenu>().0 = None;
}

/// Holds a title lit while its menu is open.
fn light_titles(
    open: Res<OpenMenu>,
    mut titles: Query<(&MenuTitle, &mut BottomLight)>,
) {
    for (title, mut light) in &mut titles {
        let held = open
            .0
            .as_ref()
            .is_some_and(|(menu, _)| menu.title == title.0);
        if light.held != held {
            light.held = held;
        }
    }
}

fn place(
    added: On<Add, MenuEntry>,
    mut placed: ResMut<Placed>,
    mut commands: Commands,
) {
    placed.0 += 1;
    commands.entity(added.entity).insert(Order(placed.0));
}

/// Gathers the menus again when an entry, a command or a key comes,
/// changes or goes.
fn gather(
    changed: Query<
        (),
        Or<(
            Changed<MenuEntry>,
            Changed<Order>,
            Changed<Command>,
            Changed<Shortcut>,
        )>,
    >,
    mut gone: RemovedComponents<MenuEntry>,
    mut gone_commands: RemovedComponents<Command>,
    mut gone_keys: RemovedComponents<Shortcut>,
    entries: Query<(&MenuEntry, &Order)>,
    commands: Query<&Command>,
    shortcuts: Query<&Shortcut>,
    mut bar: ResMut<MenuBar>,
) {
    let went = gone.read().count()
        + gone_commands.read().count()
        + gone_keys.read().count();
    if changed.is_empty() && went == 0 {
        return;
    }
    let mut placed = entries.iter().collect::<Vec<_>>();
    placed.sort_by_key(|(_, order)| order.0);
    let mut menus = Vec::<Menu>::new();
    for (entry, _) in placed {
        let Some(command) =
            commands.iter().find(|c| c.id == entry.command)
        else {
            continue;
        };
        let keys = shortcuts
            .iter()
            .find(|shortcut| shortcut.command == entry.command)
            .map(|shortcut| shortcut.chord.to_string());
        let item = Item {
            label: command.label.clone(),
            keys,
            command: entry.command.clone(),
        };
        match menus.iter_mut().find(|menu| menu.title == entry.menu) {
            Some(menu) => menu.items.push(item),
            None => menus.push(Menu {
                title: entry.menu.clone(),
                items: vec![item],
            }),
        }
    }
    // A stable sort keeps the others in the order they came.
    menus.sort_by_key(|menu| {
        OWN.iter()
            .position(|own| *own == menu.title)
            .unwrap_or(OWN.len())
    });
    let now = MenuBar(menus);
    if *bar != now {
        *bar = now;
    }
}
