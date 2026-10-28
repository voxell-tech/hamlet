//! The open menus of the gallery.

use bevy::ecs::world::World;
use bevy::math::Rect;
use hamlet_app::menu;

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    let open = match scene {
        Scene::Menu => Some((menu::Menu::App, 8.0, 95.0)),
        Scene::ViewMenu => Some((menu::Menu::View, 104.0, 176.0)),
        _ => None,
    };
    if let Some((which, left, right)) = open {
        world.insert_resource(menu::OpenMenu::under(
            which,
            Rect::new(left, 0.0, right, 34.0),
        ));
    }
}
