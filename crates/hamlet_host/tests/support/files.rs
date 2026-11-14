//! What is left of an actor on disk.

use std::path::Path;

/// Whether any of the three files of the actor `<kind>/<key>` is in
/// the realm folder `dir`: its file, its write-ahead file and its
/// shared-memory file.
pub fn any_file_of(dir: &Path, kind: &str, key: &str) -> bool {
    ["", "-wal", "-shm"].iter().any(|suffix| {
        dir.join("actors")
            .join(kind)
            .join(format!("{key}.sqlite{suffix}"))
            .exists()
    })
}
