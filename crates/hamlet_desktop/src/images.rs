//! The system's dialog image files are chosen in.

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

/// Asks the chief to choose image files for `asker` in the system's
/// dialog. A dialog already open asks nothing.
#[derive(Event, Clone, Debug)]
pub struct ChooseImages {
    pub asker: Entity,
}

/// The image files the chief chose for `asker`.
#[derive(Event, Clone, Debug)]
pub struct ImagesChosen {
    pub asker: Entity,
    pub paths: Vec<PathBuf>,
}

/// The asker the files were chosen for, and the files. None when
/// the dialog was closed with nothing chosen.
type Chosen = (Entity, Option<Vec<PathBuf>>);

/// The dialog image files are chosen in.
#[derive(Resource)]
struct Picker {
    asked: Option<ChooseImages>,
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

fn ask(choose: On<ChooseImages>, mut picker: ResMut<Picker>) {
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
        .add_filter("Images", &hamlet_client::images::ENDINGS)
        .pick_files();
    let tx = picker.tx.clone();
    tokio.0.spawn(async move {
        let files = chosen.await.map(|files| {
            files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect::<Vec<_>>()
        });
        let _ = tx.send((asked.asker, files));
    });
}

/// Tells of the files a dialog gave.
fn take(mut picker: ResMut<Picker>, mut commands: Commands) {
    while let Ok((asker, files)) = picker.rx.try_recv() {
        picker.open = false;
        if let Some(paths) = files {
            commands.trigger(ImagesChosen { asker, paths });
        }
    }
}
