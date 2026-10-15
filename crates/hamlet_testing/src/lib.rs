//! Scratch directories and protocol values for tests.

pub mod realm;

use std::ops::Deref;
use std::path::{Path, PathBuf};

/// A directory under the system's temp directory, removed on drop.
pub struct Scratch(PathBuf);

impl Scratch {
    /// A new, empty directory. `tag` must differ between tests.
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir()
            .join(format!("hamlet-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // The real path: on macOS the temp directory is a symlink.
        Scratch(dir.canonicalize().unwrap())
    }
}

impl Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
