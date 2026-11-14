//! The link between the Rust host and its Node child.

use std::io::{ErrorKind, pipe};
use std::path::Path;
use std::thread;

use hamlet_host::link::{
    self, Broken, FRAME_LIMIT, Frame, PROTOCOL, WireError,
};
use hamlet_host::route::{CallerKind, Routes};
use hamlet_host::{Address, Caller, HostEvent};
use serde_bytes::ByteBuf;
use serde_reflection::{Tracer, TracerConfig};
use sha2::{Digest, Sha256};

/// The largest write the workspace takes, 16 MiB.
const WRITE_CAP: usize = 16 << 20;

/// The recorded fingerprint, in the crate's folder.
const FINGERPRINT: &str = "link.fingerprint";

fn call(args: Vec<u8>) -> Frame {
    Frame::Call {
        id: 7,
        from: Some(Caller::Actor(Address::new("agent", "a1"))),
        to: Address::new("workspace", "w1"),
        name: "write".to_owned(),
        args,
        deadline_ms: 30_000,
    }
}

/// One frame of each kind, with each field set.
fn every_frame() -> Vec<Frame> {
    vec![
        Frame::Hello {
            protocol: PROTOCOL,
            fingerprint: "f1".to_owned(),
            kinds: vec!["agent".to_owned()],
            pid: 4242,
        },
        Frame::Welcome {
            protocol: PROTOCOL,
            fingerprint: "f1".to_owned(),
            kinds: vec!["realm".to_owned(), "workspace".to_owned()],
            routes: Routes::only(&[
                (CallerKind::Actor("agent".to_owned()), &["realm"]),
                (CallerKind::Client, &["realm", "agent"]),
            ]),
        },
        Frame::Refused {
            reason: "another fingerprint".to_owned(),
        },
        Frame::Ready,
        Frame::Goodbye,
        call(vec![0, 1, 2, 255]),
        Frame::Call {
            id: 8,
            from: Some(Caller::Client),
            to: Address::new("agent", "a2"),
            name: "say".to_owned(),
            args: Vec::new(),
            deadline_ms: 1,
        },
        Frame::Call {
            id: 9,
            from: None,
            to: Address::new("agent", "a3"),
            name: "stop".to_owned(),
            args: vec![9],
            deadline_ms: u64::MAX,
        },
        Frame::Reply {
            id: 7,
            result: Ok(ByteBuf::from(vec![3, 4])),
        },
        Frame::Reply {
            id: 8,
            result: Err(WireError {
                group: "actor".to_owned(),
                code: "not_found".to_owned(),
                message: "no actor".to_owned(),
                metadata: Some(r#"{"key":"a2"}"#.to_owned()),
            }),
        },
        Frame::HostEvent(HostEvent::TimerSetAside {
            actor: Address::new("agent", "a1"),
            event_id: "e1".to_owned(),
            action: "wake".to_owned(),
            due_at: 1_700_000_000_000,
            tries: 5,
            error: Some("actor.action_timed_out".to_owned()),
        }),
        Frame::HostEvent(HostEvent::UnreadableFile {
            actor: Address::new("agent", "a4"),
            error: "not a database".to_owned(),
        }),
        Frame::HostEvent(HostEvent::Refused {
            from: Caller::Actor(Address::new("agent", "a1")),
            to: Address::new("agent", "a2"),
            action: Some("ping".to_owned()),
        }),
    ]
}

fn encoded(frame: &Frame) -> Vec<u8> {
    let mut out = Vec::new();
    link::write(&mut out, frame).unwrap();
    out
}

#[test]
fn every_frame_round_trips_and_the_stream_ends_cleanly() {
    let mut stream = Vec::new();
    for frame in every_frame() {
        link::write(&mut stream, &frame).unwrap();
    }
    let mut input = stream.as_slice();
    for frame in every_frame() {
        assert_eq!(link::read(&mut input).unwrap(), Some(frame));
    }
    assert_eq!(link::read(&mut input).unwrap(), None);
}

#[test]
fn a_write_at_the_workspace_cap_round_trips_through_a_pipe() {
    let frame = call(vec![7; WRITE_CAP]);
    let (mut reader, mut writer) = pipe().unwrap();
    let sent = frame.clone();
    let writing =
        thread::spawn(move || link::write(&mut writer, &sent));
    assert_eq!(link::read(&mut reader).unwrap(), Some(frame));
    writing.join().unwrap().unwrap();
    assert_eq!(link::read(&mut reader).unwrap(), None);
}

#[test]
fn a_frame_over_the_limit_is_not_written() {
    let mut out = Vec::new();
    let error = link::write(&mut out, &call(vec![0; FRAME_LIMIT]))
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert!(out.is_empty());
}

#[test]
fn a_bad_magic_breaks_the_link() {
    let mut bytes = encoded(&Frame::Ready);
    bytes[..4].copy_from_slice(b"INFO");
    let read = link::read(&mut bytes.as_slice());
    assert!(
        matches!(read, Err(Broken::Magic(magic)) if &magic == b"INFO"),
        "{read:?}"
    );
}

#[test]
fn a_length_over_the_limit_breaks_the_link_before_its_body() {
    let mut bytes = encoded(&Frame::Ready);
    let length = u32::try_from(FRAME_LIMIT + 1).unwrap();
    bytes[4..8].copy_from_slice(&length.to_be_bytes());
    let read = link::read(&mut bytes.as_slice());
    assert!(
        matches!(read, Err(Broken::TooLong(n)) if n == FRAME_LIMIT + 1),
        "{read:?}"
    );
}

#[test]
fn a_cut_frame_breaks_the_link() {
    let bytes = encoded(&call(vec![1; 100]));
    for cut in [1, 4, 7, 8, bytes.len() - 1] {
        let read = link::read(&mut &bytes[..cut]);
        assert!(
            matches!(read, Err(Broken::Cut)),
            "cut at {cut}: {read:?}"
        );
    }
}

#[test]
fn a_body_that_is_not_one_frame_breaks_the_link() {
    let mut garbage = encoded(&Frame::Goodbye);
    garbage.truncate(8);
    garbage.extend_from_slice(&[0xff; 4]);
    garbage[4..8].copy_from_slice(&4u32.to_be_bytes());

    let mut trailing = encoded(&Frame::Goodbye);
    trailing.push(0);
    let length = u32::try_from(trailing.len() - 8).unwrap();
    trailing[4..8].copy_from_slice(&length.to_be_bytes());

    for bytes in [garbage, trailing] {
        let read = link::read(&mut bytes.as_slice());
        assert!(matches!(read, Err(Broken::Body(_))), "{read:?}");
    }
}

/// The shapes of the frames, as serde writes them, in a stable text.
fn shapes() -> String {
    // Each string the tracer reads is a version, so that a version
    // field reads as one; any other string field takes it as well.
    let config = TracerConfig::default()
        .default_string_value(PROTOCOL.to_string());
    let mut tracer = Tracer::new(config);
    tracer.trace_simple_type::<Caller>().unwrap();
    tracer.trace_simple_type::<CallerKind>().unwrap();
    tracer.trace_simple_type::<Routes>().unwrap();
    tracer.trace_simple_type::<HostEvent>().unwrap();
    tracer
        .trace_simple_type::<Result<ByteBuf, WireError>>()
        .unwrap();
    tracer.trace_simple_type::<Frame>().unwrap();
    serde_json::to_string_pretty(&tracer.registry().unwrap()).unwrap()
}

/// The fingerprint file for the frames' shapes `shapes`.
fn fingerprint(shapes: &str) -> String {
    let hash = Sha256::digest(format!("v {PROTOCOL}\n{shapes}"));
    format!(
        "# The link protocol's version and a sha256 of the shapes \
         of its frames\n# with that version, written by `cargo test \
         -p hamlet_host`. Do not edit.\nv {PROTOCOL}\nsha256 \
         {hash:x}\n"
    )
}

#[test]
fn the_frames_shapes_are_recorded() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(FINGERPRINT);
    let expected = fingerprint(&shapes());
    if std::fs::read_to_string(&path).ok().as_deref()
        == Some(&expected)
    {
        assert!(
            expected.ends_with(&format!("{}\n", link::fingerprint()))
        );
        return;
    }
    std::fs::write(&path, expected).unwrap();
    panic!(
        "The link's frames changed, so \
         crates/hamlet_host/{FINGERPRINT} is rewritten; commit it. \
         A Node host built before the change is refused at its \
         hello."
    );
}

