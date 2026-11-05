//! The claim on a home that keeps every other server out of it (S2).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Result;
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerNonblockingMode,
    ListenerOptions, Name, Stream,
};
use sha2::{Digest, Sha256};

/// The interval between accepting probes and checking for release.
const POLL: Duration = Duration::from_millis(50);

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

fn socket_name(home: &Path) -> Result<Name<'static>> {
    if GenericNamespaced::is_supported() {
        // A hash keeps the name within the socket path limit.
        let digest =
            Sha256::digest(home.to_string_lossy().as_bytes());
        let tag = digest
            .iter()
            .take(6)
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        Ok(format!("hamlet-{tag}.sock")
            .to_ns_name::<GenericNamespaced>()?
            .into_owned())
    } else {
        Ok(home
            .join("hamlet.sock")
            .to_fs_name::<GenericFilePath>()?
            .into_owned())
    }
}

/// This process's claim on a home, held until dropped: a listener
/// on the home's socket, served by a thread of its own.
pub(crate) struct Claim {
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Claim {
    /// Claims the home at the canonical path `root`.
    pub fn take(root: &Path) -> Result<Self> {
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

impl Drop for Claim {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
