use std::path::Path;

use crate::files::files_of;

/// Whether a file descriptor of this process names any of the three
/// files of the actor `<kind>/<key>` in the canonical realm folder
/// `dir`, removed or not.
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
