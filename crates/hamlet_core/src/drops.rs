//! Files and folders dropped on the window, told to the node they
//! fell on.

use std::path::PathBuf;

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::message::Message;

pub fn plugin(app: &mut App) {
    app.add_message::<FileDropped>();
}

/// A node, or any node in it, that takes the files dropped on it,
/// which the world's hand tells as [`FileDropped`].
#[derive(Component)]
pub struct FileDrop;

/// A file or folder dropped on a [`FileDrop`].
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct FileDropped {
    pub path: PathBuf,
}
