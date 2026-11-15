//! The kill kit. For each run it starts the demo host of
//! `hamlet_host` on a fresh realm folder, alone or with its Node
//! host, kills it with SIGKILL at the scenario's fault point or at a
//! random time, or has the Node host kill itself at a step of an
//! action, restarts twice, and prints one line with the state before
//! the kill and after each restart. It exits non-zero when any run
//! fails its scenario.
//!
//! ```text
//! hamlet_killkit <scenario> [--runs <n>] [--demo <path>] [--keep]
//! hamlet_killkit --list
//! ```
//!
//! `--demo` defaults to `examples/demo` beside the kit's own binary,
//! where `cargo build -p hamlet_host --example demo` puts it. Each
//! kit process makes a new folder of its own under
//! `hamlet-killkit` in the system's temporary folder, with one
//! folder per run inside it. A passed run's folder is removed unless
//! `--keep` is given; a failed run's folder is always kept.

mod scenario;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail, ensure};
use rand::Rng;
use rusqlite::{Connection, OpenFlags};

use crate::scenario::{Demo, Is, Kill, Probe, Read, Scenario};

/// How long the kit waits for a reply, a fault point or a restart.
const PATIENCE: Duration = Duration::from_secs(30);

/// How long a Node host may outlive its demo host.
const GONE_WAIT: Duration = Duration::from_secs(2);

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "killkit: {error:#}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    scenario: &'static Scenario,
    runs: u32,
    demo: PathBuf,
    keep: bool,
}

/// Runs the scenario the arguments name. Returns whether every run
/// passed.
fn run() -> Result<bool> {
    let mut out = std::io::stdout();
    let Some(options) = options(std::env::args().skip(1))? else {
        for scenario in scenario::all() {
            writeln!(out, "{:<22}{}", scenario.name, scenario.about)?;
        }
        return Ok(true);
    };
    ensure!(
        options.demo.is_file(),
        "no demo host at {}; build it with `cargo build -p \
         hamlet_host --example demo`",
        options.demo.display()
    );
    let parent = std::env::temp_dir().join("hamlet-killkit");
    private_dir(&parent)?;
    let base = fresh_dir(
        &parent,
        &format!("{}-{}", options.scenario.name, std::process::id()),
    )?;
    let mut passed = 0;
    for n in 1..=options.runs {
        let dir = base.join(format!("run-{n}"));
        let line =
            match run_once(options.scenario, &options.demo, &dir) {
                Ok(report) => {
                    let failures = report.failures(options.scenario);
                    let verdict = if failures.is_empty() {
                        passed += 1;
                        "pass".to_owned()
                    } else {
                        format!("FAIL ({})", failures.join("; "))
                    };
                    if failures.is_empty() && !options.keep {
                        std::fs::remove_dir_all(&dir).with_context(
                            || format!("remove {}", dir.display()),
                        )?;
                    }
                    format!("{verdict} | {}", report.describe())
                }
                Err(error) => {
                    format!("FAIL ({error:#}) | {}", dir.display())
                }
            };
        writeln!(out, "run {n}/{}: {line}", options.runs)?;
    }
    writeln!(
        out,
        "{}: {passed} of {} runs passed",
        options.scenario.name, options.runs
    )?;
    // Kept runs stay inside the kit's folder, which then is not
    // empty and stays too.
    let _ = std::fs::remove_dir(&base);
    Ok(passed == options.runs)
}

/// The options the arguments give, or none for `--list`.
fn options(
    mut args: impl Iterator<Item = String>,
) -> Result<Option<Options>> {
    let mut name = None;
    let mut runs = 1;
    let mut demo = None;
    let mut keep = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--list" => return Ok(None),
            "--runs" => {
                let value =
                    args.next().context("--runs takes a number")?;
                runs = value.parse().with_context(|| {
                    format!("`{value}` is not a number")
                })?;
                ensure!(runs > 0, "--runs must be at least 1");
            }
            "--demo" => {
                demo = Some(PathBuf::from(
                    args.next().context("--demo takes a path")?,
                ));
            }
            "--keep" => keep = true,
            _ if name.is_none() && !arg.starts_with('-') => {
                name = Some(arg);
            }
            other => bail!("unknown argument `{other}`"),
        }
    }
    let name = name.context(
        "name a scenario, or pass --list to see them: \
         hamlet_killkit <scenario> [--runs <n>] [--demo <path>] \
         [--keep]",
    )?;
    let scenario = scenario::all()
        .find(|scenario| scenario.name == name)
        .ok_or_else(|| {
            anyhow!("no scenario `{name}`; --list lists them")
        })?;
    let demo = match demo {
        Some(demo) => demo,
        None => std::env::current_exe()?
            .parent()
            .context("the kit's binary has no folder")?
            .join("examples")
            .join(format!("demo{}", std::env::consts::EXE_SUFFIX)),
    };
    Ok(Some(Options {
        scenario,
        runs,
        demo,
        keep,
    }))
}

