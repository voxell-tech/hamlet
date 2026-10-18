//! A runtime's process: started with its id, its store and the link
//! on its standard input and output, one JSON message per line.

use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc::UnboundedSender;

use crate::RuntimeInstance;

/// A line from a runtime, or `None` once its output closed.
pub(crate) struct Received {
    pub runtime: usize,
    pub generation: u64,
    pub line: Option<String>,
}

/// How a runtime process is started.
pub(crate) struct Launch<'a> {
    pub node: &'a Path,
    pub entry: &'a Path,
    pub id: &'a RuntimeInstance,
    pub store: &'a Path,
    /// The store may be made now.
    pub fresh: bool,
    pub faux: Option<&'a Path>,
}

/// A started runtime process.
pub(crate) struct Process {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Process {
    /// Starts `launch`; its lines arrive on `sender` tagged with
    /// `runtime` and `generation`.
    pub fn start(
        launch: &Launch<'_>,
        runtime: usize,
        generation: u64,
        sender: UnboundedSender<Received>,
    ) -> Result<Self> {
        let mut command = Command::new(launch.node);
        command
            .arg("--disable-warning=ExperimentalWarning")
            .arg(launch.entry)
            .arg("--runtime")
            .arg(launch.id.as_str())
            .arg("--store")
            .arg(launch.store)
            .args(["--link", "stdio"]);
        if launch.fresh {
            command.arg("--fresh");
        }
        if let Some(faux) = launch.faux {
            command.arg("--faux").arg(faux);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                format!(
                    "starting the runtime {} with {}",
                    launch.id.as_str(),
                    launch.node.display()
                )
            })?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().context("runtime output")?;
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let received = Received {
                    runtime,
                    generation,
                    line: Some(line),
                };
                if sender.send(received).is_err() {
                    return;
                }
            }
            let _ = sender.send(Received {
                runtime,
                generation,
                line: None,
            });
        });
        Ok(Self { child, stdin })
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }

    /// Writes one line; a runtime that is gone is noticed when its
    /// output closes.
    pub async fn write(&mut self, line: &str) {
        let Some(stdin) = self.stdin.as_mut() else {
            return;
        };
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(b'\n');
        if stdin.write_all(&bytes).await.is_err() {
            self.stdin = None;
        }
    }

    /// Closes the link, so the runtime exits, and waits for it; a
    /// runtime still there after `grace` is killed.
    pub async fn stop(
        mut self,
        grace: std::time::Duration,
    ) -> String {
        self.stdin = None;
        let status = match tokio::time::timeout(
            grace,
            self.child.wait(),
        )
        .await
        {
            Ok(status) => status,
            Err(_) => {
                let _ = self.child.kill().await;
                self.child.wait().await
            }
        };
        match status {
            Ok(status) => status.to_string(),
            Err(error) => error.to_string(),
        }
    }
}
