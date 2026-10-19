//! The `loading` file: written before a library is opened and removed
//! when its entry returns. One found at the next start means the
//! process died in that library.

use std::io;
use std::path::{Path, PathBuf};

const FILE: &str = "loading";

fn path(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// Writes the marker for the plugin `id`.
pub fn begin(root: &Path, id: &str) -> io::Result<()> {
    std::fs::write(path(root), id)
}

/// Removes the marker. A missing one is fine.
pub fn end(root: &Path) {
    let _ = std::fs::remove_file(path(root));
}

/// The id of the plugin that was loading when the last run ended, if
/// one was.
pub fn pending(root: &Path) -> Option<String> {
    let id = std::fs::read_to_string(path(root)).ok()?;
    Some(id.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_names_the_plugin_until_it_is_removed() {
        let root = std::env::temp_dir()
            .join(format!("hamlet_marker_{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(pending(&root), None);
        begin(&root, "spark").unwrap();
        assert_eq!(pending(&root), Some("spark".into()));
        end(&root);
        end(&root);
        assert_eq!(pending(&root), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
