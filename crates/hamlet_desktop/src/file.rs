//! The system's dialog one file, or one folder, is chosen in.

use std::path::PathBuf;

use bevy::ecs::resource::Resource;
use tokio::runtime::Handle;
use tokio::sync::mpsc::UnboundedSender;

/// What a dialog chooses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    File,
    Folder,
}

/// Opens a dialog titled `title` for one file or one folder and
/// sends the path chosen, or none when it is closed with nothing, on
/// `chosen`. It is begun on the main thread and waits on `tokio`.
pub type Open = fn(
    title: String,
    pick: Pick,
    chosen: UnboundedSender<Option<PathBuf>>,
    tokio: Option<&Handle>,
);

/// The dialog a file or a folder is chosen in. A test puts in one
/// that opens nothing.
#[derive(Resource, Clone, Copy)]
pub struct FileDialog(pub Open);

/// A dialog that opens nothing, for an app with no screen to show
/// one, whose path is chosen by whoever drives it.
pub fn unseen(
    _: String,
    _: Pick,
    _: UnboundedSender<Option<PathBuf>>,
    _: Option<&Handle>,
) {
}

/// The system's own dialog. With no runtime to wait on, nothing is
/// chosen.
pub fn system(
    title: String,
    pick: Pick,
    chosen: UnboundedSender<Option<PathBuf>>,
    tokio: Option<&Handle>,
) {
    let Some(tokio) = tokio else {
        let _ = chosen.send(None);
        return;
    };
    let dialog = rfd::AsyncFileDialog::new().set_title(title);
    match pick {
        Pick::File => {
            let picked = dialog.pick_file();
            tokio.spawn(async move {
                let path =
                    picked.await.map(|f| f.path().to_path_buf());
                let _ = chosen.send(path);
            });
        }
        Pick::Folder => {
            let picked = dialog.pick_folder();
            tokio.spawn(async move {
                let path =
                    picked.await.map(|f| f.path().to_path_buf());
                let _ = chosen.send(path);
            });
        }
    }
}
