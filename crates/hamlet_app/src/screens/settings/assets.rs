//! The Assets page: the folder the chief keeps assets in, shown in
//! the system's file browser or chosen again, and whether the realm's
//! houses are built with them.

use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::world::World;
use bevy_fynix::views::TooltipExt;
use bevy_fynix::{ViewExt, derived, resource};
use hamlet_desktop::folder::ChooseFolder;
use hamlet_desktop::open;
use hamlet_ui::styles::{action, buttons, description, note, parts};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::switch::switch;
use hamlet_ui::{AnyView, View};

use super::{field, told};
use crate::assets_folder::{
    Asker, AssetChoice, DefaultAssets, folder,
};
use crate::roll::moved;

pub(super) fn page() -> impl View {
    parts((
        field(
            "Folder",
            told(
                buttons(vec![show(), choose(), default()]),
                description(
                    derived(|world: &World| {
                        now(world).map_or_else(
                            || "None yet.".to_owned(),
                            |folder| folder.display().to_string(),
                        )
                    })
                    .when(moved::<AssetChoice>()),
                ),
            ),
        ),
        field(
            "Use in the realm",
            told(
                switch(
                    resource::<AssetChoice, _>(|choice| {
                        choice.in_realm
                    }),
                    |world, on| {
                        world
                            .resource_mut::<AssetChoice>()
                            .in_realm = on;
                    },
                ),
                description(
                    "Houses are built with your blueprints and \
                     meshes, each standing in for the built-in one \
                     of the same name.",
                ),
            ),
        ),
    ))
}

/// The folder the library reads now.
fn now(world: &World) -> Option<std::path::PathBuf> {
    folder(
        world.resource::<AssetChoice>(),
        world.get_resource::<DefaultAssets>(),
    )
}

/// Opens the folder in the system's file browser, made first if it
/// is not there yet.
fn show() -> AnyView {
    action(Glyph::Eye, "Show", |world| {
        if let Some(folder) = now(world)
            && std::fs::create_dir_all(&folder).is_ok()
        {
            open::show_file(&folder);
        }
    })
    .boxed()
}

fn choose() -> AnyView {
    action(Glyph::Folder, "Choose another", |world| {
        let asker = world
            .query_filtered::<Entity, With<Asker>>()
            .iter(world)
            .next();
        if let Some(asker) = asker {
            world.trigger(ChooseFolder {
                asker,
                title: "Choose the folder to keep your assets in"
                    .into(),
            });
        }
    })
    .boxed()
}

/// Goes back to the folder kept until one is chosen.
fn default() -> AnyView {
    action(Glyph::Undo, "Use the default", |world| {
        world.resource_mut::<AssetChoice>().folder = None;
    })
    .tooltip(|| {
        note(
            "Hamlet in your documents folder, unless HAMLET_ASSETS \
             names another",
        )
    })
    .boxed()
}