/// One run's record.
struct Report {
    /// When the kill came, as the fault point or the time drawn, and
    /// the pid it stopped.
    killed: String,
    /// Each actor's last acknowledged count before the kill.
    acked: BTreeMap<String, i64>,
    /// How long after the scenario's due time the first restart
    /// started, in milliseconds.
    past_due: Option<i64>,
    restarts: Vec<Restart>,
}

/// One restart's values, by name: each actor's last reply, the
/// count of each actor its boot started, as `boot <actor>`, how many
/// commands failed with each code, as `error <group.code>`, and each
/// probe.
type Restart = BTreeMap<String, i64>;

/// The prefix of the name of each count of failed commands.
const ERROR: &str = "error ";

impl Report {
    fn failures(&self, scenario: &Scenario) -> Vec<String> {
        let mut failures = Vec::new();
        for (n, (restart, expects)) in
            self.restarts.iter().zip(&scenario.expect).enumerate()
        {
            for expect in *expects {
                let want = match expect.is {
                    Is::Acked(actor, plus) => {
                        let Some(acked) = self.acked.get(actor)
                        else {
                            failures.push(format!(
                                "{actor} acknowledged nothing"
                            ));
                            continue;
                        };
                        Some(acked + plus)
                    }
                    Is::Exactly(value) => Some(value),
                    Is::Present => {
                        if !restart.contains_key(expect.value) {
                            failures.push(format!(
                                "restart {}: {} is missing",
                                n + 1,
                                expect.value
                            ));
                        }
                        continue;
                    }
                    Is::Absent => None,
                };
                let got = restart.get(expect.value).copied();
                if got != want {
                    failures.push(format!(
                        "restart {}: {} is {}, want {}",
                        n + 1,
                        expect.value,
                        shown(got),
                        shown(want)
                    ));
                }
            }
            for name in restart.keys().filter(|name| {
                name.starts_with(ERROR)
                    && expects
                        .iter()
                        .all(|expect| expect.value != *name)
            }) {
                failures.push(format!(
                    "restart {}: an unexpected {name}",
                    n + 1
                ));
            }
        }
        failures
    }

    fn describe(&self) -> String {
        let mut text = format!(
            "killed {}, acked {}",
            self.killed,
            values(&self.acked)
        );
        if let Some(past_due) = self.past_due {
            let _ =
                write!(text, ", restarted {past_due} ms past due");
        }
        for (n, restart) in self.restarts.iter().enumerate() {
            let _ = write!(
                text,
                " | restart {}: {}",
                n + 1,
                values(restart)
            );
        }
        text
    }
}

fn shown(value: Option<i64>) -> String {
    value.map_or_else(
        || "missing".to_owned(),
        |value| value.to_string(),
    )
}

