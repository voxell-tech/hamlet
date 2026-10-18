//! The plugin's own folder of files, kept by the host. A path is
//! relative, of names of letters, digits, `.`, `_`, `-` and spaces
//! joined by `/`, none starting with `.`. Reading needs the
//! capability `files.read` and the rest `files.write`; without it a
//! call is an error, and the host caps the files, their bytes and the
//! length of a path.

use crate::bindings::hamlet::plugin::host;

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
