//! The system's dialog the files or the folders to attach to a line
//! are chosen in (C33).

use std::path::PathBuf;

use bevy_app::{App, Update};
use bevy_ecs::entity::Entity;
use bevy_ecs::event::Event;
use bevy_ecs::observer::On;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, NonSendMarker, Res, ResMut};
use hamlet_client::Tokio;
use hamlet_protocol::client::ImageInput;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Picker>()
        .add_observer(ask)
        .add_systems(Update, (open, take));
}

/// The kind of thing a dialog chooses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum What {
    /// Files of the kinds an image is taken in.
    Images,
    /// Files of any kind.
    Files,
    Folders,
}

/// Asks the chief to choose files, or folders, for `asker` in the
/// system's dialog. A dialog already open asks nothing.
#[derive(Event, Clone, Debug)]
pub struct Choose {
    pub asker: Entity,
    pub what: What,
}

/// The files or folders the chief chose for `asker`.
#[derive(Event, Clone, Debug)]
pub struct Chosen {
    pub asker: Entity,
    pub paths: Vec<PathBuf>,
}

/// The asker the paths were chosen for, and the paths. None when the
/// dialog was closed with nothing chosen.
type Picked = (Entity, Option<Vec<PathBuf>>);

/// The dialog paths are chosen in.
#[derive(Resource)]
struct Picker {
    asked: Option<Choose>,
    /// A dialog is open.
    open: bool,
    tx: UnboundedSender<Picked>,
    rx: UnboundedReceiver<Picked>,
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

fn ask(choose: On<Choose>, mut picker: ResMut<Picker>) {
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
    let dialog = rfd::AsyncFileDialog::new();
    let tx = picker.tx.clone();
    let asker = asked.asker;
    let what = asked.what;
    let chosen = async move {
        let handles = match what {
            What::Folders => dialog.pick_folders().await,
            What::Files => dialog.pick_files().await,
            What::Images => {
                dialog
                    .add_filter("Images", &ImageInput::ENDINGS)
                    .pick_files()
                    .await
            }
        };
        handles.map(|handles| {
            handles
                .iter()
                .map(|handle| handle.path().to_path_buf())
                .collect::<Vec<_>>()
        })
    };
    tokio.0.spawn(async move {
        let _ = tx.send((asker, chosen.await));
    });
}

/// Tells of the paths a dialog gave.
fn take(mut picker: ResMut<Picker>, mut commands: Commands) {
    while let Ok((asker, paths)) = picker.rx.try_recv() {
        picker.open = false;
        if let Some(paths) = paths {
            commands.trigger(Chosen { asker, paths });
        }
    }
}