fn values(values: &BTreeMap<String, i64>) -> String {
    if values.is_empty() {
        return "nothing".to_owned();
    }
    values
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runs the scenario once in `dir`: the host until the kill, then two
/// restarts, each read once it exited.
fn run_once(
    scenario: &Scenario,
    demo: &Path,
    dir: &Path,
) -> Result<Report> {
    let home = dir.join("home");
    let faults = dir.join("faults");
    private_dir(&home)?;
    private_dir(&faults)?;

    let fault = match scenario.kill {
        Kill::Fault(point) => Some((point, faults.as_path())),
        Kill::After { .. } | Kill::NodeHost => None,
    };
    let mut host = Host::start(
        demo,
        &scenario.demo,
        &home,
        fault,
        &dir.join("host.log"),
    )?;
    let mut acked = BTreeMap::new();
    for line in scenario.setup {
        host.send(line)?;
        let (actor, count) = host.reply()?;
        acked.insert(actor, count);
    }
    let node = match scenario.demo {
        Demo::Local => None,
        Demo::Linked { pid } => Some(node_pid(&acked, pid)?),
    };
    host.send(scenario.trigger)?;
    let sent = Instant::now();
    if scenario.trigger_replies {
        let (actor, count) = host.reply()?;
        acked.insert(actor, count);
    }
    let mut surviving = None;
    let killed = match (&scenario.kill, node) {
        (Kill::Fault(_), _) => host.kill_at_fault(&faults)?,
        (Kill::After { min_ms, max_ms }, _) => {
            host.kill_after(Duration::from_millis(
                rand::thread_rng().gen_range(*min_ms..=*max_ms),
            ))?
        }
        (Kill::NodeHost, Some(pid)) => {
            ensure!(
                ended_within(pid, PATIENCE)?,
                "the Node host {pid} did not end at the trigger's \
                 step"
            );
            surviving = Some(host);
            format!("the Node host at the trigger's step (pid {pid})")
        }
        (Kill::NodeHost, None) => {
            bail!("a scenario that kills the Node host is linked")
        }
    };
    if let (Some(pid), None) = (node, &surviving)
        && !ended_within(pid, GONE_WAIT)?
    {
        end(pid);
        bail!(
            "the Node host {pid} outlived its demo host by {} s",
            GONE_WAIT.as_secs()
        );
    }

    let due = scenario
        .due
        .as_ref()
        .map(|probe| {
            read_probe(&home, probe)
                .with_context(|| format!("probe {}", probe.name))
        })
        .transpose()?;
    let down = (sent + Duration::from_millis(scenario.down_ms))
        .saturating_duration_since(Instant::now());
    let to_due = due.map_or(Duration::ZERO, |due| {
        Duration::from_millis(
            (due + 1 - unix_ms()).max(0).unsigned_abs(),
        )
    });
    ensure!(
        to_due <= down + Duration::from_millis(scenario.down_ms),
        "the one-shot is due {} ms after the planned restart",
        to_due.saturating_sub(down).as_millis()
    );
    std::thread::sleep(down.max(to_due));
    let past_due = due.map(|due| unix_ms() - due);
    if let Some(past_due) = past_due {
        ensure!(
            past_due > 0,
            "the first restart started {} ms before the due time",
            -past_due
        );
    }
    let mut nodes = Vec::from_iter(node);
    let mut restarts = Vec::new();
    for (n, lines) in (1..).zip(scenario.restart) {
        let mut host = match surviving.take() {
            Some(host) => host,
            None => Host::start(
                demo,
                &scenario.demo,
                &home,
                None,
                &dir.join(format!("restart-{n}.log")),
            )?,
        };
        for line in lines {
            host.send(line)?;
        }
        let (mut printed, children) = match scenario.demo {
            Demo::Local => (Vec::new(), Vec::new()),
            Demo::Linked { pid } => {
                let printed = host.lines_through(pid)?;
                (printed, children(host.child.id())?)
            }
        };
        printed.extend(host.finish()?);
        let mut restart = Restart::new();
        for line in printed {
            if let Some(code) = error_code(&line) {
                *restart
                    .entry(format!("{ERROR}{code}"))
                    .or_insert(0) += 1;
                continue;
            }
            let (name, count) = match line.strip_prefix("boot ") {
                Some(reply) => {
                    let (actor, count) = parse_reply(reply)?;
                    (format!("boot {actor}"), count)
                }
                None => parse_reply(&line)?,
            };
            restart.insert(name, count);
        }
        if let Demo::Linked { pid } = scenario.demo {
            let pid = node_pid(&restart, pid)?;
            ensure!(
                !nodes.contains(&pid),
                "restart {n} ran on the Node host {pid} of before"
            );
            ensure!(
                children == [pid],
                "restart {n}'s demo host ran the processes \
                 {children:?}, not its one Node host {pid}"
            );
            if !ended_within(pid, GONE_WAIT)? {
                end(pid);
                bail!(
                    "the Node host {pid} of restart {n} outlived \
                     its demo host"
                );
            }
            nodes.push(pid);
        }
        for probe in scenario.probes {
            let value = read_probe(&home, probe)
                .with_context(|| format!("probe {}", probe.name))?;
            restart.insert(probe.name.to_owned(), value);
        }
        restarts.push(restart);
    }
    Ok(Report {
        killed,
        acked,
        past_due,
        restarts,
    })
}

/// Makes `path` and its missing parents, readable by this user
/// alone.
fn private_dir(path: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(path)
        .with_context(|| format!("create {}", path.display()))
}

/// Makes a new folder in `parent` whose name starts with `stem`,
/// readable by this user alone. It never returns a folder that
/// existed before, so no other kit process shares it.
fn fresh_dir(parent: &Path, stem: &str) -> Result<PathBuf> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    for n in 0.. {
        let dir = parent.join(format!("{stem}-{n}"));
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(error)
                if error.kind()
                    == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("create {}", dir.display())
                });
            }
        }
    }
    unreachable!("the folder names never run out")
}

