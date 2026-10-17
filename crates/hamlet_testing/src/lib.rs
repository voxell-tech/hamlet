//! A directory for a test to work in.

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Git for scratch fixtures, isolated from user configuration and
/// hooks.
pub fn git(root: &Path) -> Command {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    command
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LC_ALL", "C")
        .args(["-c", "commit.gpgsign=false", "-c"])
        .arg(format!("core.hooksPath={null}"))
        .args(["-c", "init.templateDir="]);
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
