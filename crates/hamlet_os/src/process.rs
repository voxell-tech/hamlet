//! The processes of one command: started together, limited, and
//! killed together, or bound to the process that starts them.

mod sys;

use std::collections::BTreeMap;
use std::io::{self, PipeReader, PipeWriter, Read};
use std::process::{ExitStatus, Stdio};

use tokio::process::{ChildStderr, ChildStdout};

/// The resources one command may use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Seconds of CPU time.
    pub cpu_secs: u64,
    /// Seconds of wall time.
    pub wall_secs: u64,
    /// Processes of the user at once.
    pub max_procs: u64,
    /// The largest file written, in MiB.
    pub max_file_mb: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            cpu_secs: 60,
            wall_secs: 120,
            max_procs: 4096,
            max_file_mb: 512,
        }
    }
}

/// A command to start.
pub struct Command<'a> {
    /// The program and its arguments. [`Tree`] refuses an empty one.
    pub argv: &'a [String],
    /// Its whole environment.
    pub env: &'a BTreeMap<String, String>,
    pub cwd: &'a str,
}

/// This process's ends of the pipes to a child started with
/// [`Tree::spawn_bound`].
pub struct Pipes {
    /// What the child reads as [`Ends::input`].
    pub input: PipeWriter,
    /// What the child writes as [`Ends::output`].
    pub output: PipeReader,
    /// The child's standard output and standard error.
    pub errors: PipeReader,
}

/// A bound child's ends of the pipes to the parent that started it.
pub struct Ends {
    pub input: PipeReader,
    pub output: PipeWriter,
}

/// The processes of a running command. Dropped before it is
/// released, it kills them.
pub struct Tree(Option<sys::Group>);

/// The processes a command left running when it ended.
pub struct Leftovers(sys::Group);

impl Tree {
    /// Starts `command` as a tree of its own, reading `stdin`, with
    /// both outputs piped.
    pub fn spawn(
        command: Command<'_>,
        stdin: Stdio,
        limits: &Limits,
    ) -> io::Result<(Self, ChildStdout, ChildStderr)> {
        has_program(&command)?;
        let (group, stdout, stderr) =
            sys::spawn(command, stdin, limits)?;
        Ok((Self(Some(group)), stdout, stderr))
    }

    /// Starts `command` as a tree of its own, with no limits, bound
    /// to this process: its process is killed when this process ends,
    /// however it ends, from its call to [`Ends::take`] on, and on
    /// Linux and Windows from its start. Its standard output writes
    /// to [`Pipes::errors`] with its standard error, and its standard
    /// input holds only what [`Ends::take`] reads.
    pub fn spawn_bound(
        command: Command<'_>,
    ) -> io::Result<(Self, Pipes)> {
        has_program(&command)?;
        let (preamble, told) = io::pipe()?;
        let (child_input, input) = io::pipe()?;
        let (output, child_output) = io::pipe()?;
        let (errors, child_errors) = io::pipe()?;
        let mut spawn = command.program();
        // On Windows the child's C runtime fills its descriptors 0 to
        // 2 from these once, at its start.
        spawn
            .stdin(preamble)
            .stdout(child_errors.try_clone()?)
            .stderr(child_errors);
        let group = sys::spawn_bound(
            spawn,
            told,
            Ends {
                input: child_input,
                output: child_output,
            },
        )?;
        Ok((
            Self(Some(group)),
            Pipes {
                input,
                output,
                errors,
            },
        ))
    }

    /// Kills every process of the tree now.
    pub fn kill(&mut self) {
        if let Some(group) = &mut self.0 {
            group.kill();
        }
    }

    /// Waits for the command to end.
    pub async fn ended(&mut self) -> io::Result<ExitStatus> {
        match &mut self.0 {
            Some(group) => group.exited().await,
            None => unreachable!("a tree is released only by value"),
        }
    }

    /// The command ended: what it started keeps running, as its
    /// leftovers, if anything does.
    pub fn release(mut self) -> Option<Leftovers> {
        self.0.take().filter(sys::Group::is_running).map(Leftovers)
    }
}

fn has_program(command: &Command<'_>) -> io::Result<()> {
    if command.argv.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a command needs a program",
        ));
    }
    Ok(())
}

impl Command<'_> {
    fn program(&self) -> tokio::process::Command {
        let mut spawn = tokio::process::Command::new(&self.argv[0]);
        spawn
            .args(&self.argv[1..])
            .env_clear()
            .envs(self.env)
            .current_dir(self.cwd)
            .kill_on_drop(true);
        spawn
    }
}

impl Ends {
    /// Takes this process's ends of the pipes to the parent that
    /// started it with [`Tree::spawn_bound`]. From then on this
    /// process is killed when that parent ends, however it ends, and
    /// at once when it has ended already, and the processes it starts
    /// do not have the ends. Fails at once when this process was not
    /// started bound, or has taken its ends already.
    pub fn take() -> io::Result<Self> {
        let preamble = sys::standard_input()
            .ok()
            .filter(|(_, waiting)| *waiting == Preamble::LEN as u64)
            .and_then(|(mut input, _)| {
                let mut bytes = [0; Preamble::LEN];
                input.read_exact(&mut bytes).ok()?;
                Preamble::decode(&bytes)
            })
            .ok_or_else(no_ends)?;
        sys::take_ends(&preamble)
    }
}

fn no_ends() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        "no ends to take: this process was not started bound, or \
         took them already",
    )
}

struct Preamble {
    input: u64,
    output: u64,
    key: u64,
}

impl Preamble {
    const TAG: [u8; 8] = *b"ends/v01";
    const LEN: usize = 32;

    fn encode(&self) -> [u8; Self::LEN] {
        let mut bytes = [0; Self::LEN];
        bytes[..8].copy_from_slice(&Self::TAG);
        bytes[8..16].copy_from_slice(&self.input.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.output.to_le_bytes());
        bytes[24..].copy_from_slice(&self.key.to_le_bytes());
        bytes
    }

    fn decode(bytes: &[u8; Self::LEN]) -> Option<Self> {
        let value = |at: usize| {
            bytes[at..at + 8].try_into().map(u64::from_le_bytes).ok()
        };
        (bytes[..8] == Self::TAG).then_some(Self {
            input: value(8)?,
            output: value(16)?,
            key: value(24)?,
        })
    }
}

impl Leftovers {
    /// Whether any of them still runs.
    pub fn is_running(&self) -> bool {
        self.0.is_running()
    }

    /// Kills every process left.
    pub fn end(self) {
        self.0.end();
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        self.kill();
    }
}

/// `argv`, run so that `limits` hold for it and for every process it
/// starts, none of which dumps core.
pub fn limited(limits: &Limits, argv: &[String]) -> Vec<String> {
    sys::limited(limits, argv)
}

/// The names of the `limits` this OS cannot hold for a command.
pub fn unenforced(limits: &Limits) -> &'static [&'static str] {
    sys::unenforced(limits)
}

/// Makes every process this one starts end when it exits, even when
/// it crashes: on Windows, the leftovers it has not ended yet.
pub fn reap_with_server() -> io::Result<()> {
    sys::reap_with_server()
}

/// Whether `path` is a file this process may run.
pub fn is_executable(path: &str) -> bool {
    sys::is_executable(path)
}

/// Kills this process at once.
pub fn end_now() -> ! {
    sys::end_now()
}

#[cfg(test)]
mod tests;
