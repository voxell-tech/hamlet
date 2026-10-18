//! Bringing a panel's tab to the front.

use bevy::ecs::event::Event;
use bevy::ecs::world::World;
use bevy_fynix::dock::DockTree;

/// The tab of the window of this id was brought to the front.
#[derive(Event)]
pub struct TabShown(pub String);

/// Brings a panel's tab to the front. A panel that is closed is
/// added to the first area that is not the world's.
pub fn open(world: &mut World, id: &str) {
    let Some(mut tree) = world.get_resource_mut::<DockTree>() else {
        return;
    };
    if tree.find_leaf_with_window(id).is_none() {
        let area = tree
            .leaves()
            .filter(|(_, leaf)| !leaf.has_window("world"))
            .map(|(node, _)| node)
            .min_by_key(|node| node.0);
        if let Some(area) = area {
            tree.add_tab(area, id);
        }
    }
    show_tab(world, id);
}

/// Brings a panel's tab to the front.
pub fn show_tab(world: &mut World, id: &str) {
    let Some(mut tree) = world.get_resource_mut::<DockTree>() else {
        return;
    };
    let Some(leaf) = tree.find_leaf_with_window(id) else {
        return;
    };
    let tab = tree.leaf(leaf).and_then(|l| {
        l.tabs()
            .find(|(window, _)| *window == id)
            .map(|(_, tab)| tab)
    });
    if let Some(tab) = tab {
        tree.set_active(leaf, tab);
    }
    world.trigger(TabShown(id.to_string()));
}
