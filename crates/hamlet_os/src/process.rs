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
    /// The process the ends are for: on Unix its parent, on Windows
    /// itself.
    owner: u64,
}

impl Preamble {
    const TAG: [u8; 8] = *b"ends/v01";
    const LEN: usize = 32;

    fn encode(&self) -> [u8; Self::LEN] {
        let mut bytes = [0; Self::LEN];
        bytes[..8].copy_from_slice(&Self::TAG);
        bytes[8..16].copy_from_slice(&self.input.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.output.to_le_bytes());
        bytes[24..].copy_from_slice(&self.owner.to_le_bytes());
        bytes
    }

    fn decode(bytes: &[u8; Self::LEN]) -> Option<Self> {
        let value = |at: usize| {
            bytes[at..at + 8].try_into().map(u64::from_le_bytes).ok()
        };
        (bytes[..8] == Self::TAG).then_some(Self {
            input: value(8)?,
            output: value(16)?,
            owner: value(24)?,
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
            },
            Stdio::null(),
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
            },
            Stdio::null(),
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

#[cfg(test)]
mod bound {
    use std::io::{BufRead, BufReader, Write};
    use std::time::{Duration, Instant};

    use hamlet_testing::signals::is_alive;
    use tokio::io::AsyncBufReadExt as _;

    use super::*;

    const PART: &str = "HAMLET_TEST_BOUND_PART";

    fn part() -> Option<String> {
        std::env::var(PART).ok()
    }

    fn this_test(test: &str) -> [String; 4] {
        [
            std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            "--exact".into(),
            test.into(),
            "--nocapture".into(),
        ]
    }

    fn run_part(test: &str, part: &str) -> tokio::process::Command {
        let [program, args @ ..] = this_test(test);
        let mut command = tokio::process::Command::new(program);
        command.args(args).env(PART, part).kill_on_drop(true);
        command
    }

    fn spawn_child(test: &str, part: &str) -> (Tree, Pipes) {
        let argv = this_test(test);
        let mut env: BTreeMap<_, _> = std::env::vars().collect();
        env.insert(PART.to_owned(), part.to_owned());
        Tree::spawn_bound(Command {
            argv: &argv,
            env: &env,
            cwd: &std::env::current_dir().unwrap().to_string_lossy(),
        })
        .unwrap()
    }

    fn child() -> ! {
        let Ends { input, mut output } = Ends::take().unwrap();
        writeln!(output, "{}", std::process::id()).unwrap();
        for line in BufReader::new(input).lines() {
            writeln!(output, "{}", line.unwrap()).unwrap();
        }
        loop {
            std::thread::park();
        }
    }

    #[tokio::test]
    async fn a_bound_child_that_took_its_ends_is_gone_within_two_seconds_of_its_parent()
     {
        const TEST: &str = "process::bound::a_bound_child_that_took_its_ends_is_gone_within_two_seconds_of_its_parent";
        match part().as_deref() {
            Some("child") => child(),
            Some("parent") => {
                let (_tree, pipes) = spawn_child(TEST, "child");
                let pid = BufReader::new(pipes.output)
                    .lines()
                    .next()
                    .unwrap()
                    .unwrap();
                #[expect(
                    clippy::print_stdout,
                    reason = "the test reads the child's id from it"
                )]
                {
                    println!("child {pid}");
                }
                std::future::pending::<()>().await;
            }
            _ => {}
        }
        let mut parent = run_part(TEST, "parent")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut shown =
            tokio::io::BufReader::new(parent.stdout.take().unwrap())
                .lines();
        let pid: u32 = loop {
            let line = shown
                .next_line()
                .await
                .unwrap()
                .expect("the parent printed the child's id");
            if let Some(pid) = line.strip_prefix("child ") {
                break pid.parse().unwrap();
            }
        };
        assert!(is_alive(pid));
        parent.kill().await.unwrap();
        let killed = Instant::now();
        while is_alive(pid) {
            if killed.elapsed() > Duration::from_secs(2) {
                #[cfg(unix)]
                let _ = hamlet_testing::signals::kill(pid);
                panic!("the child {pid} outlived its parent");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn a_bound_child_outlives_the_thread_that_started_it() {
        const TEST: &str =
            "process::bound::a_bound_child_outlives_the_thread_that_started_it";
        if part().is_some() {
            child();
        }
        let runtime = tokio::runtime::Handle::current();
        let (_tree, mut pipes) = std::thread::spawn(move || {
            let _runtime = runtime.enter();
            spawn_child(TEST, "child")
        })
        .join()
        .unwrap();
        let mut echoed = BufReader::new(pipes.output).lines();
        echoed.next().unwrap().unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        writeln!(pipes.input, "still here").unwrap();
        assert_eq!(echoed.next().unwrap().unwrap(), "still here");
    }

    #[tokio::test]
    async fn a_take_in_a_process_not_started_bound_fails_at_once() {
        const TEST: &str =
            "process::bound::a_take_in_a_process_not_started_bound_fails_at_once";
        if part().is_some() {
            let taken = Ends::take().map(|_| ());
            assert_eq!(
                taken.map_err(|error| error.kind()),
                Err(io::ErrorKind::NotFound)
            );
            return;
        }
        let mut unbound = run_part(TEST, "unbound")
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let ended = tokio::time::timeout(
            Duration::from_secs(10),
            unbound.wait(),
        )
        .await
        .expect("the take waited");
        assert!(ended.unwrap().success());
    }

    #[tokio::test]
    async fn a_take_in_a_process_its_bound_child_started_fails_at_once()
     {
        const TEST: &str = "process::bound::a_take_in_a_process_its_bound_child_started_fails_at_once";
        match part().as_deref() {
            Some("launcher") => {
                let [program, args @ ..] = this_test(TEST);
                let taken = std::process::Command::new(program)
                    .args(args)
                    .env(PART, "taker")
                    .status()
                    .unwrap();
                assert!(taken.success(), "{taken}");
                return;
            }
            Some(_) => {
                let taken = Ends::take().map(|_| ());
                assert_eq!(
                    taken.map_err(|error| error.kind()),
                    Err(io::ErrorKind::NotFound)
                );
                return;
            }
            None => {}
        }
        let (mut launcher, _pipes) = spawn_child(TEST, "launcher");
        let ended = tokio::time::timeout(
            Duration::from_secs(20),
            launcher.ended(),
        )
        .await
        .expect("the take waited");
        assert!(ended.unwrap().success());
    }
}
