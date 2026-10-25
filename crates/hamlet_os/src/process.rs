//! The processes of one command: started together, limited, and
//! killed together.

mod sys;

use std::collections::BTreeMap;
use std::io;
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
pub struct Tree(Option<sys::Group>);

/// The processes a command left running when it ended.
pub struct Leftovers(sys::Group);

impl Tree {
    /// Starts `command` as a tree of its own, with both outputs
    /// piped.
    pub fn spawn(
        command: Command<'_>,
        limits: &Limits,
    ) -> io::Result<(Self, ChildStdout, ChildStderr)> {
        if command.argv.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a command needs a program",
            ));
        }
        let (group, stdout, stderr) = sys::spawn(command, limits)?;
        Ok((Self(Some(group)), stdout, stderr))
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

#[cfg(all(test, unix))]
mod tests {
    use std::time::Duration;

    use hamlet_testing::signals::is_alive;
    use rustix::process::{Pid, Signal, kill_process};
    use tokio::io::{AsyncBufReadExt, BufReader};

    use super::*;

    fn bash(script: &str) -> (Tree, ChildStdout) {
        let argv = ["bash".to_owned(), "-c".into(), script.into()];
        let env = BTreeMap::from([(
            "PATH".to_owned(),
            std::env::var("PATH").unwrap_or_default(),
        )]);
        let (tree, stdout, _) = Tree::spawn(
            Command {
                argv: &argv,
                env: &env,
                cwd: "/",
                stdin: Stdio::null(),
            },
            &Limits::default(),
        )
        .unwrap();
        (tree, stdout)
    }

