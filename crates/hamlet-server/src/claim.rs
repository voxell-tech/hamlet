//! The claim on a home that keeps every other server out of it (S2).

use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_home::instance::socket_name;
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{
    ListenerNonblockingMode, ListenerOptions, Stream,
};

/// The interval between accepting probes and checking for release.
const POLL: Duration = Duration::from_millis(50);

/// This process's claim on a home, held until dropped.
pub(crate) struct Claim {
    _holders: Vec<Holder>,
}

/// The error of claiming a home another process holds.
#[derive(Debug)]
pub struct Held(pub PathBuf);

impl std::fmt::Display for Held {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        write!(f, "A server already runs for {}.", self.0.display())
    }
}

impl std::error::Error for Held {}

impl Claim {
    /// The claims on the given spelling and the resolved home's
    /// canonical path.
    pub fn take(dir: &Path) -> Result<Self> {
        let root = std::path::absolute(dir)?;
        let canonical =
            resolved_path(&root)?.canonicalize().with_context(
                || format!("resolving {}", root.display()),
            )?;
        let mut holders = vec![Holder::take(&root)?];
        // Path equality ignores a trailing slash, but the socket hash
        // does not.
        if canonical.as_os_str() != root.as_os_str() {
            holders.push(Holder::take(&canonical)?);
        }
        Ok(Self { _holders: holders })
    }
}

/// Node's path.resolve spelling, with parent components removed
/// before following symlinks.
pub(crate) fn resolved_path(dir: &Path) -> Result<PathBuf> {
    let mut resolved = PathBuf::new();
    for component in std::path::absolute(dir)?.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other),
        }
    }
    Ok(resolved)
}

/// The listener on one spelling's socket, served by a thread of its
/// own.
struct Holder {
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Holder {
    fn take(root: &Path) -> Result<Self> {
        let name = socket_name(root)?;
        let held = || anyhow::Error::new(Held(root.to_owned()));
        if Stream::connect(name.clone()).is_ok() {
            return Err(held());
        }
        // Nobody answered, so a socket file left on disk is a dead
        // holder's.
        let listener = ListenerOptions::new()
            .name(name)
            .nonblocking(ListenerNonblockingMode::Accept)
            .try_overwrite(true)
            .create_sync()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::AddrInUse => held(),
                _ => anyhow::Error::new(error)
                    .context(format!("claiming {}", root.display())),
            })?;
        let (stop, stopped) = mpsc::channel();
        let thread = thread::spawn(move || {
            while let Err(RecvTimeoutError::Timeout) =
                stopped.recv_timeout(POLL)
            {
                // Another claimant only checks that someone answers.
                while let Ok(stream) = listener.accept() {
                    drop(stream);
                }
            }
        });
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
