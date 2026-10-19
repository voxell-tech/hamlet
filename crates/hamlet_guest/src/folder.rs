//! A folder the chief chooses for the plugin, outside its own: the
//! plugin asks with [`pick`], and lists, reads and writes in it by
//! paths relative to it. It needs the capability `fs.folder`, chosen
//! for reading and for writing apart. Each call is an intent to
//! return from `step`, numbered by the plugin; its answer reaches a
//! later `step`, read with [`picked`] and [`answer`].

pub use crate::bindings::exports::hamlet::plugin::plugin::FolderResult;
use crate::bindings::exports::hamlet::plugin::plugin::{
    FolderFile, FolderOp, FolderRequest, Incoming, Intent,
    PickRequest,
};

/// The intent that asks the chief for a folder in the system's
/// dialog, titled with the plugin's name and `title`, numbered `id`.
/// The folder chosen replaces the one the plugin had.
pub fn pick(id: u32, title: &str) -> Intent {
    Intent::PickFolder(PickRequest {
        id,
        title: title.into(),
    })
}

/// The number and the outcome of the pick that `incoming` answers:
/// the name of the folder, or why there is none. `None` for anything
/// else.
pub fn picked(
    incoming: &Incoming,
) -> Option<(u32, Result<&str, &str>)> {
    let Incoming::FolderPicked(answer) = incoming else {
        return None;
    };
    let outcome = answer
        .outcome
        .as_ref()
        .map(String::as_str)
        .map_err(String::as_str);
    Some((answer.id, outcome))
}

/// The intent that lists the folder at `path`, `""` for the folder
/// itself.
pub fn list(id: u32, path: &str) -> Intent {
    folder(id, FolderOp::List(path.into()))
}

/// The intent that reads the file at `path`.
pub fn read(id: u32, path: &str) -> Intent {
    folder(id, FolderOp::Read(path.into()))
}

/// The intent that sets the file at `path`, in a folder that is
/// there. It waits for the chief unless the folder is in the folder
/// of a hamlet set to auto.
pub fn write(id: u32, path: &str, data: &[u8]) -> Intent {
    folder(
        id,
        FolderOp::Write(FolderFile {
            path: path.into(),
            data: data.to_vec(),
        }),
    )
}

fn folder(id: u32, op: FolderOp) -> Intent {
    Intent::Folder(FolderRequest { id, op })
}

/// The number and the outcome of the call in the folder that
/// `incoming` answers. `None` for anything else.
pub fn answer(
    incoming: &Incoming,
) -> Option<(u32, Result<&FolderResult, &str>)> {
    let Incoming::FolderDone(answer) = incoming else {
        return None;
    };
    Some((answer.id, answer.outcome.as_ref().map_err(String::as_str)))
}
