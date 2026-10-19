//! One hamlet per home: a second launch asks the running one to show
//! its window, then exits.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::{Context, Result};
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions, Name, Stream,
};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

/// This process's standing in its home: the one hamlet of it, or a
/// later launch.
pub enum Instance {
    /// This process is the one running. A message comes for each
    /// request to show.
    Primary(UnboundedReceiver<()>),
    /// Another hamlet already runs for this home and has been asked
    /// to show itself.
    Secondary,
}

fn name(home: &Path) -> Result<Name<'static>> {
    if GenericNamespaced::is_supported() {
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

/// Claims `home` for this process, or asks the hamlet that runs there
/// to show itself.
pub fn claim(home: &Path) -> Result<Instance> {
    let name = name(home)?;
    if let Ok(mut stream) = Stream::connect(name.clone()) {
        stream.write_all(b"show\n")?;
        return Ok(Instance::Secondary);
    }
    // Nobody answered, so a socket file still on disk (Unix sockets
    // outlive their process) is stale and safe to replace.
    let listener = ListenerOptions::new()
        .name(name)
        .try_overwrite(true)
        .create_sync()
        .with_context(|| {
            format!(
                "claiming {} as the only running hamlet",
                home.display()
            )
        })?;
    let (tx, rx) = unbounded_channel();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(conn).read_line(&mut line).is_ok()
                && line.trim() == "show"
            {
                // The claim outlives a receiver nobody reads: a mode
                // with no window still has to stay the only one.
                let _ = tx.send(());
            }
        }
    });
    Ok(Instance::Primary(rx))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn second_claim_signals_the_first() {
        let home = Scratch::new("instance");
        let Instance::Primary(mut rx) = claim(&home).unwrap() else {
            panic!("first claim should be primary")
        };
        assert!(matches!(claim(&home).unwrap(), Instance::Secondary));
        let shown = (0..200).any(|_| {
            std::thread::sleep(Duration::from_millis(10));
            rx.try_recv().is_ok()
        });
        assert!(shown);
    }

    #[test]
    fn the_claim_holds_when_nobody_listens_for_show() {
        let home = Scratch::new("instance-deaf");
        let Instance::Primary(rx) = claim(&home).unwrap() else {
            panic!("first claim should be primary")
        };
        drop(rx);
        for _ in 0..3 {
            assert!(matches!(
                claim(&home).unwrap(),
                Instance::Secondary
            ));
            // Time for the listener to take the request in.
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn stale_socket_from_a_dead_run_is_replaced() {
        let home = Scratch::new("instance-stale");
        // A listener that is gone without cleaning up, like a process
        // that exited. An earlier run with this pid may have left one
        // already.
        let stale = ListenerOptions::new()
            .name(name(&home).unwrap())
            .reclaim_name(false)
            .try_overwrite(true)
            .create_sync()
            .unwrap();
        drop(stale);
        assert!(matches!(
            claim(&home).unwrap(),
            Instance::Primary(_)
        ));
    }
}
