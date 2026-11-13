//! `cargo xtask idle`: how much memory a command's process tree holds
//! and how often its threads wake while it should be idle.
//!
//! It starts the command with its standard input held open, waits
//! out a settle time, and samples every thread of every process in
//! the tree twice, a window apart. Memory is the sum of each
//! process's resident set at the window's end, printed in megabytes
//! of 1,000,000 bytes. Wakeups are the growth
//! of `voluntary_ctxt_switches` summed over the threads, per second
//! of the window; a thread born inside the window counts in full, and
//! one that ended inside it is lost. It reads `/proc`, so it runs on
//! Linux alone.

use std::collections::BTreeMap;
#[cfg(target_os = "linux")]
use std::collections::HashMap;
use std::time::Duration;

/// The probe's reading over one window.
#[derive(Debug)]
pub struct Reading {
    pub rss_kib: u64,
    pub wakeups_per_second: f64,
    pub processes: usize,
    /// The tree's threads at the window's end, by name.
    pub threads: BTreeMap<String, usize>,
}

impl std::fmt::Display for Reading {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        let threads: usize = self.threads.values().sum();
        write!(
            f,
            "rss {:.1} MB, {:.2} wakeups/s, {} processes, {threads} \
             threads (",
            self.rss_kib as f64 * 1024.0 / 1e6,
            self.wakeups_per_second,
            self.processes,
        )?;
        let mut names = self.threads.iter().peekable();
        while let Some((name, count)) = names.next() {
            write!(f, "{name} {count}")?;
            if names.peek().is_some() {
                write!(f, ", ")?;
            }
        }
        write!(f, ")")
    }
}

/// The probe's command line: `[--settle <s>] [--window <s>] <command>
/// [args]`.
pub struct Options {
    pub settle: Duration,
    pub window: Duration,
    pub command: Vec<String>,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self {
            settle: Duration::from_secs(30),
            window: Duration::from_secs(60),
            command: Vec::new(),
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let slot = match arg.as_str() {
                "--settle" => &mut options.settle,
                "--window" => &mut options.window,
                "--" => {
                    options.command.extend(args.cloned());
                    break;
                }
                _ => {
                    options.command.push(arg.clone());
                    options.command.extend(args.cloned());
                    break;
                }
            };
            let seconds = args
                .next()
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|seconds| *seconds >= 0.0)
                .ok_or_else(|| format!("{arg} takes seconds"))?;
            *slot = Duration::from_secs_f64(seconds);
        }
        if options.command.is_empty() {
            return Err("idle takes a command to run".to_owned());
        }
        Ok(options)
    }
}

/// Runs the command, reads it, and stops its tree.
#[cfg(target_os = "linux")]
pub fn measure(options: &Options) -> Result<Reading, String> {
    let (program, args) =
        options.command.split_first().ok_or("no command")?;
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("{program}: {error}"))?;
    let reading =
        read_tree(&mut child, options.settle, options.window);
    stop(&mut child);
    reading
}

#[cfg(not(target_os = "linux"))]
pub fn measure(_options: &Options) -> Result<Reading, String> {
    Err("idle reads /proc, which only Linux has".to_owned())
}

#[cfg(target_os = "linux")]
fn read_tree(
    child: &mut std::process::Child,
    settle: Duration,
    window: Duration,
) -> Result<Reading, String> {
    let root = child.id();
    let alive =
        |child: &mut std::process::Child| match child.try_wait() {
            Ok(None) => Ok(()),
            _ => Err("the command exited before the window ended"
                .to_owned()),
        };
    std::thread::sleep(settle);
    alive(child)?;
    let before = Sample::take(root);
    std::thread::sleep(window);
    alive(child)?;
    let after = Sample::take(root);
    let woke: u64 = after
        .threads
        .iter()
        .map(|(id, thread)| {
            let earlier = before
                .threads
                .get(id)
                .map_or(0, |thread| thread.switches);
            thread.switches.saturating_sub(earlier)
        })
        .sum();
    let mut threads = BTreeMap::new();
    for thread in after.threads.values() {
        *threads.entry(thread.name.clone()).or_insert(0) += 1;
    }
    Ok(Reading {
        rss_kib: after.rss_kib,
        wakeups_per_second: woke as f64 / window.as_secs_f64(),
        processes: after.processes,
        threads,
    })
}

