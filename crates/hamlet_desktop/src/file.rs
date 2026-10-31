//! The system's dialogs a file is opened from or saved to.

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

/// Whether a file is to be opened or saved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileMode {
    Open,
    Save,
}

/// Asks the chief to choose a file for `asker` in the system's
/// dialog, which says `title`, among files ending in `extension`. A
/// save starts at the name `name`. A dialog already open asks
/// nothing.
#[derive(Event, Clone, Debug)]
pub struct ChooseFile {
    pub asker: Entity,
    pub mode: FileMode,
    pub title: String,
    pub extension: String,
    pub name: String,
}

/// The file the chief chose for `asker`.
#[derive(Event, Clone, Debug)]
pub struct FileChosen {
    pub asker: Entity,
    pub mode: FileMode,
    pub path: PathBuf,
}

/// The asker a file was chosen for, how, and the file. None when the
/// dialog was closed with nothing chosen.
type Chosen = (Entity, FileMode, Option<PathBuf>);

/// The dialog a file is chosen in.
#[derive(Resource)]
struct Picker {
    asked: Option<ChooseFile>,
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

fn ask(choose: On<ChooseFile>, mut picker: ResMut<Picker>) {
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
    let dialog = rfd::AsyncFileDialog::new()
        .set_title(asked.title)
        .add_filter(
            asked.extension.clone(),
            &[asked.extension.as_str()],
        );
    let (asker, mode) = (asked.asker, asked.mode);
    let tx = picker.tx.clone();
    match mode {
        FileMode::Open => {
            let chosen = dialog.pick_file();
            tokio.0.spawn(async move {
                let path =
                    chosen.await.map(|f| f.path().to_path_buf());
                let _ = tx.send((asker, mode, path));
            });
        }
        FileMode::Save => {
            let chosen = dialog.set_file_name(asked.name).save_file();
            tokio.0.spawn(async move {
                let path =
                    chosen.await.map(|f| f.path().to_path_buf());
                let _ = tx.send((asker, mode, path));
            });
        }
    }
}

/// Tells of the file a dialog gave.
fn take(mut picker: ResMut<Picker>, mut commands: Commands) {
    while let Ok((asker, mode, path)) = picker.rx.try_recv() {
        picker.open = false;
        if let Some(path) = path {
            commands.trigger(FileChosen { asker, mode, path });
        }
    }
}
