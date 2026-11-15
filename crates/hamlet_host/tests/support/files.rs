//! What is left of an actor on disk.

use std::path::{Path, PathBuf};

/// The three files of the actor `<kind>/<key>` in the realm folder
/// `dir`: its file, its write-ahead file and its shared-memory file.
fn files_of(dir: &Path, kind: &str, key: &str) -> [PathBuf; 3] {
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

/// Whether a file descriptor of this process names any of the three
/// files of the actor `<kind>/<key>` in the canonical realm folder
/// `dir`, removed or not.
#[cfg(target_os = "linux")]
#[expect(
    clippy::allow_attributes,
    reason = "an expect would go unfulfilled in the test binaries \
              that use it"
)]
#[allow(
    dead_code,
    reason = "a support module shared by test binaries that do not \
              all use it"
)]
pub fn any_file_of_open(dir: &Path, kind: &str, key: &str) -> bool {
    let files = files_of(dir, kind, key);
    std::fs::read_dir("/proc/self/fd")
        .unwrap()
        // Another thread may close a descriptor once it is listed.
        .filter_map(|entry| {
            std::fs::read_link(entry.ok()?.path()).ok()
        })
        .any(|target| {
            let target = target.to_string_lossy();
            let target =
                target.strip_suffix(" (deleted)").unwrap_or(&target);
            files.iter().any(|file| file.as_os_str() == target)
        })
}
