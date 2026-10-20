//! The system's dialog a folder is chosen in, one at a time: for a
//! plugin to be added from, or for a plugin's node that asks for one.
//! It is opened on the main thread and waited for off the frame.

use std::path::PathBuf;

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, NonSendMarker, ResMut};
use bevy::tasks::futures::check_ready;
use bevy::tasks::{IoTaskPool, Task};

use crate::add::AddFrom;
use crate::instance::NodeInput;
use crate::retained::NodeAction;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Dialog>()
        .add_observer(ask)
        .add_systems(Update, (open, chosen));
}

/// Whom a folder is chosen for.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Asker {
    /// A plugin to be added from it.
    Adding,
    /// The node `key` of `plugin`, told of it.
    Node { plugin: Entity, key: u32 },
}

/// Asks for a folder, chosen in the system's dialog titled `title`,
/// unless the dialog is open already.
#[derive(Event, Clone, Debug)]
pub(crate) struct ChooseFolder {
    pub(crate) title: String,
    pub(crate) asker: Asker,
}

/// The dialog: asked for, and open.
#[derive(Resource, Default)]
struct Dialog {
    asked: Option<ChooseFolder>,
    open: Option<(Asker, Task<Option<PathBuf>>)>,
}

fn ask(asked: On<ChooseFolder>, mut dialog: ResMut<Dialog>) {
    if dialog.open.is_none() {
        dialog.asked = Some(asked.clone());
    }
}

/// Opens the dialog when it is asked for. The marker keeps this on
/// the main thread, where macOS wants a dialog begun.
fn open(_main: NonSendMarker, mut dialog: ResMut<Dialog>) {
    if dialog.open.is_some() {
        return;
    }
    let Some(asked) = dialog.asked.take() else {
        return;
    };
    let chosen = rfd::AsyncFileDialog::new()
        .set_title(asked.title)
        .pick_folder();
    dialog.open = Some((
        asked.asker,
        IoTaskPool::get().spawn(async move {
            chosen.await.map(|folder| folder.path().to_path_buf())
        }),
    ));
}

/// Hands the folder the dialog gave, if one was chosen, to whom it
/// was chosen for.
fn chosen(
    mut dialog: ResMut<Dialog>,
    mut actions: MessageWriter<NodeAction>,
    mut commands: Commands,
) {
    let Some((asker, open)) = &mut dialog.open else {
        return;
    };
    let Some(folder) = check_ready(open) else {
        return;
    };
    let asker = *asker;
    dialog.open = None;
    let Some(folder) = folder else {
        return;
    };
    match asker {
        Asker::Adding => commands.trigger(AddFrom { folder }),
        Asker::Node { plugin, key } => {
            actions.write(NodeAction {
                plugin,
                key,
                input: NodeInput::Chosen(
                    folder.to_string_lossy().into_owned(),
                ),
            });
        }
    }
}
