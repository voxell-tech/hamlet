//! The system's clipboard, read to attach (C33): files and folders
//! copied in a file manager, and raw picture data such as a
//! screenshot.

use std::path::PathBuf;

use bevy_app::{App, Update};
use bevy_ecs::entity::Entity;
use bevy_ecs::event::Event;
use bevy_ecs::observer::On;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Res, ResMut};
use hamlet_client::Tokio;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Reader>()
        .add_observer(ask)
        .add_observer(copy)
        .add_systems(Update, take);
}

/// Puts `html` on the clipboard, with `text` for what takes only
/// words.
#[derive(Event, Clone, Debug)]
pub struct CopyRich {
    pub text: String,
    pub html: String,
}

fn copy(copy: On<CopyRich>) {
    let CopyRich { text, html } = copy.event().clone();
    // On Linux what was copied lasts while its owner does, so the
    // thread holds it until another program takes the clipboard.
    std::thread::spawn(move || {
        let Ok(mut clipboard) = arboard::Clipboard::new() else {
            return;
        };
        let set = clipboard.set();
        #[cfg(target_os = "linux")]
        let set = {
            use arboard::SetExtLinux as _;
            set.wait()
        };
        let _ = set.html(html, Some(text));
    });
}

/// Asks for what is on the clipboard, for `asker`.
#[derive(Event, Clone, Debug)]
pub struct Paste {
    pub asker: Entity,
}

/// The files and folders, or the bytes of PNG files made from
/// picture data, the clipboard held for `asker`. Both are empty when
/// it held neither.
#[derive(Event, Clone, Debug)]
pub struct Pasted {
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
    paste: On<Paste>,
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

/// Tells of what a read found.
fn take(mut reader: ResMut<Reader>, mut commands: Commands) {
    while let Ok((asker, files, pictures)) = reader.rx.try_recv() {
        if !files.is_empty() || !pictures.is_empty() {
            commands.trigger(Pasted {
                asker,
                files,
                pictures,
            });
        }
    }
}

/// The files, else the picture, on the clipboard.
fn read() -> (Vec<PathBuf>, Vec<Vec<u8>>) {
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        return Default::default();
    };
    if let Ok(files) = clipboard.get().file_list()
        && !files.is_empty()
    {
        return (files, Vec::new());
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
