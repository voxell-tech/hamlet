//! The menus of the bar, gathered from the menu entries there are.

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::lifecycle::{Add, RemovedComponents};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, Or};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ui::{UiRect, px};
use bevy_fynix::views::{
    FrameProps, MenuEntry as Row, menu_button, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_engine_api::CommandId;

use crate::AnyView;
use crate::command::{self, Command, MenuEntry, Shortcut};

/// The engine's own menus, first and in this order.
const OWN: [&str; 3] = ["Hamlet", "Workspace", "View"];

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<MenuBar>()
        .init_resource::<Placed>()
        .add_observer(place)
        .add_systems(Update, gather);
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

/// The menus, for the bar.
pub fn view() -> AnyView {
    keyed(resource::<MenuBar, _>(Clone::clone), |bar| {
        row(bar.0.iter().map(title).collect::<Vec<_>>())
            .gap(0.0)
            .boxed()
    })
    .boxed()
}

fn title(menu: &Menu) -> AnyView {
    let commands = menu
        .items
        .iter()
        .map(|item| item.command.clone())
        .collect::<Vec<_>>();
    let rows = menu.items.iter().map(|item| match &item.keys {
        Some(keys) => Row::Noted(item.label.clone(), keys.clone()),
        None => Row::Item(item.label.clone()),
    });
    menu_button(menu.title.to_uppercase(), rows, move |world, at| {
        if let Some(id) = commands.get(at) {
            command::run(world, id);
        }
    })
    .padding(UiRect::axes(px(12.0), px(0.0)))
    .boxed()
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