/// Ends the command's standard input, asks the tree to stop, and
/// kills what is left after five seconds.
#[cfg(target_os = "linux")]
fn stop(child: &mut std::process::Child) {
    child.stdin = None;
    let pids = tree(child.id());
    for pid in &pids {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
    for _ in 0..50 {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    for pid in tree(child.id()).into_iter().chain(pids) {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(target_os = "linux")]
struct Thread {
    name: String,
    switches: u64,
}

#[cfg(target_os = "linux")]
struct Sample {
    threads: HashMap<(u32, u32), Thread>,
    rss_kib: u64,
    processes: usize,
}

#[cfg(target_os = "linux")]
impl Sample {
    fn take(root: u32) -> Self {
        let mut sample = Self {
            threads: HashMap::new(),
            rss_kib: 0,
            processes: 0,
        };
        for pid in tree(root) {
            let Ok(status) = std::fs::read_to_string(format!(
                "/proc/{pid}/status"
            )) else {
                continue;
            };
            sample.processes += 1;
            sample.rss_kib += field(&status, "VmRSS:").unwrap_or(0);
            let Ok(tasks) =
                std::fs::read_dir(format!("/proc/{pid}/task"))
            else {
                continue;
            };
            for task in tasks.flatten() {
                let Some(tid) = task
                    .file_name()
                    .to_str()
                    .and_then(|tid| tid.parse::<u32>().ok())
                else {
                    continue;
                };
                let Ok(status) = std::fs::read_to_string(
                    task.path().join("status"),
                ) else {
                    continue;
                };
                let name = status
                    .lines()
                    .find_map(|line| line.strip_prefix("Name:"))
                    .map_or_else(String::new, |name| {
                        name.trim().to_owned()
                    });
                let switches =
                    field(&status, "voluntary_ctxt_switches:")
                        .unwrap_or(0);
                sample
                    .threads
                    .insert((pid, tid), Thread { name, switches });
            }
        }
        sample
    }
}

/// The number after `key` in a `/proc` status file.
#[cfg(target_os = "linux")]
fn field(status: &str, key: &str) -> Option<u64> {
    status.lines().find_map(|line| {
        line.strip_prefix(key)?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

/// `root` and every process below it.
#[cfg(target_os = "linux")]
fn tree(root: u32) -> Vec<u32> {
    let mut parents: HashMap<u32, Vec<u32>> = HashMap::new();
    for entry in
        std::fs::read_dir("/proc").into_iter().flatten().flatten()
    {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|pid| pid.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(stat) =
            std::fs::read_to_string(entry.path().join("stat"))
        else {
            continue;
        };
        // The name sits in parentheses and may hold spaces, so the
        // fields are counted from the last `)`.
        let parent = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(1))
            .and_then(|ppid| ppid.parse::<u32>().ok());
        if let Some(parent) = parent {
            parents.entry(parent).or_default().push(pid);
        }
    }
    let mut found = vec![root];
    let mut next = 0;
    while let Some(&pid) = found.get(next) {
        found
            .extend(parents.get(&pid).into_iter().flatten().copied());
        next += 1;
    }
    found
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// The control the next test starts in a child: a thread that
    /// sleeps 0.1 seconds in a loop.
    #[test]
    #[ignore = "never ends; the idle test runs it in a child"]
    fn control() {
        loop {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    #[test]
    fn reads_a_thread_that_sleeps_a_tenth_of_a_second_as_ten_wakeups()
    {
        let exe = std::env::current_exe().unwrap();
        let options = Options {
            settle: Duration::from_millis(500),
            window: Duration::from_secs(3),
            command: vec![
                exe.to_string_lossy().into_owned(),
                "--exact".to_owned(),
                "idle::tests::control".to_owned(),
                "--ignored".to_owned(),
            ],
        };
        let reading = measure(&options).unwrap();
        assert!(
            (8.0..=13.0).contains(&reading.wakeups_per_second),
            "{reading}"
        );
        assert!(reading.rss_kib > 0, "{reading}");
    }

    /// Memory is in megabytes of a million bytes, the unit the
    /// limits it is held to are written in.
    #[test]
    fn prints_memory_in_megabytes() {
        let reading = Reading {
            rss_kib: 10_000,
            wakeups_per_second: 0.0,
            processes: 1,
            threads: BTreeMap::new(),
        };
        assert!(
            reading.to_string().starts_with("rss 10.2 MB,"),
            "{reading}"
        );
    }

    #[test]
    fn parses_the_durations_and_the_command() {
        let args: Vec<String> = [
            "--settle", "1", "--window", "2.5", "demo", "serve", "30",
        ]
        .map(str::to_owned)
        .into();
        let options = Options::parse(&args).unwrap();
        assert_eq!(options.settle, Duration::from_secs(1));
        assert_eq!(options.window, Duration::from_millis(2500));
        assert_eq!(options.command, ["demo", "serve", "30"]);
        assert!(Options::parse(&[]).is_err());
        assert!(Options::parse(&["--settle".to_owned()]).is_err());
    }
}
