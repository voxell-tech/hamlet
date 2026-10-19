//! The system's dialog one file is chosen in.

use std::path::PathBuf;

use bevy::ecs::resource::Resource;
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedSender;

/// Opens a dialog titled `title` for one file and sends the file
/// chosen, or none when it is closed with nothing, on `chosen`. It is
/// begun on the main thread and waits on `tokio`.
pub type Open = fn(
    title: String,
    chosen: UnboundedSender<Option<PathBuf>>,
    tokio: Option<&Handle>,
);

/// The dialog a file is chosen in. A test puts in one that opens
/// nothing.
#[derive(Resource, Clone, Copy)]
pub struct FileDialog(pub Open);

/// A dialog that opens nothing, for an app with no screen to show
/// one, whose file is chosen by whoever drives it.
pub fn unseen(
    _: String,
    _: UnboundedSender<Option<PathBuf>>,
    _: Option<&Handle>,
) {
}

/// The system's own dialog. With no runtime to wait on, nothing is
/// chosen.
pub fn system(
    title: String,
    chosen: UnboundedSender<Option<PathBuf>>,
    tokio: Option<&Handle>,
) {
    let Some(tokio) = tokio else {
        let _ = chosen.send(None);
        return;
    };
    let picked =
        rfd::AsyncFileDialog::new().set_title(title).pick_file();
    tokio.spawn(async move {
        let file = picked.await.map(|file| file.path().to_path_buf());
        let _ = chosen.send(file);
    });
}
