//! Bringing a panel's tab to the front.

use bevy::ecs::event::Event;
use bevy::ecs::world::World;
use bevy_fynix::dock::DockTree;

/// The tab of the window of this id was brought to the front.
#[derive(Event)]
pub struct TabShown(pub String);

/// Brings a panel's tab to the front.
pub fn show_tab(world: &mut World, id: &str) {
    let mut tree = world.resource_mut::<DockTree>();
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
