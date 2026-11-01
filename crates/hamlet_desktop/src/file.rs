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
        .add_observer(ask_many)
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

/// Asks the chief to choose any number of files for `asker` in the
/// system's dialog, which says `title`, among files ending in
/// `extension`. A dialog already open asks nothing.
#[derive(Event, Clone, Debug)]
pub struct ChooseFiles {
    pub asker: Entity,
    pub title: String,
    pub extension: String,
}

/// The files the chief chose for `asker`, at least one.
#[derive(Event, Clone, Debug)]
pub struct FilesChosen {
    pub asker: Entity,
    pub paths: Vec<PathBuf>,
}

/// What the chief is asked to choose.
enum Asked {
    One(ChooseFile),
    Many(ChooseFiles),
}

/// What a dialog gave. Empty when it was closed with nothing chosen.
enum Chosen {
    One(Entity, FileMode, Option<PathBuf>),
    Many(Entity, Vec<PathBuf>),
}

/// The dialog a file is chosen in.
#[derive(Resource)]
struct Picker {
    asked: Option<Asked>,
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
    picker.asked = Some(Asked::One(choose.event().clone()));
}

fn ask_many(choose: On<ChooseFiles>, mut picker: ResMut<Picker>) {
    picker.asked = Some(Asked::Many(choose.event().clone()));
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
    let tx = picker.tx.clone();
    match asked {
        Asked::One(asked) => {
            let dialog = filtered(asked.title, &asked.extension);
            let (asker, mode) = (asked.asker, asked.mode);
            match mode {
                FileMode::Open => {
                    let chosen = dialog.pick_file();
                    tokio.0.spawn(async move {
                        let path = chosen
                            .await
                            .map(|f| f.path().to_path_buf());
                        let _ =
                            tx.send(Chosen::One(asker, mode, path));
                    });
                }
                FileMode::Save => {
                    let chosen =
                        dialog.set_file_name(asked.name).save_file();
                    tokio.0.spawn(async move {
                        let path = chosen
                            .await
                            .map(|f| f.path().to_path_buf());
                        let _ =
                            tx.send(Chosen::One(asker, mode, path));
                    });
                }
            }
        }
        Asked::Many(asked) => {
            let chosen =
                filtered(asked.title, &asked.extension).pick_files();
            let asker = asked.asker;
            tokio.0.spawn(async move {
                let paths = chosen.await.unwrap_or_default();
                let paths = paths
                    .iter()
                    .map(|f| f.path().to_path_buf())
                    .collect();
                let _ = tx.send(Chosen::Many(asker, paths));
            });
        }
    }
}

/// A dialog that says `title` and shows files ending in `extension`.
fn filtered(title: String, extension: &str) -> rfd::AsyncFileDialog {
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .add_filter(extension, &[extension])
}

/// Tells of the files a dialog gave.
fn take(mut picker: ResMut<Picker>, mut commands: Commands) {
    while let Ok(chosen) = picker.rx.try_recv() {
        picker.open = false;
        match chosen {
            Chosen::One(asker, mode, Some(path)) => {
                commands.trigger(FileChosen { asker, mode, path });
            }
            Chosen::Many(asker, paths) if !paths.is_empty() => {
                commands.trigger(FilesChosen { asker, paths });
            }
            Chosen::One(..) | Chosen::Many(..) => {}
        }
    }
}
