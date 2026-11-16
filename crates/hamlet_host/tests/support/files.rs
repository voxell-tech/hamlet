//! What is left of an actor on disk.

use std::path::{Path, PathBuf};

pub fn files_of(dir: &Path, kind: &str, key: &str) -> [PathBuf; 3] {
    ["", "-wal", "-shm"].map(|suffix| {
        dir.join("actors")
            .join(kind)
            .join(format!("{key}.sqlite{suffix}"))
    })
}

pub fn any_file_of(dir: &Path, kind: &str, key: &str) -> bool {
    files_of(dir, kind, key).iter().any(|file| file.exists())
}
