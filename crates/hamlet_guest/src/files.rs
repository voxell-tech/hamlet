//! The plugin's own folder of files, kept by the host. A path is
//! relative, of names of letters, digits, `.`, `_`, `-` and spaces
//! joined by `/`, none starting with `.`. Reading needs the
//! capability `files.read` and the rest `files.write`; without it a
//! call is an error, and the host caps the files, their bytes and the
//! length of a path. A file the chief picks with [`pick`] is copied
//! in.

use crate::bindings::exports::hamlet::plugin::plugin::{
    Incoming, Intent, PickRequest,
};
use crate::bindings::hamlet::plugin::host;

/// The intent that asks the chief for a file in the system's dialog,
/// titled with the plugin's name and `title`, numbered `id`. The file
/// the chief picks is copied into this folder under `picked/`, and
/// the answer reaches a later `step` as `Incoming::Picked`, which
/// [`picked`] reads. It needs the capability `fs.pick`, and reading
/// the copy `files.read`. To be returned from `step`.
pub fn pick(id: u32, title: &str) -> Intent {
    Intent::PickFile(PickRequest {
        id,
        title: title.into(),
    })
}

/// The number and the outcome of the pick that `incoming` answers:
/// the path of the copy in this folder, or why there is none. `None`
/// for anything else.
pub fn picked(
    incoming: &Incoming,
) -> Option<(u32, Result<&str, &str>)> {
    let Incoming::Picked(answer) = incoming else {
        return None;
    };
    let outcome = answer
        .outcome
        .as_ref()
        .map(String::as_str)
        .map_err(String::as_str);
    Some((answer.id, outcome))
}

/// The paths of the files, in order.
pub fn list() -> Result<Vec<String>, String> {
    host::files_list()
}

/// The bytes of the file at `path`.
pub fn read(path: &str) -> Result<Vec<u8>, String> {
    host::files_read(path)
}

/// Sets the file at `path`, creating the folders on the way.
pub fn write(path: &str, data: &[u8]) -> Result<(), String> {
    host::files_write(path, data)
}

/// Removes the file at `path`, if it is there.
pub fn delete(path: &str) -> Result<(), String> {
    host::files_delete(path)
}