fn read_probe(home: &Path, probe: &Probe) -> Result<i64> {
    let file = home.join(probe.file);
    match probe.read {
        Read::Sql(sql) => {
            let conn = Connection::open_with_flags(
                &file,
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .with_context(|| format!("open {}", file.display()))?;
            Ok(conn.query_row(sql, [], |row| row.get(0))?)
        }
        Read::Files => {
            let mut found = 0;
            for suffix in ["", "-wal", "-shm"] {
                let mut path = file.as_os_str().to_owned();
                path.push(suffix);
                found += i64::from(Path::new(&path).try_exists()?);
            }
            Ok(found)
        }
    }
}

/// Reads `counter/<key> <count>`.
fn parse_reply(line: &str) -> Result<(String, i64)> {
    let parsed = line.split_once(' ').and_then(|(actor, count)| {
        Some((actor.to_owned(), count.parse().ok()?))
    });
    parsed.ok_or_else(|| anyhow!("the host printed `{line}`"))
}

/// The `<group.code>` of `error: <group.code>: <words>`, the line the
/// demo prints for a command that failed with an actor's error.
fn error_code(line: &str) -> Option<&str> {
    let (code, _) = line.strip_prefix("error: ")?.split_once(": ")?;
    let (group, name) = code.split_once('.')?;
    let plain = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    };
    (plain(group) && plain(name)).then_some(code)
}

fn node_pid(
    values: &BTreeMap<String, i64>,
    name: &str,
) -> Result<u32> {
    let pid = values
        .get(name)
        .with_context(|| format!("the host printed no {name}"))?;
    u32::try_from(*pid)
        .with_context(|| format!("{name} is {pid}, not a process id"))
}

fn ended_within(pid: u32, within: Duration) -> Result<bool> {
    let deadline = Instant::now() + within;
    loop {
        if has_ended(pid)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether the process `pid` has ended: no process has the id, or
/// the one that has it is dead and waits for its parent to collect
/// it.
#[cfg(target_os = "linux")]
fn has_ended(pid: u32) -> Result<bool> {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => Ok(stat_field(&stat, 0)
            .is_some_and(|state| state.starts_with('Z'))),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(true)
        }
        Err(error) => Err(error)
            .with_context(|| format!("read the state of {pid}")),
    }
}

#[cfg(not(target_os = "linux"))]
fn has_ended(_pid: u32) -> Result<bool> {
    bail!(
        "the kit tells that a Node host has ended only on Linux, \
         from /proc"
    )
}

#[cfg(target_os = "linux")]
fn children(parent: u32) -> Result<Vec<u32>> {
    let mut children = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let Some(pid) = entry?
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        // A process may end between the listing and the read.
        let Ok(stat) =
            std::fs::read_to_string(format!("/proc/{pid}/stat"))
        else {
            continue;
        };
        if stat_field(&stat, 1) == Some(&parent.to_string()) {
            children.push(pid);
        }
    }
    Ok(children)
}

/// The field `n` of a process's `/proc/<pid>/stat` after the
/// command's name, which is in parentheses and may hold any
/// character: 0 is the state, 1 the parent's id.
#[cfg(target_os = "linux")]
fn stat_field(stat: &str, n: usize) -> Option<&str> {
    stat.rsplit_once(')')?.1.split_whitespace().nth(n)
}

#[cfg(not(target_os = "linux"))]
fn children(_parent: u32) -> Result<Vec<u32>> {
    bail!("the kit counts a demo host's children only on Linux")
}

fn unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            since.as_millis().try_into().unwrap_or(i64::MAX)
        })
}

/// Kills the Node host `pid`, which outlived its demo host, so that
/// a failed run leaves no process. The kit has no handle on it, so
/// it asks `kill`.
fn end(pid: u32) {
    #[cfg(unix)]
    let _ = Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status();
    #[cfg(not(unix))]
    let _ = pid;
}

