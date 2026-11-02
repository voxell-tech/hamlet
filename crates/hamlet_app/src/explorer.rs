//! The Explorer panel: the folder a chosen hamlet works on as a tree,
//! after VS Code's, kept up with the disk. The tree is in `tree`,
//! kept up by `sync`, drawn by `view` and pressed by `acts`.
//!
//! The window reads the folder from its own disk, so a hamlet that
//! works on a folder of another machine has no tree here.

mod acts;
mod sync;
mod tree;
mod view;

use std::path::Path;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy_fynix::dock::DockWindowKind;
use hamlet_client::realm::{HOME, HamletId, Works};
use hamlet_ui::styles::ICON;
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::icons::Glyph;

use crate::dock::{PanelAppExt, panel, show_tab};

/// The Explorer's id in the dock.
pub const PANEL: &str = "explorer";

pub fn plugin(app: &mut App) {
    // Closed until asked for, from an agent's page or a tab bar's
    // plus.
    app.add_panel(PANEL, window())
        .init_resource::<Browsing>()
        .add_plugins(sync::plugin);
}

/// The Explorer as a dock window.
pub fn window() -> DockWindowKind<HamletTheme> {
    DockWindowKind::new("EXPLORER", || panel(view::panel()))
        .icon(Glyph::Folder.image(ICON))
}

/// The hamlet whose files the Explorer shows. None for the first.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
pub struct Browsing(pub Option<Entity>);

/// Shows the files of `hamlet` in the Explorer, and brings it to the
/// front.
pub(crate) fn browse(world: &mut World, hamlet: Entity) {
    world.resource_mut::<Browsing>().0 = Some(hamlet);
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

/// Every hamlet and its name: the first hamlet first, then by name.
fn hamlets(world: &World) -> Vec<(Entity, String)> {
    let Some(mut query) =
        world.try_query::<(Entity, &HamletId, &Name)>()
    else {
        return Vec::new();
    };
    let mut hamlets = query
        .iter(world)
        .map(|(hamlet, id, name)| {
            (id.0 != HOME, name.as_str().to_string(), hamlet)
        })
        .collect::<Vec<_>>();
    hamlets.sort_by_key(|(later, name, _)| {
        (*later, name.to_lowercase())
    });
    hamlets
        .into_iter()
        .map(|(_, name, hamlet)| (hamlet, name))
        .collect()
}

/// The hamlet shown: the one chosen while it stands, else the first.
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
    fn an_agents_folder_button_shows_its_hamlet_in_the_explorer() {
        let mut app = panel_app();
        app.init_resource::<Browsing>();
        let mut state = hamlet_testing::realm::state();
        let mut quay = hamlet_testing::realm::hamlet("quay");
        quay.name = "The Quay".into();
        state.hamlets.push(quay);
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
