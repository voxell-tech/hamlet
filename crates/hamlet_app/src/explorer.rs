//! The Files panel, after a VS Code window: the folder a chosen
//! hamlet works on as a tree, kept up with the disk, and the file
//! opened from it beside it. The tree is in `tree`, kept up by
//! `sync`; the open file is in `editor`; both are drawn by `view` and
//! pressed by `acts`.
//!
//! The window reads the folder from its own disk, so a hamlet that
//! works on a folder of another machine has no tree here.

mod acts;
mod editor;
mod page;
mod sync;
mod tree;
mod view;

use std::path::Path;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy_fynix::dock::{DockTree, DockWindowKind};
use hamlet_client::realm::{HamletId, Works};
use hamlet_ui::styles::ICON;
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::icons::Glyph;

use crate::dock::{PanelAppExt, panel, show_tab};

/// The Files panel's id in the dock.
pub const PANEL: &str = "files";

/// The world's id in the dock.
const WORLD: &str = "world";

pub fn plugin(app: &mut App) {
    // Closed until asked for, from an agent's page or a tab bar's
    // plus.
    app.add_panel(PANEL, window())
        .init_resource::<Browsing>()
        .add_plugins((sync::plugin, editor::plugin));
}

/// The Files panel as a dock window.
pub fn window() -> DockWindowKind<HamletTheme> {
    DockWindowKind::new("FILES", || panel(view::panel()))
        .icon(Glyph::Folder.image(ICON))
}

/// The hamlet whose files the panel shows. None for the first.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
pub struct Browsing(pub Option<Entity>);

/// Shows the files of `hamlet` in the panel, and brings it to the
/// front: beside the world when the desk shown does not have it,
/// where there is the most room.
pub(crate) fn browse(world: &mut World, hamlet: Entity) {
    world.resource_mut::<Browsing>().0 = Some(hamlet);
    let mut tree = world.resource_mut::<DockTree>();
    if tree.find_leaf_with_window(PANEL).is_none()
        && let Some(centre) = tree.find_leaf_with_window(WORLD)
    {
        tree.add_tab(centre, PANEL);
        return;
    }
    show_tab(world, PANEL);
}

/// Opens every folder of `hamlet`'s tree down to `path`, and selects
/// it.
pub fn reveal(world: &mut World, hamlet: Entity, path: &Path) {
    let Some(folder) = folder(world, hamlet) else {
        return;
    };
    sync::open(world, hamlet, Path::new(&folder));
    let path =
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(mut explorer) =
        world.get_mut::<sync::Explorer>(hamlet)
    {
        explorer.0.reveal(&path);
    }
}

/// Opens the file `path` in `hamlet`'s editor, with its folders open
/// down to it in the tree.
pub fn show_file(world: &mut World, hamlet: Entity, path: &Path) {
    reveal(world, hamlet, path);
    let path =
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    acts::open(world, hamlet, &path);
}

/// Every hamlet and its name, by name.
fn hamlets(world: &World) -> Vec<(Entity, String)> {
    let Some(mut query) =
        world.try_query::<(Entity, &HamletId, &Name)>()
    else {
        return Vec::new();
    };
    let mut hamlets = query
        .iter(world)
        .map(|(hamlet, _, name)| (hamlet, name.as_str().to_string()))
        .collect::<Vec<_>>();
    hamlets.sort_by_key(|(_, name)| name.to_lowercase());
    hamlets
}

/// The hamlet shown: the one chosen while it stands, else the
/// first by name.
fn shown(world: &World) -> Option<Entity> {
    world
        .resource::<Browsing>()
        .0
        .filter(|hamlet| world.get::<HamletId>(*hamlet).is_some())
        .or_else(|| hamlets(world).first().map(|(hamlet, _)| *hamlet))
}

/// The folder `hamlet` works on, as the server names it.
fn folder(world: &World, hamlet: Entity) -> Option<String> {
    world
        .get::<Works>(hamlet)
        .and_then(|works| works.folder.clone())
}

#[cfg(test)]
mod tests {
    use bevy_fynix::dock::{DockAreaStyle, DockLeaf, DockTree};
    use hamlet_client::realm::hamlet;
    use hamlet_protocol::client as protocol;

    use super::*;
    use crate::testing::panel_app;

    #[test]
    fn a_hamlet_with_no_folder_offers_to_choose_one() {
        #[derive(bevy::ecs::resource::Resource, Default)]
        struct Asked(Vec<Entity>);
        let mut app = panel_app();
        app.init_resource::<Browsing>()
            .init_resource::<Asked>()
            .add_observer(
                |ask: bevy::ecs::observer::On<
                    hamlet_desktop::folder::ChooseFolder,
                >,
                 mut asked: bevy::ecs::system::ResMut<Asked>| {
                    asked.0.push(ask.asker);
                },
            );
        app.update();
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            view::panel(),
        );
        app.update();

        crate::testing::press(&mut app, "Choose a folder");

        let home = hamlet(app.world(), "home").unwrap();
        assert_eq!(app.world().resource::<Asked>().0, [home]);
    }

    #[test]
    fn an_agents_folder_button_shows_its_hamlet_in_the_explorer() {
        let mut app = panel_app();
        app.init_resource::<Browsing>();
        let mut state = hamlet_testing::realm::state();
        let mut quay = hamlet_testing::realm::hamlet("quay", 20);
        quay.name = "The Quay".into();
        state.hamlets.push(quay);
        state
            .agents
            .push(hamlet_testing::realm::agent(20, "quay", 0));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        let world = app.world_mut();
        world.resource_mut::<DockTree>().set_root_leaf(
            DockLeaf::new("centre", DockAreaStyle::TabBar)
                .with_windows(vec!["world".into()]),
        );
        let (home, quay) = (
            hamlet(world, "home").unwrap(),
            hamlet(world, "quay").unwrap(),
        );
        assert_eq!(shown(world), Some(home));

        browse(world, quay);

        assert_eq!(shown(world), Some(quay));
        assert!(
            world
                .resource::<DockTree>()
                .find_leaf_with_window(PANEL)
                .is_some()
        );
        world.despawn(quay);
        assert_eq!(shown(world), Some(home));
    }
}