/// A demo host the kit drives through `demo serve`.
struct Host {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
}

impl Host {
    /// Starts `demo serve` on `home` as `how` says, stopping at
    /// `fault` when given, with its standard error in `log`.
    fn start(
        demo: &Path,
        how: &Demo,
        home: &Path,
        fault: Option<(&str, &Path)>,
        log: &Path,
    ) -> Result<Self> {
        let mut command = Command::new(demo);
        if let Demo::Linked { .. } = how {
            command.arg("--link");
        }
        command
            .arg("serve")
            .env("HAMLET_HOME", home)
            .env_remove("HAMLET_FAULT")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(log)?);
        if let Some((point, dir)) = fault {
            command
                .env("HAMLET_FAULT", point)
                .env("HAMLET_FAULT_DIR", dir);
        }
        let mut child = command
            .spawn()
            .with_context(|| format!("start {}", demo.display()))?;
        let stdout = child.stdout.take().context("no stdout")?;
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        Ok(Self {
            stdin: child.stdin.take(),
            child,
            lines,
        })
    }

    fn send(&mut self, line: &str) -> Result<()> {
        let stdin = self.stdin.as_mut().context("stdin is closed")?;
        writeln!(stdin, "{line}")?;
        stdin.flush()?;
        Ok(())
    }

    /// The lines the host prints up to the one that names `value`,
    /// that one included.
    fn lines_through(&self, value: &str) -> Result<Vec<String>> {
        let mut lines = Vec::new();
        loop {
            let line =
                self.lines.recv_timeout(PATIENCE).with_context(
                    || format!("the host printed no {value}"),
                )?;
            let named = line
                .split_once(' ')
                .is_some_and(|(name, _)| name == value);
            lines.push(line);
            if named {
                return Ok(lines);
            }
        }
    }

    /// The next reply, as an actor and its count.
    fn reply(&self) -> Result<(String, i64)> {
        let line = self
            .lines
            .recv_timeout(PATIENCE)
            .context("no reply from the host")?;
        parse_reply(&line)
    }

    /// Waits for the host to stop at its fault point, then kills it
    /// with SIGKILL. Returns the point and the pid.
    fn kill_at_fault(mut self, faults: &Path) -> Result<String> {
        let marker = faults.join("host-paused");
        let deadline = Instant::now() + PATIENCE;
        while !marker.exists() {
            ensure!(
                Instant::now() < deadline,
                "the host never reached its fault point"
            );
            ensure!(
                self.child.try_wait()?.is_none(),
                "the host exited before its fault point"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let paused = std::fs::read_to_string(&marker)?;
        let (point, pid) = paused
            .trim()
            .split_once(' ')
            .context("the fault marker holds no pid")?;
        ensure!(
            pid == self.child.id().to_string(),
            "the fault marker names pid {pid}, not the host"
        );
        self.child.kill()?;
        self.child.wait()?;
        Ok(format!("at {point} (pid {pid})"))
    }

    /// Kills the host with SIGKILL once `wait` has passed. Returns
    /// the time and the pid.
    fn kill_after(mut self, wait: Duration) -> Result<String> {
        std::thread::sleep(wait);
        ensure!(
            self.child.try_wait()?.is_none(),
            "the host exited before its kill"
        );
        let pid = self.child.id();
        self.child.kill()?;
        self.child.wait()?;
        Ok(format!(
            "{} ms after the trigger (pid {pid})",
            wait.as_millis()
        ))
    }

    /// Ends stdin, so the host stops, and returns every line it
    /// printed once it exited cleanly.
    fn finish(mut self) -> Result<Vec<String>> {
        self.stdin = None;
        let deadline = Instant::now() + PATIENCE;
        let mut lines = Vec::new();
        loop {
            let left =
                deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) => lines.push(line),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    bail!("the host did not stop")
                }
            }
        }
        let status = self.child.wait()?;
        ensure!(status.success(), "the host exited with {status}");
        Ok(lines)
    }
}

impl Drop for Host {
    /// Leaves no host running when a run fails halfway.
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter()
            .map(|arg| (*arg).to_owned())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn zero_runs_is_an_error() {
        assert!(
            options(args(&["counter-transaction", "--runs", "0"]))
                .is_err()
        );
        let options =
            options(args(&["counter-transaction", "--runs", "2"]))
                .unwrap()
                .unwrap();
        assert_eq!(options.runs, 2);
    }

