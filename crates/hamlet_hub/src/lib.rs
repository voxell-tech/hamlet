//! The Hub: the chat with agents, the realm's overview and the pages
//! a Hub shows, and the menu over the world's things.

pub mod asking;
pub mod chat;
pub mod convo;
mod folder;
pub mod hub;
pub mod links;
pub mod lists;
mod markdown;
mod naming;
pub mod overview;
pub mod pictures;
mod profile_tag;
mod reference;
pub mod select;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod viewer;
pub mod wait;
pub mod world_menu;

use bevy::app::{App, PreUpdate};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use hamlet_core::assets::TuningPlugin;
use hamlet_core::panels::PanelSystems;
use hamlet_core::pick::Selected;

/// Everything the Hub is made of.
pub fn plugin(app: &mut App) {
    app.add_plugins((
        TuningPlugin::<chat::ChatSettings>::default(),
        hub::plugin,
        overview::plugin,
        chat::plugin,
        links::plugin,
        pictures::plugin,
        viewer::plugin,
        wait::plugin,
        folder::plugin,
        asking::plugin,
        select::plugin,
        world_menu::plugin,
        reference::plugin,
    ))
    .add_systems(
        PreUpdate,
        (
            convo::reseat.run_if(convo::roster_moved),
            hub::open_first,
            hub::follow_selection
                .run_if(resource_changed::<Selected>),
            hub::tell_page,
        )
            .chain()
            .in_set(PanelSystems::Follow),
    );
}
