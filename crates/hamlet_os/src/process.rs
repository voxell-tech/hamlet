//! The processes of one command: started together, limited, and
//! killed together.

mod sys;

use std::collections::BTreeMap;
use std::io;
use std::process::Stdio;

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
    /// The program and its arguments. [`Tree::spawn`] refuses an
    /// empty one.
    pub argv: &'a [String],
    /// Its whole environment.
    pub env: &'a BTreeMap<String, String>,
    pub cwd: &'a str,
    pub stdin: Stdio,
}

/// The processes of a running command. Dropped before it is
/// released, it kills them.
pub struct Tree(sys::Group);

/// The processes a command left running when it ended.
pub struct Leftovers(
    #[expect(
        dead_code,
        reason = "a command's leftovers keep running"
    )]
    sys::Group,
);

impl Tree {
    /// Starts `command` as a tree of its own, with both outputs
    /// piped.
    pub fn spawn(
        command: Command<'_>,
        limits: &Limits,
    ) -> io::Result<(Self, tokio::process::Child)> {
        if command.argv.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a command needs a program",
            ));
        }
        let (group, child) = sys::spawn(command, limits)?;
        Ok((Self(group), child))
    }

    /// Kills every process of the tree now.
    pub fn kill(&mut self) {
        self.0.kill();
    }

    /// The command ended: what it started keeps running.
    pub fn release(mut self) -> Leftovers {
        Leftovers(std::mem::take(&mut self.0))
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

/// Whether `path` is a file this process may run.
pub fn is_executable(path: &str) -> bool {
    sys::is_executable(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn limited_wraps_the_command_in_ulimit() {
        let limits = Limits {
            cpu_secs: 7,
            max_procs: 300,
            max_file_mb: 2,
            ..Limits::default()
        };
        let argv = limited(
            &limits,
            &[
                "bash".into(),
                "-c".into(),
                "ulimit -t -u -f -c".into(),
            ],
        );
        let ran = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .unwrap();
        let shown = String::from_utf8_lossy(&ran.stdout);
        let values = shown
            .lines()
            .map(|line| line.rsplit(' ').next().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(values, ["7", "300", "2048", "0"], "{shown}");
    }
}
