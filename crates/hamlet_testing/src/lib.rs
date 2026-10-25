//! Fixtures shared by the crates' tests: scratch folders, protocol
//! values, faux scripts, a loopback OpenAI endpoint and a server
//! process on a scratch home.

pub mod links;
pub mod openai;
pub mod realm;
mod script;
// It stops a server with Unix signals.
#[cfg(unix)]
mod server;

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

pub use script::{Script, Step, call, calls, failure, words};
#[cfg(unix)]
pub use server::{
    Home, Models, Server, Socket, Unstarted, answers, cli, send,
    until,
};

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

/// A folder under the system's temp folder, removed on drop.
pub struct Scratch(PathBuf);

impl Scratch {
    /// A new, empty folder named after `tag`.
    pub fn new(tag: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "hamlet-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // The real path: on macOS the temp folder is a symlink.
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

/// A FIFO at `path`.
#[cfg(unix)]
pub fn fifo(path: &Path) {
    let made = Command::new("mkfifo").arg(path).status().unwrap();
    assert!(made.success(), "mkfifo {}", path.display());
}
