//! The images on the system's clipboard (C33): image files copied in
//! a file manager, and raw picture data such as a screenshot.

use std::path::PathBuf;

use bevy_app::{App, Update};
use bevy_ecs::entity::Entity;
use bevy_ecs::event::Event;
use bevy_ecs::observer::On;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Res, ResMut};
use hamlet_client::Tokio;
use hamlet_client::images::is_image_file;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Reader>()
        .add_observer(ask)
        .add_systems(Update, take);
}

/// Asks for the images on the clipboard, for `asker`.
#[derive(Event, Clone, Debug)]
pub struct PasteImages {
    pub asker: Entity,
}

/// The images the clipboard held for `asker`: image files, or the
/// bytes of PNG files made from picture data. Both are empty when it
/// held no image.
#[derive(Event, Clone, Debug)]
pub struct ImagesPasted {
    pub asker: Entity,
    pub files: Vec<PathBuf>,
    pub pictures: Vec<Vec<u8>>,
}

type Found = (Entity, Vec<PathBuf>, Vec<Vec<u8>>);

/// The reads of the clipboard going on, each on a thread of its own.
#[derive(Resource)]
struct Reader {
    tx: UnboundedSender<Found>,
    rx: UnboundedReceiver<Found>,
}

impl Default for Reader {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Reader { tx, rx }
    }
}

fn ask(
    paste: On<PasteImages>,
    reader: Res<Reader>,
    tokio: Option<Res<Tokio>>,
) {
    let Some(tokio) = tokio else {
        return;
    };
    let (asker, tx) = (paste.asker, reader.tx.clone());
    tokio.0.spawn_blocking(move || {
        let (files, pictures) = read();
        let _ = tx.send((asker, files, pictures));
    });
}

/// Tells of the images a read found.
fn take(mut reader: ResMut<Reader>, mut commands: Commands) {
    while let Ok((asker, files, pictures)) = reader.rx.try_recv() {
        if !files.is_empty() || !pictures.is_empty() {
            commands.trigger(ImagesPasted {
                asker,
                files,
                pictures,
            });
        }
    }
}

/// The image files, else the picture, on the clipboard.
fn read() -> (Vec<PathBuf>, Vec<Vec<u8>>) {
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        return Default::default();
    };
    if let Ok(files) = clipboard.get().file_list() {
        let images = files
            .into_iter()
            .filter(|file| is_image_file(file))
            .collect::<Vec<_>>();
        if !images.is_empty() {
            return (images, Vec::new());
        }
    }
    // Words on the clipboard are pasted as words, though what they
    // were copied from may have left a picture of them beside.
    if clipboard.get_text().is_ok_and(|text| !text.is_empty()) {
        return Default::default();
    }
    let Ok(picture) = clipboard.get_image() else {
        return Default::default();
    };
    let mut png = Vec::new();
    let encoded = u32::try_from(picture.width)
        .ok()
        .zip(u32::try_from(picture.height).ok())
        .map(|(width, height)| {
            PngEncoder::new(&mut png).write_image(
                &picture.bytes,
                width,
                height,
                ExtendedColorType::Rgba8,
            )
        });
    match encoded {
        Some(Ok(())) => (Vec::new(), vec![png]),
        _ => Default::default(),
    }
}
