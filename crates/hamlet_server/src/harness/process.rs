//! A runtime's process: started with its id, its store and the
//! runtime protocol on its standard input and output, one JSON
//! message per line.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, ensure};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc::UnboundedSender;

use crate::RuntimeInstance;

/// How the pi harness's runtimes are started.
#[derive(Clone, Debug)]
pub struct Pi {
    pub node: PathBuf,
    /// The harness's entry, `harnesses/pi/src/main.ts`.
    pub entry: PathBuf,
    /// A faux script in place of real models.
    pub faux: Option<PathBuf>,
}

impl Pi {
    /// The provider of the faux script's models.
    pub const FAUX_PROVIDER: &str = "faux";

    /// The pi harness the environment gives: `HAMLET_NODE` and
    /// `HAMLET_PI_HARNESS`, else Node on the path and the entry
    /// beside the server or in the checkout it was built from.
    pub fn from_env(faux: Option<PathBuf>) -> Result<Self> {
        let set =
            |name| std::env::var(name).ok().filter(|v| !v.is_empty());
        let entry = if let Some(path) = set("HAMLET_PI_HARNESS") {
            let path = PathBuf::from(path);
            ensure!(
                path.is_file(),
                "HAMLET_PI_HARNESS names {}, which is not a file.",
                path.display()
            );
            path
        } else {
            let executable = std::env::current_exe()?;
            let directory = executable
                .parent()
                .context("Cannot locate the server's directory")?;
            [
                directory.join("pi/src/main.ts"),
                directory.join("../Resources/pi/src/main.ts"),
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../harnesses/pi/src/main.ts"),
            ]
            .into_iter()
            .find(|path| path.is_file())
            .with_context(|| {
                format!(
                    "No pi harness beside the server in {} or in \
                     the checkout it was built from. Set \
                     HAMLET_PI_HARNESS to pi's src/main.ts.",
                    directory.display()
                )
            })?
        };
        Ok(Self {
            node: set("HAMLET_NODE")
                .unwrap_or_else(|| "node".into())
                .into(),
            entry,
            faux,
        })
    }

    /// The store of the runtime whose folder is `folder`.
    pub fn store(folder: &Path) -> PathBuf {
        folder.join("pi.sqlite")
    }
}

/// A line from a runtime, or `None` once its output closed.
pub(crate) struct Received {
    pub runtime: usize,
    pub generation: u64,
    pub line: Option<String>,
}

/// How a runtime process is started.
pub(crate) struct Launch<'a> {
    pub pi: &'a Pi,
    pub id: &'a RuntimeInstance,
    pub store: &'a Path,
    /// The store may be made now.
    pub fresh: bool,
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
        let pi = launch.pi;
        let mut command = Command::new(&pi.node);
        command
            .env_remove("HAMLET_API_KEY")
            .arg("--disable-warning=ExperimentalWarning")
            .arg(&pi.entry)
            .arg("--runtime")
            .arg(launch.id.as_str())
            .arg("--store")
            .arg(launch.store)
            .args(["--transport", "stdio"]);
        if launch.fresh {
            command.arg("--fresh");
        }
        if let Some(faux) = &pi.faux {
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
                    pi.node.display()
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

    /// Closes the runtime's input, so it exits, and waits for it; a
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
