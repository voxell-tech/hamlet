//! Scratch directories and protocol values for tests.

pub mod realm;

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The variables a developer's shell may carry that would change what
/// a test server does. Every test process strips them before setting
/// its own.
pub const OVERRIDES: &[&str] = &[
    "HAMLET_SERVER",
    "HAMLET_TOKEN",
    "HAMLET_MODEL",
    "HAMLET_BASE_URL",
    "HAMLET_API_KEY",
    "HAMLET_NET",
    "HAMLET_REFLECT_EVERY",
    "HAMLET_WORKSPACE_URL",
    "HAMLET_WORKSPACE_TOKEN",
    "HAMLET_FAULT",
    "HAMLET_FAULT_DIR",
    "HAMLET_FAULT_SPLIT_MARKER",
    "HAMLET_HARNESS_FAULT",
    "HAMLET_UNSAFE_TOOLS",
];

/// Removes every one of [`OVERRIDES`] from `command`'s environment.
pub fn strip_overrides(command: &mut Command) -> &mut Command {
    for name in OVERRIDES {
        command.env_remove(name);
    }
    command
}

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
