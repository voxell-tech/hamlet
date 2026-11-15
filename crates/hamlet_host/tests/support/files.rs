//! What is left of an actor on disk.

use std::path::{Path, PathBuf};

/// The three files of the actor `<kind>/<key>` in the realm folder
/// `dir`: its file, its write-ahead file and its shared-memory file.
pub fn files_of(dir: &Path, kind: &str, key: &str) -> [PathBuf; 3] {
    ["", "-wal", "-shm"].map(|suffix| {
        dir.join("actors")
            .join(kind)
            .join(format!("{key}.sqlite{suffix}"))
    })
}

/// Whether any of the three files of the actor `<kind>/<key>` is in
/// the realm folder `dir`.
pub fn any_file_of(dir: &Path, kind: &str, key: &str) -> bool {
    files_of(dir, kind, key).iter().any(|file| file.exists())
}
