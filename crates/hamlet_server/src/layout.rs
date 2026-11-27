//! Where everything of a realm lives in its folder.

use std::path::{Path, PathBuf};

/// The paths of a realm's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
        }
    }

    /// The running server's address, for the CLI.
    pub fn server(&self) -> PathBuf {
        self.root.join("server.json")
    }
}
