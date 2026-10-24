//! The system's dialog a folder is chosen in.

use std::path::PathBuf;

use bevy_app::{App, Update};
use bevy_ecs::entity::Entity;
use bevy_ecs::event::Event;
use bevy_ecs::observer::On;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, NonSendMarker, Res, ResMut};
use hamlet_client::Tokio;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Picker>()
        .add_observer(ask)
        .add_systems(Update, (open, take));
}

/// Asks the chief to choose a folder for `asker` in the system's
/// dialog, which says `title`. A dialog already open asks nothing.
#[derive(Event, Clone, Debug)]
pub struct ChooseFolder {
    pub asker: Entity,
    pub title: String,
}

/// The folder the chief chose for `asker`.
#[derive(Event, Clone, Debug)]
pub struct FolderChosen {
    pub asker: Entity,
    pub folder: PathBuf,
}

/// The asker a folder was chosen for, and the folder. None when the
/// dialog was closed with nothing chosen.
type Chosen = (Entity, Option<PathBuf>);

/// The dialog a folder is chosen in.
#[derive(Resource)]
struct Picker {
    asked: Option<ChooseFolder>,
    /// A dialog is open.
    open: bool,
    tx: UnboundedSender<Chosen>,
    rx: UnboundedReceiver<Chosen>,
}

impl Default for Picker {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Picker {
            asked: None,
            open: false,
            tx,
            rx,
        }
    }
}

fn ask(choose: On<ChooseFolder>, mut picker: ResMut<Picker>) {
    picker.asked = Some(choose.event().clone());
}

/// Opens the dialog for whoever asked. The marker keeps this on the
/// main thread, where macOS wants a dialog begun.
fn open(
    _main: NonSendMarker,
    mut picker: ResMut<Picker>,
    tokio: Option<Res<Tokio>>,
) {
    if picker.open {
        picker.asked = None;
        return;
    }
    let (Some(asked), Some(tokio)) = (picker.asked.take(), tokio)
    else {
        return;
    };
    picker.open = true;
    let chosen = rfd::AsyncFileDialog::new()
        .set_title(asked.title)
        .pick_folder();
    let tx = picker.tx.clone();
    tokio.0.spawn(async move {
        let folder = chosen.await.map(|f| f.path().to_path_buf());
        let _ = tx.send((asked.asker, folder));
    });
}

/// Tells of the folder a dialog gave.
fn take(mut picker: ResMut<Picker>, mut commands: Commands) {
    while let Ok((asker, folder)) = picker.rx.try_recv() {
        picker.open = false;
        if let Some(folder) = folder {
            commands.trigger(FolderChosen { asker, folder });
        }
    }
}
