//! The `loading` file: the ids of the libraries not yet trusted, one
//! to a line. It is written before a library is opened and a line
//! goes when the library has run clean. One found at the next start
//! means the process died while those libraries were not trusted.

use std::io;
use std::path::{Path, PathBuf};

const FILE: &str = "loading";

fn path(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// Adds the plugin `id` to the marker.
pub fn begin(root: &Path, id: &str) -> io::Result<()> {
    let mut ids = pending(root);
    if !ids.iter().any(|known| known == id) {
        ids.push(id.into());
    }
    std::fs::write(path(root), ids.join("\n"))
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
        let _ = std::fs::write(path(root), ids.join("\n"));
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
        std::fs::remove_dir_all(&root).unwrap();
    }
}
