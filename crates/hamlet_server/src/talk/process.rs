//! A runtime's process: started with its id, its store and the
//! runtime protocol on its standard input and output, one JSON
//! message per line.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, ensure};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc::{self, UnboundedSender};

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
    /// The pi harness the environment gives: `HAMLET_NODE` and
    /// `HAMLET_PI_HARNESS`, else the Node a release ships beside the
    /// server, or Node on the path, and the entry beside the server
    /// or in the checkout it was built from.
    pub fn from_env(faux: Option<PathBuf>) -> Result<Self> {
        let set =
            |name| std::env::var(name).ok().filter(|v| !v.is_empty());
        let executable = std::env::current_exe()?;
        let directory = executable
            .parent()
            .context("Cannot locate the server's directory")?;
        let entry = if let Some(path) = set("HAMLET_PI_HARNESS") {
            let path = PathBuf::from(path);
            ensure!(
                path.is_file(),
                "HAMLET_PI_HARNESS names {}, which is not a file.",
                path.display()
            );
            path
        } else {
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
        let shipped = directory
            .join(format!("node{}", std::env::consts::EXE_SUFFIX));
        let node = match set("HAMLET_NODE") {
            Some(node) => node.into(),
            None if shipped.is_file() => shipped,
            None => "node".into(),
        };
        Ok(Self { node, entry, faux })
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
    pub id: &'a str,
    pub store: &'a Path,
    /// The store may be made now.
    pub fresh: bool,
}

/// A started runtime process.
pub(crate) struct Process {
    child: Child,
    /// The lines for its standard input, which a task of its own
    /// writes in order; none once closed.
    lines: Option<UnboundedSender<String>>,
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
        #[expect(
            clippy::disallowed_methods,
            reason = "pi is the server's own runtime, not an \
                      agent's command"
        )]
        let mut command = Command::new(&pi.node);
        command
            .env_remove("HAMLET_API_KEY")
            .arg("--disable-warning=ExperimentalWarning")
            .arg(&pi.entry)
            .arg("--runtime")
            .arg(launch.id)
            .arg("--store")
            .arg(launch.store)
            .args(["--transport", "stdio"]);
        if launch.fresh {
            command.arg("--fresh");
        }
        if let Some(faux) = &pi.faux {
            command.arg("--faux").arg(faux);
        }
        // CREATE_NO_WINDOW: a server `hamlet` starts in the
        // background has no console, and Windows would open one for
        // pi.
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                format!(
                    "starting the runtime {} with {}",
                    launch.id,
                    pi.node.display()
                )
            })?;
        let mut stdin =
            child.stdin.take().context("runtime input")?;
        let stdout = child.stdout.take().context("runtime output")?;
        let (lines, mut writing) =
            mpsc::unbounded_channel::<String>();
        // A runtime that is gone is noticed when its output closes.
        tokio::spawn(async move {
            while let Some(mut line) = writing.recv().await {
                line.push('\n');
                if stdin.write_all(line.as_bytes()).await.is_err() {
                    return;
                }
            }
        });
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
        Ok(Self {
            child,
            lines: Some(lines),
        })
    }

    /// Queues one line, which is written after the lines before it.
    pub fn write(&mut self, line: String) {
        if let Some(lines) = &self.lines
            && lines.send(line).is_err()
        {
            self.lines = None;
        }
    }

    /// Closes the runtime's input, so it exits, and waits for it; a
    /// runtime still there after `grace` is killed.
    pub async fn stop(
        mut self,
        grace: std::time::Duration,
    ) -> String {
        // Its input closes once the lines queued are written.
        self.lines = None;
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