    async fn is_gone(pids: &[u32]) -> bool {
        for _ in 0..500 {
            if !pids.iter().any(|&pid| is_alive(pid)) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    #[tokio::test]
    async fn ending_leftovers_ends_their_whole_group() {
        let (mut tree, stdout) = bash(
            "sleep 300 & echo $!; bash -c 'sleep 300 & echo $!; \
             wait' &",
        );
        let mut lines = BufReader::new(stdout).lines();
        let mut pids = Vec::new();
        while pids.len() < 2 {
            let line = lines.next_line().await.unwrap().unwrap();
            pids.push(line.parse::<u32>().unwrap());
        }
        tree.ended().await.unwrap();
        let leftovers = tree.release().expect("the sleeps run on");
        assert!(pids.iter().all(|&pid| is_alive(pid)), "{pids:?}");
        leftovers.end();
        assert!(is_gone(&pids).await, "{pids:?}");
    }

    #[tokio::test]
    async fn leftovers_hold_their_group_s_id_until_they_end() {
        let (mut tree, stdout) = bash("echo $$; sleep 300 & echo $!");
        let mut lines = BufReader::new(stdout).lines();
        let mut ids = [0u32; 2];
        for id in &mut ids {
            *id = lines
                .next_line()
                .await
                .unwrap()
                .unwrap()
                .parse()
                .unwrap();
        }
        let [leader, member] = ids;
        tree.ended().await.unwrap();
        let leftovers = tree.release().expect("the sleep runs on");
        let _ = kill_process(
            Pid::from_raw(member as i32).unwrap(),
            Signal::KILL,
        );
        assert!(is_gone(&[member]).await);
        assert!(!leftovers.is_running());
        assert!(is_alive(leader), "the group's id is free for reuse");
        leftovers.end();
        assert!(is_gone(&[leader]).await);
    }

    #[tokio::test]
    async fn a_command_that_starts_nothing_leaves_no_leftovers() {
        let (mut tree, stdout) = bash("echo $$; exit 3");
        let mut lines = BufReader::new(stdout).lines();
        let leader = lines
            .next_line()
            .await
            .unwrap()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(tree.ended().await.unwrap().code(), Some(3));
        assert!(
            is_alive(leader),
            "the leader is reaped only on release"
        );
        assert!(tree.release().is_none());
        assert!(is_gone(&[leader]).await);
    }

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

#[cfg(all(test, windows))]
mod tests {
    use std::time::Duration;

    use hamlet_testing::signals::is_alive;
    use tokio::io::{AsyncBufReadExt, BufReader, Lines};

    use super::*;

    /// A `ping` that waits five minutes, started apart from its
    /// parent, and the line with its id.
    const SLEEPER: &str = "(Start-Process ping -ArgumentList \
                           '-n','300','127.0.0.1' -NoNewWindow \
                           -PassThru).Id";

    fn powershell(
        script: &str,
    ) -> (Tree, Lines<BufReader<ChildStdout>>) {
        let program = format!(
            r"{}\System32\WindowsPowerShell\v1.0\powershell.exe",
            std::env::var("SystemRoot").unwrap()
        );
        let argv = [
            program,
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            script.into(),
        ];
        let env = std::env::vars().collect();
        let (tree, stdout, _) = Tree::spawn(
            Command {
                argv: &argv,
                env: &env,
                cwd: &std::env::temp_dir().to_string_lossy(),
                stdin: Stdio::null(),
            },
            &Limits::default(),
        )
        .unwrap();
        (tree, BufReader::new(stdout).lines())
    }

    /// The first number printed, past what `ping` prints.
    async fn id(lines: &mut Lines<BufReader<ChildStdout>>) -> u32 {
        loop {
            let line = lines.next_line().await.unwrap().unwrap();
            if let Ok(id) = line.trim().parse() {
                return id;
            }
        }
    }

    async fn is_gone(pid: u32) -> bool {
        for _ in 0..100 {
            if !is_alive(pid) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    #[tokio::test]
    async fn a_timeout_ends_the_whole_job() {
        let (mut tree, mut lines) =
            powershell(&format!("{SLEEPER}; Start-Sleep 300"));
        let pid = id(&mut lines).await;
        tree.kill();
        tree.ended().await.unwrap();
        assert!(is_gone(pid).await, "{pid}");
    }

    #[tokio::test]
    async fn ending_leftovers_ends_their_job() {
        let (mut tree, mut lines) = powershell(SLEEPER);
        let pid = id(&mut lines).await;
        tree.ended().await.unwrap();
        let leftovers = tree.release().expect("the ping runs on");
        assert!(is_alive(pid), "{pid}");
        leftovers.end();
        assert!(is_gone(pid).await, "{pid}");
    }

    #[tokio::test]
    async fn a_command_that_starts_nothing_leaves_no_leftovers() {
        let (mut tree, _) = powershell("exit 3");
        assert_eq!(tree.ended().await.unwrap().code(), Some(3));
        // The console host started beside it can end a moment later.
        if let Some(leftovers) = tree.release() {
            let mut waited = 0;
            while leftovers.is_running() && waited < 50 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                waited += 1;
            }
            assert!(!leftovers.is_running());
        }
    }

    /// Set in the copy of this test that plays the server.
    const SERVER: &str = "HAMLET_TEST_REAPER_SERVER";

    #[tokio::test]
    async fn released_leftovers_end_with_the_reaper() {
        if std::env::var_os(SERVER).is_some() {
            reap_with_server().unwrap();
            let (mut tree, mut lines) = powershell(SLEEPER);
            let pid = id(&mut lines).await;
            tree.ended().await.unwrap();
            let leftovers = tree.release().expect("the ping runs on");
            #[expect(
                clippy::print_stdout,
                reason = "the parent test reads the id from it"
            )]
            {
                println!("leftover {pid}");
            }
            // Kept, not ended: only the server's exit ends it.
            std::mem::forget(leftovers);
            return;
        }
        let server = std::process::Command::new(
            std::env::current_exe().unwrap(),
        )
        .args([
            "--exact",
            "process::tests::released_leftovers_end_with_the_reaper",
            "--nocapture",
        ])
        .env(SERVER, "1")
        .output()
        .unwrap();
        let shown = String::from_utf8_lossy(&server.stdout);
        let pid = shown
            .lines()
            .find_map(|line| line.strip_prefix("leftover "))
            .and_then(|pid| pid.trim().parse().ok())
            .unwrap_or_else(|| panic!("{shown}"));
        assert!(is_gone(pid).await, "{pid}");
    }
}
