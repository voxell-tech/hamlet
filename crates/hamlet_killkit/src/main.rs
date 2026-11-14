//! The kill kit. For each run it starts the demo host of
//! `hamlet_host` on a fresh realm folder, kills it with SIGKILL at
//! the scenario's fault point or at a random time, restarts it twice,
//! and prints one line with the state before the kill and after each
//! restart. It exits non-zero when any run fails its scenario.
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

use crate::scenario::{Is, Kill, Probe, Read, SCENARIOS, Scenario};

/// How long the kit waits for a reply, a fault point or a restart.
const PATIENCE: Duration = Duration::from_secs(30);

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
        for scenario in SCENARIOS {
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
    let scenario = SCENARIOS
        .iter()
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
    restarts: Vec<Restart>,
}

/// One restart's values, by name: each actor's last reply, the
/// count of each actor its boot started, as `boot <actor>`, and each
/// probe.
type Restart = BTreeMap<String, i64>;

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
        }
        failures
    }

    fn describe(&self) -> String {
        let mut text = format!(
            "killed {}, acked {}",
            self.killed,
            values(&self.acked)
        );
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
        Kill::After { .. } => None,
    };
    let mut host =
        Host::start(demo, &home, fault, &dir.join("host.log"))?;
    let mut acked = BTreeMap::new();
    for line in scenario.setup {
        host.send(line)?;
        let (actor, count) = host.reply()?;
        acked.insert(actor, count);
    }
    host.send(scenario.trigger)?;
    let sent = Instant::now();
    if scenario.trigger_replies {
        let (actor, count) = host.reply()?;
        acked.insert(actor, count);
    }
    let killed = match scenario.kill {
        Kill::Fault(_) => host.kill_at_fault(&faults)?,
        Kill::After { min_ms, max_ms } => {
            host.kill_after(Duration::from_millis(
                rand::thread_rng().gen_range(min_ms..=max_ms),
            ))?
        }
    };

    std::thread::sleep(
        (sent + Duration::from_millis(scenario.down_ms))
            .saturating_duration_since(Instant::now()),
    );
    let mut restarts = Vec::new();
    for (n, lines) in (1..).zip(scenario.restart) {
        let mut host = Host::start(
            demo,
            &home,
            None,
            &dir.join(format!("restart-{n}.log")),
        )?;
        for line in lines {
            host.send(line)?;
        }
        let mut restart = Restart::new();
        for line in host.finish()? {
            let (name, count) = match line.strip_prefix("boot ") {
                Some(reply) => {
                    let (actor, count) = parse_reply(reply)?;
                    (format!("boot {actor}"), count)
                }
                None => parse_reply(&line)?,
            };
            restart.insert(name, count);
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

/// A demo host the kit drives through `demo serve`.
struct Host {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
}

impl Host {
    /// Starts `demo serve` on `home`, stopping at `fault` when given,
    /// with its standard error in `log`.
    fn start(
        demo: &Path,
        home: &Path,
        fault: Option<(&str, &Path)>,
        log: &Path,
    ) -> Result<Self> {
        let mut command = Command::new(demo);
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
}