#[cfg(unix)]
#[path = "support/logs.rs"]
mod logs;

/// The handshake, with a stand-in Node host as the child: the
/// example `link_child`, built beside this test.
#[cfg(unix)]
mod handshake {
    use std::io::Write;
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use hamlet_host::child::{ChildOptions, LinkState, Supervisor};
    use hamlet_host::link::{self, Frame, PROTOCOL};
    use hamlet_host::route::Routes;
    use hamlet_testing::Scratch;
    use hamlet_testing::signals::{is_alive, kill};
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::sync::watch;
    use tracing_subscriber::filter::LevelFilter;

    use super::logs::Logs;

    /// How long a test waits for a child to be ready.
    const READY_WAIT: Duration = Duration::from_secs(30);

    /// How long a child may outlive its link.
    const GONE_WAIT: Duration = Duration::from_secs(2);

    /// The folder of the Rust host [`rust_host`] runs. Unset, that
    /// test does nothing.
    const HOST_HOME: &str = "HAMLET_LINK_TEST_HOME";

    /// Set, the Rust host's child holds.
    const HOST_HOLD: &str = "HAMLET_LINK_TEST_HOLD";

    /// What the Rust host's child does on the link.
    enum StandIn {
        Serves,
        Holds,
    }

    /// The stand-in Node host, built beside this test.
    fn stand_in() -> PathBuf {
        let exe = std::env::current_exe().unwrap();
        let path = exe
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("examples")
            .join("link_child");
        assert!(
            path.exists(),
            "no stand-in Node host at {}: build it with `cargo \
             build -p hamlet_host --examples`",
            path.display()
        );
        path
    }

