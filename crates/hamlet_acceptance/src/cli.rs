//! The `hamlet` command run against a served home, with what it
//! printed kept apart by stream.

use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;

use crate::client::PATIENCE;
use crate::server::{OVERRIDES, cli};

/// Which stream a piece of output came on.
#[derive(Clone, Copy)]
enum Stream {
    Out,
    Err,
}

/// A running `hamlet` command, killed on drop.
pub struct Cli {
    child: Child,
    stdin: Option<ChildStdin>,
    printed: mpsc::UnboundedReceiver<(Stream, Vec<u8>)>,
    out: Vec<u8>,
    err: Vec<u8>,
}

impl Cli {
    /// Runs `hamlet args` on `home` with `env` added, its stdin open.
    pub fn start(
        home: &Path,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Self {
        let mut command = Command::new(cli());
        for name in OVERRIDES {
            command.env_remove(name);
        }
        let mut child = command
            .args(args)
            .env("HAMLET_HOME", home)
            .env("HAMLET_USER", "chief")
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap_or_else(|error| {
                panic!("cannot run {}: {error}", cli().display())
            });
        let (sender, printed) = mpsc::unbounded_channel();
        let out = child.stdout.take().unwrap();
        let err = child.stderr.take().unwrap();
        tokio::spawn(forward(out, Stream::Out, sender.clone()));
        tokio::spawn(forward(err, Stream::Err, sender));
        Self {
            stdin: child.stdin.take(),
            child,
            printed,
            out: vec![],
            err: vec![],
        }
    }

    /// Runs `hamlet args` on `home` with `input` on stdin, to its
    /// end.
    pub async fn run(
        home: &Path,
        args: &[&str],
        input: &str,
        env: &[(&str, &str)],
    ) -> Ended {
        let mut cli = Self::start(home, args, env);
        cli.write(input).await;
        cli.end().await
    }

    /// Writes `text` to stdin.
    pub async fn write(&mut self, text: &str) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        stdin.write_all(text.as_bytes()).await.unwrap();
        stdin.flush().await.unwrap();
    }

    /// Waits until stdout shows `text`.
    pub async fn out_shows(&mut self, text: &str) {
        self.until(text, |cli| cli.out().contains(text)).await;
    }

    /// Waits until stderr shows `text`.
    pub async fn err_shows(&mut self, text: &str) {
        self.until(text, |cli| cli.err().contains(text)).await;
    }

    /// Waits until `done` holds of what was printed.
    pub async fn until(
        &mut self,
        what: &str,
        done: impl Fn(&Self) -> bool,
    ) {
        let began = Instant::now();
        while !done(self) {
            assert!(
                began.elapsed() < PATIENCE,
                "waited {PATIENCE:?} for \
                 {what}\nstdout:\n{}\nstderr:\n{}",
                self.out(),
                self.err()
            );
            let tick = Duration::from_millis(100);
            if let Ok(Some(piece)) =
                tokio::time::timeout(tick, self.printed.recv()).await
            {
                self.keep(piece);
            }
        }
    }

    /// Closes stdin and waits for the command to end.
    pub async fn end(mut self) -> Ended {
        self.stdin = None;
        self.wait().await
    }

    /// Waits for the command to end, stdin left as it is.
    pub async fn wait(mut self) -> Ended {
        let status =
            tokio::time::timeout(PATIENCE, self.child.wait())
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "the command ran past \
                         {PATIENCE:?}\nstdout:\n{}\nstderr:\n{}",
                        self.out(),
                        self.err()
                    )
                })
                .unwrap();
        while let Some(piece) = self.printed.recv().await {
            self.keep(piece);
        }
        Ended {
            status,
            out: self.out(),
            err: self.err(),
        }
    }

    /// What stdout printed so far.
    pub fn out(&self) -> String {
        String::from_utf8_lossy(&self.out).into_owned()
    }

    /// What stderr printed so far.
    pub fn err(&self) -> String {
        String::from_utf8_lossy(&self.err).into_owned()
    }

    fn keep(&mut self, (stream, bytes): (Stream, Vec<u8>)) {
        match stream {
            Stream::Out => self.out.extend(bytes),
            Stream::Err => self.err.extend(bytes),
        }
    }
}

/// A command that ended, and all it printed.
#[derive(Debug)]
pub struct Ended {
    pub status: ExitStatus,
    pub out: String,
    pub err: String,
}

impl Ended {
    pub fn code(&self) -> Option<i32> {
        self.status.code()
    }
}

async fn forward(
    mut from: impl AsyncRead + Unpin,
    stream: Stream,
    to: mpsc::UnboundedSender<(Stream, Vec<u8>)>,
) {
    let mut buffer = vec![0; 8192];
    while let Ok(n) = from.read(&mut buffer).await {
        if n == 0 || to.send((stream, buffer[..n].to_vec())).is_err()
        {
            break;
        }
    }
}