    #[test]
    fn a_kit_folder_is_never_one_that_exists() {
        let parent = std::env::temp_dir().join(format!(
            "hamlet-killkit-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&parent);
        private_dir(&parent).unwrap();
        std::fs::create_dir(parent.join("kit-0")).unwrap();
        std::fs::write(parent.join("kit-0").join("other"), "")
            .unwrap();

        let first = fresh_dir(&parent, "kit").unwrap();
        let second = fresh_dir(&parent, "kit").unwrap();
        assert_ne!(first, parent.join("kit-0"));
        assert_ne!(first, second);
        for dir in [&first, &second] {
            assert!(dir.is_dir());
            assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
        }
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[test]
    fn only_a_coded_error_line_is_an_error_value() {
        assert_eq!(
            error_code(
                "error: link.lost: The link to the other host went \
                 down before the answer came."
            ),
            Some("link.lost")
        );
        assert_eq!(error_code("error: unknown command `x`"), None);
        assert_eq!(error_code("error: no such file: a.b"), None);
        assert_eq!(error_code("agent/1/note 3"), None);
    }

    fn report(restart: &[(&str, i64)]) -> Report {
        let restart = restart
            .iter()
            .map(|(name, value)| ((*name).to_owned(), *value))
            .collect::<Restart>();
        Report {
            killed: String::new(),
            acked: BTreeMap::from([("agent/1/note".to_owned(), 3)]),
            past_due: None,
            restarts: vec![restart.clone(), restart],
        }
    }

    #[test]
    fn an_error_no_expectation_names_fails_the_restart() {
        let lost = scenario::all()
            .find(|scenario| scenario.name == "link-kill-node")
            .unwrap();
        let shown = [
            ("error link.lost", 1),
            ("heard/1", 1),
            ("agent/1/note", 5),
            ("agent/1/count", 5),
            ("agent.notes", 5),
            ("agent.jobs", 0),
        ];
        assert_eq!(
            report(&shown).failures(lost),
            ["restart 2: an unexpected error link.lost"]
        );
        let mut down = shown.to_vec();
        down.push(("error link.down", 1));
        assert_eq!(
            report(&down).failures(lost),
            [
                "restart 1: an unexpected error link.down",
                "restart 2: an unexpected error link.down",
                "restart 2: an unexpected error link.lost",
            ]
        );
        let counters = scenario::all()
            .find(|scenario| scenario.name == "counter-transaction")
            .unwrap();
        assert!(
            report(&[("error actor.not_found", 1)])
                .failures(counters)
                .contains(
                    &"restart 1: an unexpected error actor.not_found"
                        .to_owned()
                )
        );
    }

    #[test]
    fn a_scenario_that_kills_the_node_host_is_linked() {
        for scenario in scenario::all() {
            if let Kill::NodeHost = scenario.kill {
                assert!(
                    matches!(scenario.demo, Demo::Linked { .. }),
                    "{}",
                    scenario.name
                );
            }
        }
    }

    #[test]
    fn a_scenario_that_arms_a_one_shot_is_down_past_its_due_time() {
        for scenario in scenario::all() {
            let words: Vec<&str> =
                scenario.trigger.split(' ').collect();
            if let ["tell", _, "arm", ms] = words.as_slice() {
                let due: u64 = ms.parse().expect(scenario.name);
                assert!(scenario.down_ms > due, "{}", scenario.name);
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_processs_children_are_the_processes_it_started() {
        let started: Vec<Child> = (0..2)
            .map(|_| {
                Command::new(std::env::current_exe().unwrap())
                    .arg("--list")
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        // Other tests of this binary start children too.
        let found = children(std::process::id()).unwrap();
        for mut child in started {
            assert!(found.contains(&child.id()), "{found:?}");
            child.wait().unwrap();
        }
        assert!(children(u32::MAX).unwrap().is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_has_ended_once_it_is_dead_also_before_it_is_collected()
     {
        assert!(!has_ended(std::process::id()).unwrap());
        let mut child =
            Command::new(std::env::current_exe().unwrap())
                .arg("--list")
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
        let pid = child.id();
        assert!(ended_within(pid, PATIENCE).unwrap());
        child.wait().unwrap();
        assert!(has_ended(pid).unwrap());
    }
}