    fn options(home: &Path, args: &[&str]) -> ChildOptions {
        let mut argv = vec![stand_in().to_str().unwrap().to_owned()];
        argv.extend(args.iter().map(|arg| (*arg).to_owned()));
        ChildOptions {
            argv,
            home: home.to_str().unwrap().to_owned(),
            kinds: vec!["realm".to_owned()],
            node_kinds: vec!["agent".to_owned()],
            routes: Routes::Any,
        }
    }

    /// The stand-in, started by the test itself on `home`, with this
    /// process as its Rust host. Spawn it on the test's own thread:
    /// Linux kills it when the thread that started it ends.
    fn own_stand_in(home: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(stand_in());
        command
            .args(args)
            .env_clear()
            .env("HAMLET_HOME", home)
            .env("HAMLET_LINK_PARENT", std::process::id().to_string())
            .stdin(Stdio::piped());
        command
    }

    async fn wait_for(
        state: &mut watch::Receiver<LinkState>,
        to: LinkState,
    ) {
        let reached = tokio::time::timeout(
            READY_WAIT,
            state.wait_for(|now| *now == to),
        )
        .await;
        assert!(
            matches!(reached, Ok(Ok(_))),
            "the link never got {to:?}"
        );
    }

    /// Each start the stand-in recorded: its process id and when it
    /// started, in Unix milliseconds.
    fn starts(path: &Path) -> Vec<(u32, u64)> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|line| {
                let (pid, ms) = line.split_once(' ').unwrap();
                (pid.parse().unwrap(), ms.parse().unwrap())
            })
            .collect()
    }

    async fn wait_for_starts(
        path: &Path,
        count: usize,
    ) -> Vec<(u32, u64)> {
        let deadline = Instant::now() + READY_WAIT;
        loop {
            let starts = starts(path);
            if starts.len() >= count {
                return starts;
            }
            assert!(Instant::now() < deadline, "{starts:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Whether a line of `logs` holds each of `words`.
    fn has_line(logs: &Logs, words: &[&str]) -> bool {
        logs.text()
            .lines()
            .any(|line| words.iter().all(|word| line.contains(word)))
    }

    #[tokio::test]
    async fn a_child_gets_ready_and_exits_cleanly_at_the_shutdown() {
        let home = Scratch::new("link-ready");
        let supervisor =
            Supervisor::start(options(&home, &[])).unwrap();
        wait_for(&mut supervisor.state(), LinkState::Ready).await;
        assert!(home.join("node.lock").exists());
        let status =
            supervisor.shutdown().await.expect("a child ran");
        assert!(status.success(), "{status}");
    }

    #[tokio::test]
    async fn a_hello_of_another_protocol_is_refused_and_logged() {
        let home = Scratch::new("link-protocol");
        let logs = Logs::default();
        let _recording = logs.record(LevelFilter::ERROR);
        let supervisor = Supervisor::start(options(
            &home,
            &["--protocol", "9.0.0"],
        ))
        .unwrap();
        let state = supervisor.state();
        let deadline = Instant::now() + READY_WAIT;
        while !has_line(&logs, &["refused", "9.0.0"]) {
            assert!(Instant::now() < deadline, "{}", logs.text());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!state.has_changed().unwrap());
        assert_eq!(*state.borrow(), LinkState::Down);
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn a_refused_child_is_followed_by_a_new_one_a_second_later_each_time()
     {
        let home = Scratch::new("link-restart");
        let record = home.join("starts");
        let supervisor = Supervisor::start(options(
            &home,
            &[
                "--protocol",
                "9.0.0",
                "--starts",
                record.to_str().unwrap(),
            ],
        ))
        .unwrap();
        let starts = wait_for_starts(&record, 3).await;
        supervisor.shutdown().await;
        for pair in starts.windows(2) {
            let gap = pair[1].1 - pair[0].1;
            assert!((1000..1800).contains(&gap), "{starts:?}");
        }
    }

    #[tokio::test]
    async fn a_hello_of_other_kinds_is_refused_before_the_child_opens_a_file()
     {
        let home = Scratch::new("link-kinds");
        let record = home.join("starts");
        let logs = Logs::default();
        let _recording = logs.record(LevelFilter::ERROR);
        let mut options =
            options(&home, &["--starts", record.to_str().unwrap()]);
        options.node_kinds = vec!["profile".to_owned()];
        let supervisor = Supervisor::start(options).unwrap();
        wait_for_starts(&record, 2).await;
        assert!(
            has_line(&logs, &["refused", "profile"]),
            "{}",
            logs.text()
        );
        supervisor.shutdown().await;
        assert!(!home.join("node.lock").exists());
        assert!(!home.join("actors").exists());
    }

    #[tokio::test]
    async fn a_second_node_host_on_the_folder_fails_at_its_lock_and_is_never_ready()
     {
        let home = Scratch::new("link-lock");
        let supervisor =
            Supervisor::start(options(&home, &[])).unwrap();
        let mut state = supervisor.state();
        wait_for(&mut state, LinkState::Ready).await;

        let mut second = own_stand_in(&home, &[])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = second.stdin.take().unwrap();
        let mut output = second.stdout.take().unwrap();
        let hello = link::read(&mut output).unwrap();
        assert!(
            matches!(&hello, Some(Frame::Hello { kinds, .. }) if kinds == &["agent"]),
            "{hello:?}"
        );
        link::write(
            &mut input,
            &Frame::Welcome {
                protocol: PROTOCOL,
                fingerprint: link::fingerprint().to_owned(),
                kinds: vec!["realm".to_owned()],
                routes: Routes::Any,
            },
        )
        .unwrap();
        assert_eq!(link::read(&mut output).unwrap(), None);
        let ended = second.wait_with_output().unwrap();
        assert!(!ended.status.success());
        let stderr = String::from_utf8_lossy(&ended.stderr);
        let lock = home.join("node.lock");
        assert!(
            stderr.contains(&lock.display().to_string()),
            "{stderr}"
        );

        assert!(!state.has_changed().unwrap());
        assert_eq!(*state.borrow(), LinkState::Ready);
        drop(input);
        assert!(supervisor.shutdown().await.unwrap().success());
    }

    #[tokio::test]
    async fn the_childs_environment_holds_only_the_named_variables() {
        assert!(std::env::vars_os().any(|(name, _)| {
            name.to_string_lossy().starts_with("CARGO_")
        }));
        let home = Scratch::new("link-env");
        let names = home.join("env");
        let supervisor = Supervisor::start(options(
            &home,
            &["--env", names.to_str().unwrap()],
        ))
        .unwrap();
        wait_for(&mut supervisor.state(), LinkState::Ready).await;
        supervisor.shutdown().await;

        let mut expected = vec!["HAMLET_HOME", "HAMLET_LINK_PARENT"];
        for inherited in ["PATH", "HAMLET_FAULT_DIR"] {
            if std::env::var_os(inherited).is_some() {
                expected.push(inherited);
            }
        }
        expected.sort_unstable();
        let names = std::fs::read_to_string(names).unwrap();
        let mut names = names.lines().collect::<Vec<_>>();
        names.sort_unstable();
        assert_eq!(names, expected);
    }

    /// Runs as a Rust host in a process of its own, with the stand-in
    /// as its child, until it is killed. Prints `ready` once the link
    /// is ready, or at once when the child holds.
    #[tokio::test]
    async fn rust_host() {
        let Some(home) = std::env::var_os(HOST_HOME) else {
            return;
        };
        let home = PathBuf::from(home);
        let record = home.join("starts");
        let hold = std::env::var_os(HOST_HOLD).is_some();
        let mut args = vec!["--starts", record.to_str().unwrap()];
        if hold {
            args.push("--hold");
        }
        let supervisor =
            Supervisor::start(options(&home, &args)).unwrap();
        if !hold {
            wait_for(&mut supervisor.state(), LinkState::Ready).await;
        }
        writeln!(std::io::stdout(), "ready").unwrap();
        std::future::pending::<()>().await;
    }

    /// Runs [`rust_host`] in a process of its own with its child
    /// doing `stand_in`, kills it with SIGKILL once it is ready and
    /// its child has started, and asserts that the child is gone
    /// within [`GONE_WAIT`].
    async fn kill_a_rust_host(home: &Path, stand_in: StandIn) {
        let mut command = tokio::process::Command::new(
            std::env::current_exe().unwrap(),
        );
        command
            .args(["--exact", "handshake::rust_host", "--nocapture"])
            .env(HOST_HOME, home)
            .stdout(Stdio::piped())
            .kill_on_drop(true);
        if let StandIn::Holds = stand_in {
            command.env(HOST_HOLD, "1");
        }
        let mut host = command.spawn().unwrap();
        let mut lines =
            BufReader::new(host.stdout.take().unwrap()).lines();
        let ready = tokio::time::timeout(READY_WAIT, async {
            while let Some(line) = lines.next_line().await.unwrap() {
                if line == "ready" {
                    return true;
                }
            }
            false
        })
        .await;
        assert_eq!(ready, Ok(true));
        let (pid, _) =
            wait_for_starts(&home.join("starts"), 1).await[0];
        assert!(is_alive(pid));
        host.kill().await.unwrap();
        let killed = Instant::now();
        while is_alive(pid) {
            if killed.elapsed() > GONE_WAIT {
                let _ = kill(pid);
                panic!("the child {pid} outlived its Rust host");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn a_killed_rust_hosts_child_is_gone_within_two_seconds() {
        kill_a_rust_host(&Scratch::new("link-kill"), StandIn::Serves)
            .await;
    }

    #[tokio::test]
    async fn a_killed_rust_hosts_child_that_has_not_said_hello_is_gone_within_two_seconds()
     {
        kill_a_rust_host(&Scratch::new("link-hold"), StandIn::Holds)
            .await;
    }

    #[tokio::test]
    async fn a_child_whose_link_input_ends_kills_itself() {
        let home = Scratch::new("link-input");
        let record = home.join("starts");
        let mut child = own_stand_in(
            &home,
            &["--hold", "--starts", record.to_str().unwrap()],
        )
        .spawn()
        .unwrap();
        wait_for_starts(&record, 1).await;
        drop(child.stdin.take());
        let dropped = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if dropped.elapsed() > GONE_WAIT {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the child outlived its link's input");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert_eq!(status.signal(), Some(9), "{status}");
    }
}
