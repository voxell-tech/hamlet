//! A command on a pseudo-terminal: a session of its own whose input,
//! output and size are the terminal's, killed whole.

use std::collections::BTreeMap;
use std::io;
use std::pin::Pin;
use std::process::ExitStatus;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::sys;

/// The size of a terminal, in cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub rows: u16,
    pub cols: u16,
}

/// A command to start on a terminal.
pub struct Command<'a> {
    /// The program and its arguments. [`open`] refuses an empty one.
    pub argv: &'a [String],
    /// Its whole environment.
    pub env: &'a BTreeMap<String, String>,
    pub cwd: &'a str,
}

/// The processes of a command on a terminal. Dropped, it hangs up on
/// them as a closed terminal does, then kills its group.
pub struct Session(Option<sys::Group>);

/// What is typed into the terminal.
pub struct Input(sys::PtyInput);

/// What the terminal prints.
pub struct Output(sys::PtyOutput);

/// Starts `command` on a new terminal of `size`, as the leader of a
/// session and process group of its own.
pub fn open(
    command: Command<'_>,
    size: Size,
) -> io::Result<(Session, Input, Output)> {
    if command.argv.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a command needs a program",
        ));
    }
    let (group, input, output) = sys::spawn_pty(command, size)?;
    Ok((Session(Some(group)), Input(input), Output(output)))
}

impl Session {
    /// Kills every process of the session now.
    pub fn kill(&mut self) {
        if let Some(group) = &mut self.0 {
            group.kill();
        }
    }

    /// Waits for the command to end, and kills what it left running.
    pub async fn ended(&mut self) -> io::Result<ExitStatus> {
        let Some(group) = &mut self.0 else {
            unreachable!("a session is emptied only as it drops")
        };
        let status = group.exited().await?;
        group.kill();
        Ok(status)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(group) = self.0.take() {
            group.hang_up();
        }
    }
}

impl Input {
    pub fn resize(&self, size: Size) -> io::Result<()> {
        sys::resize_pty(&self.0, size)
    }
}

impl AsyncWrite for Input {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

impl AsyncRead for Output {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    fn bash(size: Size) -> (Session, Input, Output) {
        let argv = ["bash".to_owned(), "--norc".into(), "-i".into()];
        let env = BTreeMap::from([
            (
                "PATH".to_owned(),
                std::env::var("PATH").unwrap_or_default(),
            ),
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("PS1".to_owned(), "$ ".to_owned()),
        ]);
        open(
            Command {
                argv: &argv,
                env: &env,
                cwd: "/",
            },
            size,
        )
        .unwrap()
    }

    /// What `output` prints until it holds `wanted`.
    async fn until(output: &mut Output, wanted: &str) -> String {
        let mut seen = Vec::new();
        let mut buffer = [0; 4096];
        let read = async {
            while !String::from_utf8_lossy(&seen).contains(wanted) {
                let n = output.read(&mut buffer).await.unwrap();
                assert_ne!(
                    n,
                    0,
                    "{}",
                    String::from_utf8_lossy(&seen)
                );
                seen.extend_from_slice(&buffer[..n]);
            }
        };
        tokio::time::timeout(Duration::from_secs(10), read)
            .await
            .unwrap_or_else(|_| {
                panic!("{}", String::from_utf8_lossy(&seen))
            });
        String::from_utf8_lossy(&seen).into_owned()
    }

    #[tokio::test]
    async fn a_shell_sees_a_terminal_of_its_size_and_follows_a_resize()
     {
        let (mut session, mut input, mut output) = bash(Size {
            rows: 30,
            cols: 100,
        });
        input
            .write_all(b"[ -t 0 ] && echo tty; stty size\n")
            .await
            .unwrap();
        let seen = until(&mut output, "30 100").await;
        assert!(seen.contains("tty"), "{seen}");
        input.resize(Size { rows: 12, cols: 40 }).unwrap();
        input.write_all(b"stty size\n").await.unwrap();
        until(&mut output, "12 40").await;
        input.write_all(b"exit 7\n").await.unwrap();
        // The shell's last output must be read before it can end.
        let mut rest = Vec::new();
        let drained = output.read_to_end(&mut rest);
        let _ =
            tokio::time::timeout(Duration::from_secs(10), drained)
                .await
                .expect("the shell's output ends");
        let status = tokio::time::timeout(
            Duration::from_secs(10),
            session.ended(),
        )
        .await
        .expect("the shell ends")
        .unwrap();
        assert_eq!(status.code(), Some(7));
    }

    #[tokio::test]
    async fn dropping_a_session_ends_what_it_started() {
        let (session, mut input, mut output) =
            bash(Size { rows: 24, cols: 80 });
        input
            .write_all(b"sleep 300 & echo pid=$(jobs -p).\n")
            .await
            .unwrap();
        let mut seen = Vec::new();
        let mut buffer = [0; 4096];
        let pid_of = |seen: &[u8]| {
            String::from_utf8_lossy(seen).split("pid=").find_map(
                |rest| rest.split('.').next()?.parse::<u32>().ok(),
            )
        };
        let read = async {
            while pid_of(&seen).is_none() {
                let n = output.read(&mut buffer).await.unwrap();
                assert_ne!(n, 0);
                seen.extend_from_slice(&buffer[..n]);
            }
        };
        tokio::time::timeout(Duration::from_secs(10), read)
            .await
            .unwrap_or_else(|_| {
                panic!("{:?}", String::from_utf8_lossy(&seen))
            });
        let pid = pid_of(&seen).expect("a pid");
        // As a terminal closes: its processes and both its ends.
        drop((session, input, output));
        for _ in 0..250 {
            if !hamlet_testing::signals::is_alive(pid) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("{pid} still runs");
    }
}
