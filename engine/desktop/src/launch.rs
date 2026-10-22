//! One engine per home: a later launch asks the running one to show
//! its window, and ends.

use std::fs;

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, ResMut};
use hamlet_engine_core::Home;
use hamlet_engine_shell::ShowWindow;
use hamlet_home::instance::{self, Instance};
use tokio::sync::mpsc::UnboundedReceiver;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, show_asked);
}

/// The asks of later launches to show the window.
#[derive(Resource)]
pub struct Launches(UnboundedReceiver<()>);

/// Claims `home` for this process, with the asks of later launches.
/// None when an engine already runs there, which was asked to show
/// its window.
pub fn claim(home: &Home) -> Result<Option<Launches>, String> {
    fs::create_dir_all(home.root()).map_err(|why| why.to_string())?;
    // A home named from another folder is the same home.
    let root = std::path::absolute(home.root())
        .map_err(|why| why.to_string())?;
    match instance::claim(&root) {
        Ok(Instance::Primary(asks)) => Ok(Some(Launches(asks))),
        Ok(Instance::Secondary) => Ok(None),
        Err(why) => Err(format!("{why:#}")),
    }
}

/// Shows the window when a later launch asked for it.
fn show_asked(
    launches: Option<ResMut<Launches>>,
    mut commands: Commands,
) {
    let Some(mut launches) = launches else {
        return;
    };
    let mut asked = false;
    while launches.0.try_recv().is_ok() {
        asked = true;
    }
    if asked {
        commands.trigger(ShowWindow);
    }
}
