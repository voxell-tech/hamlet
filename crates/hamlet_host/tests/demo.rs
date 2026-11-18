use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use hamlet_actor::action::encode_positional;
use hamlet_messages::address::{self, Party};
use hamlet_messages::bytes::Base64;
use hamlet_testing::{Scratch, names_in, strip_overrides};
use tell::{TellAny, Told};

#[path = "../examples/demo/tell.rs"]
mod tell;

/// How long a test waits for a line of the demo.
const PATIENCE: Duration = Duration::from_secs(30);

fn demo() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.ancestors()
        .map(|folder| {
            folder
                .join("examples")
                .join(format!("demo{}", std::env::consts::EXE_SUFFIX))
        })
        .find(|path| path.exists())
        .unwrap_or_else(|| {
            panic!(
                "no demo above {}: build it with `cargo build -p \
                 hamlet_host --example demo`",
                exe.display()
            )
        })
}

fn command(home: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(demo());
    strip_overrides(&mut command)
        .args(args)
        .env("HAMLET_HOME", home);
    command
}

/// Runs `demo <args>` on `home` to its end.
fn once(home: &Path, args: &[&str]) -> Output {
    command(home, args).stdin(Stdio::null()).output().unwrap()
}

/// A `demo <options> serve` on a realm folder.
struct Serve {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
}

impl Serve {
    fn start(home: &Path, options: &[&str]) -> Self {
        let mut args = options.to_vec();
        args.push("serve");
        let mut child = command(home, &args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        Self {
            stdin: child.stdin.take(),
            child,
            lines,
        }
    }

    /// Writes `line`, and returns the next line the demo prints.
    fn ask(&mut self, line: &str) -> String {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        self.lines.recv_timeout(PATIENCE).unwrap_or_else(|error| {
            panic!("no answer to `{line}`: {error}")
        })
    }

    /// Ends stdin, and returns whether the demo exited cleanly.
    fn stop(mut self) -> bool {
        self.stdin = None;
        self.child.wait().unwrap().success()
    }
}

impl Drop for Serve {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn two_demo_hosts_with_kinds_of_their_own_share_a_folder_and_a_third_of_one_name_is_refused()
 {
    let home = Scratch::new("demo-two-hosts");
    let mut main =
        Serve::start(&home, &["--name", "main", "--kind", "alpha"]);
    assert_eq!(main.ask("increment a1"), "alpha/a1 1");
    let mut node =
        Serve::start(&home, &["--name", "node", "--kind", "beta"]);
    assert_eq!(node.ask("increment b1"), "beta/b1 1");
    assert!(
        main.ask("get beta/b1")
            .starts_with("error: actor.not_registered")
    );

    let third = once(
        &home,
        &["--name", "main", "--kind", "gamma", "get", "g1"],
    );
    assert!(!third.status.success());
    let stderr = String::from_utf8_lossy(&third.stderr);
    assert!(stderr.contains("main.lock"), "{stderr}");

    assert_eq!(main.ask("get a1"), "alpha/a1 1");
    assert_eq!(node.ask("get b1"), "beta/b1 1");
    assert!(main.stop());
    assert!(node.stop());
    assert_eq!(names_in(&home.join("actors")), ["alpha", "beta"]);
}

#[test]
fn a_demo_host_with_no_kind_runs_the_counter_and_every_named_kind() {
    let home = Scratch::new("demo-default-kinds");
    let got = once(&home, &["get", "a"]);
    assert!(got.status.success());
    assert_eq!(String::from_utf8_lossy(&got.stdout), "counter/a 0\n");
    assert_eq!(
        names_in(&home.join("actors")),
        [
            "agent",
            "counter",
            "library",
            "profile",
            "realm",
            "workspace"
        ]
    );
}

#[test]
fn a_profile_or_library_command_runs_both_beside_the_kinds_it_names()
{
    let home = Scratch::new("demo-profile-kinds");
    let got = once(&home, &["--kind", "x", "profile"]);
    assert!(!got.status.success());
    let stderr = String::from_utf8_lossy(&got.stderr);
    assert!(stderr.contains("usage: demo profile"), "{stderr}");
    assert_eq!(
        names_in(&home.join("actors")),
        ["agent", "library", "profile", "realm", "x"]
    );
}

/// A `tell`'s arguments and its answer cross as base64 text, a third
/// over their size, and not as a list of numbers, which takes two
/// bytes for each byte of `0xff`.
#[test]
fn a_tells_bytes_cross_as_text() {
    const MIB: usize = 1 << 20;
    let bytes = Base64(vec![0xff; MIB]);
    let tell = TellAny {
        to: address::Realm.address().unwrap(),
        name: "note".to_owned(),
        args: bytes.clone(),
    };
    let told = Told {
        answer: bytes,
        took: Duration::ZERO,
    };
    for encoded in [
        encode_positional(&tell).unwrap(),
        encode_positional(&told).unwrap(),
    ] {
        assert!(
            encoded.len() < MIB * 4 / 3 + 64,
            "{}",
            encoded.len()
        );
    }
}
