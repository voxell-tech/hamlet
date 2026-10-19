//! Two files in the home about the runs that open libraries.
//!
//! `loading`: the ids of the libraries not yet trusted, one to a
//! line. It is written before a library is opened and a line goes
//! when the library has run clean. One found at the next start means
//! the process died while those libraries were not trusted.
//!
//! `unclean`: how many runs in a row opened a library and did not
//! exit by themselves. It counts up when a run opens its first
//! library and goes when a run exits by itself.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

const FILE: &str = "loading";

const UNCLEAN: &str = "unclean";

fn path(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// Writes `text` to a file beside `path`, syncs it and renames it
/// over `path`, so that a stop in the middle leaves the old file or
/// the new one.
fn write(path: &Path, text: &str) -> io::Result<()> {
    let part = path.with_extension("part");
    let mut file = std::fs::File::create(&part)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&part, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&part);
    })
}

/// Adds the plugin `id` to the marker.
pub fn begin(root: &Path, id: &str) -> io::Result<()> {
    let mut ids = pending(root);
    if !ids.iter().any(|known| known == id) {
        ids.push(id.into());
    }
    write(&path(root), &ids.join("\n"))
}

/// Takes the plugin `id` off the marker, and removes the file when
/// none is left.
pub fn settled(root: &Path, id: &str) {
    let ids = pending(root)
        .into_iter()
        .filter(|known| known != id)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        end(root);
    } else {
        let _ = write(&path(root), &ids.join("\n"));
    }
}

/// Removes the marker. A missing one is fine.
pub fn end(root: &Path) {
    let _ = std::fs::remove_file(path(root));
}

/// The ids of the plugins that were not trusted when the last run
/// ended.
pub fn pending(root: &Path) -> Vec<String> {
    std::fs::read_to_string(path(root))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(String::from)
        .collect()
}

/// The runs in a row that opened a library and did not exit by
/// themselves.
pub fn unclean(root: &Path) -> u32 {
    std::fs::read_to_string(root.join(UNCLEAN))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Counts this run as unclean until it exits by itself.
pub fn opening(root: &Path) -> io::Result<()> {
    write(&root.join(UNCLEAN), &(unclean(root) + 1).to_string())
}

/// This run exited by itself: the count starts again.
pub fn exited(root: &Path) {
    let _ = std::fs::remove_file(root.join(UNCLEAN));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_names_the_plugins_until_each_is_settled() {
        let root = std::env::temp_dir()
            .join(format!("hamlet_marker_{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(pending(&root).is_empty());
        begin(&root, "spark").unwrap();
        begin(&root, "glow").unwrap();
        begin(&root, "spark").unwrap();
        assert_eq!(pending(&root), ["spark", "glow"]);
        settled(&root, "spark");
        assert_eq!(pending(&root), ["glow"]);
        settled(&root, "glow");
        assert!(!path(&root).exists());
        end(&root);

        opening(&root).unwrap();
        opening(&root).unwrap();
        assert_eq!(unclean(&root), 2);
        exited(&root);
        assert_eq!(unclean(&root), 0);
        // Nothing is left beside the files.
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
