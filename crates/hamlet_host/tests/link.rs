//! The link between the Rust host and its Node child.

use std::io::{ErrorKind, pipe};
use std::path::Path;
use std::thread;

use hamlet_host::link::{
    self, Ask, Broken, FRAME_LIMIT, Frame, PROTOCOL, WireError,
};
use hamlet_host::route::{CallerKind, Operation, Routes};
use hamlet_host::{Address, Caller, HostEvent};
use serde_bytes::ByteBuf;
use serde_reflection::{Tracer, TracerConfig};
use sha2::{Digest, Sha256};

/// The largest write the workspace takes, 16 MiB.
const WRITE_CAP: usize = 16 << 20;

/// The recorded fingerprint, in the crate's folder.
const FINGERPRINT: &str = "link.fingerprint";

fn call(args: Vec<u8>) -> Frame {
    Frame::Request {
        id: 7,
        from: Some(Caller::Actor(
            Address::new("agent", "a1").unwrap(),
        )),
        to: Address::new("workspace", "w1").unwrap(),
        ask: Ask::Call {
            name: "write".to_owned(),
            args,
            deadline_ms: 30_000,
        },
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
        Frame::Request {
            id: 8,
            from: Some(Caller::Client),
            to: Address::new("agent", "a2").unwrap(),
            ask: Ask::Call {
                name: "say".to_owned(),
                args: Vec::new(),
                deadline_ms: 1,
            },
        },
        Frame::Request {
            id: 9,
            from: None,
            to: Address::new("agent", "a3").unwrap(),
            ask: Ask::Call {
                name: "stop".to_owned(),
                args: vec![9],
                deadline_ms: u64::MAX,
            },
        },
        Frame::Request {
            id: 10,
            from: Some(Caller::Actor(
                Address::new("realm", "main").unwrap(),
            )),
            to: Address::new("agent", "3").unwrap(),
            ask: Ask::Create { input: vec![0xa0] },
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
            actor: Address::new("agent", "a1").unwrap(),
            event_id: "e1".to_owned(),
            action: "wake".to_owned(),
            due_at: 1_700_000_000_000,
            tries: 5,
            error: Some("actor.action_timed_out".to_owned()),
        }),
        Frame::HostEvent(HostEvent::UnreadableFile {
            actor: Address::new("agent", "a4").unwrap(),
            error: "not a database".to_owned(),
        }),
        Frame::HostEvent(HostEvent::Refused {
            from: Caller::Actor(Address::new("agent", "a1").unwrap()),
            to: Address::new("agent", "a2").unwrap(),
            operation: Operation::Call {
                action: "ping".to_owned(),
            },
        }),
        Frame::HostEvent(HostEvent::Refused {
            from: Caller::Client,
            to: Address::new("workspace", "w1").unwrap(),
            operation: Operation::Subscribe {
                event: "changes".to_owned(),
            },
        }),
        Frame::Request {
            id: 11,
            from: Some(Caller::Client),
            to: Address::new("agent", "a1").unwrap(),
            ask: Ask::Subscribe {
                event: "changes".to_owned(),
            },
        },
        Frame::Event {
            sub: 11,
            args: vec![0x81, 0x18, 0x64],
        },
        Frame::Gap { sub: 11 },
        Frame::SubscriptionEnded {
            sub: 11,
            error: WireError {
                group: "actor".to_owned(),
                code: "stopping".to_owned(),
                message: "Actor is stopping.".to_owned(),
                metadata: None,
            },
        },
        Frame::Unsubscribe { sub: 12 },
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
fn a_request_of_each_ask_has_the_recorded_bytes() {
    let call = Frame::Request {
        id: 7,
        from: Some(Caller::Actor(
            Address::new("realm", "main").unwrap(),
        )),
        to: Address::new("agent", "3").unwrap(),
        ask: Ask::Call {
            name: "submit".to_owned(),
            args: vec![0x81, 0xa0],
            deadline_ms: 10_000,
        },
    };
    let create = Frame::Request {
        id: 8,
        from: Some(Caller::Client),
        to: Address::new("agent", "3").unwrap(),
        ask: Ask::Create { input: vec![0xa0] },
    };
    let body = |frame| hex(&encoded(frame)[8..]);
    assert_eq!(
        body(&call),
        "a16772657175657374a4626964076466726f6da1656163746f72a2646b696e\
         64657265616c6d636b6579646d61696e62746fa2646b696e64656167656e74\
         636b657961336361736ba16463616c6ca3646e616d65667375626d69746461\
         7267734281a06b646561646c696e655f6d73192710"
    );
    assert_eq!(
        body(&create),
        "a16772657175657374a4626964086466726f6d66636c69656e7462746fa264\
         6b696e64656167656e74636b657961336361736ba166637265617465a16569\
         6e70757441a0"
    );
}

#[test]
fn a_subscription_frame_of_each_kind_has_the_recorded_bytes() {
    let subscribe = Frame::Request {
        id: 9,
        from: Some(Caller::Client),
        to: Address::new("agent", "3").unwrap(),
        ask: Ask::Subscribe {
            event: "changes".to_owned(),
        },
    };
    let ended = Frame::SubscriptionEnded {
        sub: 9,
        error: WireError {
            group: "actor".to_owned(),
            code: "destroyed".to_owned(),
            message: "Actor was destroyed.".to_owned(),
            metadata: None,
        },
    };
    let body = |frame: &Frame| hex(&encoded(frame)[8..]);
    assert_eq!(
        body(&subscribe),
        "a16772657175657374a4626964096466726f6d66636c69656e7462746fa264\
         6b696e64656167656e74636b657961336361736ba169737562736372696265\
         a1656576656e74676368616e676573"
    );
    assert_eq!(
        body(&Frame::Event {
            sub: 9,
            args: vec![0x81, 0x18, 0x64],
        }),
        "a1656576656e74a26373756209646172677343811864"
    );
    assert_eq!(
        body(&Frame::Gap { sub: 9 }),
        "a163676170a16373756209"
    );
    assert_eq!(
        body(&ended),
        "a172737562736372697074696f6e5f656e646564a2637375620965657272\
         6f72a46567726f7570656163746f7264636f64656964657374726f796564\
         676d657373616765744163746f72207761732064657374726f7965642e68\
         6d65746164617461f6"
    );
    assert_eq!(
        body(&Frame::Unsubscribe { sub: 9 }),
        "a16b756e737562736372696265a16373756209"
    );
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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
    tracer.trace_simple_type::<Operation>().unwrap();
    tracer.trace_simple_type::<HostEvent>().unwrap();
    tracer
        .trace_simple_type::<Result<ByteBuf, WireError>>()
        .unwrap();
    tracer.trace_simple_type::<Ask>().unwrap();
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

#[path = "support/linked.rs"]
mod linked;
#[path = "support/logs.rs"]
mod logs;

/// The handshake, with a stand-in Node host as the child: the
/// example `link_child`, built beside this test.
mod handshake {
    use std::collections::{BTreeMap, HashMap};
    use std::io::{Read, Write};
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    use hamlet_host::child::{
        self, ChildOptions, LinkState, Supervisor,
    };
    use hamlet_host::link::{self, Frame, PROTOCOL};
    use hamlet_host::route::Routes;
    use hamlet_host::{Host, HostOptions};
    use hamlet_os::process::Pipes;
    use hamlet_testing::Scratch;
    use hamlet_testing::signals::is_alive;
    #[cfg(unix)]
    use hamlet_testing::signals::kill;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tracing_subscriber::filter::LevelFilter;

    use super::linked::{
        self, GONE_WAIT, has_line, own_stand_in, wait_for,
    };
    use super::logs::Logs;

    /// How long a test waits for a child to be ready.
    const READY_WAIT: Duration = Duration::from_secs(30);

    /// The folder of the Rust host [`rust_host`] runs. Unset, that
    /// test does nothing.
    const HOST_HOME: &str = "HAMLET_LINK_TEST_HOME";

    /// Set, the Rust host's child holds.
    const HOST_HOLD: &str = "HAMLET_LINK_TEST_HOLD";

    /// A made-up token of the actor runtime, which no log holds.
    const ACTOR_TOKEN: &str = "made-up-actor-token-4711";

    /// A made-up proxy whose password no log holds.
    const PROXY: &str =
        "http://user:made-up-proxy-password@127.0.0.1:9";

    /// A made-up key outside what crosses.
    const OUTSIDE_KEY: &str = "made-up-hamlet-key-0815";

    /// The folder of the Rust host [`environment_host`] runs. Unset,
    /// that test does nothing.
    const ENVIRONMENT_HOME: &str =
        "HAMLET_LINK_TEST_ENVIRONMENT_HOME";

    /// The folder, in its realm folder, where [`environment_child`]
    /// writes its environment. Absent, that test does nothing.
    const WRITTEN: &str = "child-environment";

    /// What the Rust host's child does on the link.
    enum StandIn {
        Serves,
        Holds,
    }

    fn open(home: &Path, args: &[&str]) -> (Host, Supervisor) {
        open_with(home, linked::child(args))
    }

    fn open_with(
        home: &Path,
        child: ChildOptions,
    ) -> (Host, Supervisor) {
        child::open(
            home,
            HashMap::new(),
            HostOptions::default(),
            child,
        )
        .unwrap()
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
    #[tokio::test]
    async fn a_child_gets_ready_and_exits_cleanly_at_the_shutdown() {
        let home = Scratch::new("link-ready");
        let (_host, supervisor) = open(&home, &[]);
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
        let (_host, supervisor) =
            open(&home, &["--protocol", "9.0.0"]);
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
    async fn a_refused_child_reads_why_before_it_ends() {
        let home = Scratch::new("link-refused-reads");
        let (record, frames) =
            (home.join("starts"), home.join("frames"));
        let (_host, supervisor) = open(
            &home,
            &[
                "--protocol",
                "9.0.0",
                "--starts",
                record.to_str().unwrap(),
                "--frames",
                frames.to_str().unwrap(),
            ],
        );
        wait_for_starts(&record, 2).await;
        supervisor.shutdown().await;
        let read =
            std::fs::read_to_string(&frames).unwrap_or_default();
        assert_eq!(read.lines().next(), Some("refused"), "{read:?}");
    }

    #[tokio::test]
    async fn a_refused_child_is_followed_by_a_new_one_after_pauses_that_double()
     {
        let home = Scratch::new("link-restart");
        let record = home.join("starts");
        let (_host, supervisor) = open(
            &home,
            &[
                "--protocol",
                "9.0.0",
                "--starts",
                record.to_str().unwrap(),
            ],
        );
        let starts = wait_for_starts(&record, 4).await;
        supervisor.shutdown().await;
        assert!(starts[1].1 - starts[0].1 < 1800, "{starts:?}");
        let gaps =
            starts.windows(2).map(|pair| pair[1].1 - pair[0].1);
        for (gap, least) in gaps.zip([1000, 2000, 4000]) {
            assert!(gap >= least, "{starts:?}");
        }
    }

    #[tokio::test]
    async fn a_hello_of_other_kinds_is_refused_before_the_child_opens_a_file()
     {
        let home = Scratch::new("link-kinds");
        let record = home.join("starts");
        let logs = Logs::default();
        let _recording = logs.record(LevelFilter::ERROR);
        let mut child =
            linked::child(&["--starts", record.to_str().unwrap()]);
        child.kinds = vec!["profile".to_owned()];
        let (_host, supervisor) = open_with(&home, child);
        wait_for_starts(&record, 2).await;
        assert!(
            has_line(&logs, &["refused", "profile"]),
            "{}",
            logs.text()
        );
        supervisor.shutdown().await;
        assert!(!home.join("node.lock").exists());
        assert!(!home.join("actors").join("agent").exists());
    }

    #[tokio::test]
    async fn a_second_node_host_on_the_folder_fails_at_its_lock_and_is_never_ready()
     {
        let home = Scratch::new("link-lock");
        let (_host, supervisor) = open(&home, &[]);
        let mut state = supervisor.state();
        wait_for(&mut state, LinkState::Ready).await;

        let (
            mut second,
            Pipes {
                mut input,
                mut output,
                mut errors,
            },
        ) = own_stand_in(&home, &[]);
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
        assert!(!second.ended().await.unwrap().success());
        let mut stderr = String::new();
        errors.read_to_string(&mut stderr).unwrap();
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

    /// The variables [`environment_host`] runs with that its child
    /// gets, each with a made-up value.
    fn crossing() -> BTreeMap<String, String> {
        // Windows processes need the real one.
        #[cfg(windows)]
        let system_root = std::env::var("SystemRoot").unwrap();
        let mut crossing = vec![
            ("PATH", "/made-up/bin"),
            ("HAMLET_FAULT_DIR", "/made-up/faults"),
            ("HAMLET_REFLECT_EVERY", "7"),
            ("HAMLET_CLAUDE_EXECUTABLE", "/made-up/claude"),
            ("HAMLET_ACTOR_ENDPOINT", "http://127.0.0.1:9"),
            ("HAMLET_ACTOR_TOKEN", ACTOR_TOKEN),
            ("CLAUDE_CONFIG_DIR", "/made-up/claude-config"),
            ("HOME", "/made-up/home"),
            ("TMPDIR", "/made-up/tmp"),
            ("HTTP_PROXY", PROXY),
            ("HTTPS_PROXY", PROXY),
            ("NO_PROXY", "localhost"),
            ("NODE_EXTRA_CA_CERTS", "/made-up/ca.pem"),
            ("SSL_CERT_FILE", "/made-up/cert.pem"),
            ("SSL_CERT_DIR", "/made-up/certs"),
        ];
        #[cfg(not(windows))]
        crossing.extend([
            ("http_proxy", PROXY),
            ("https_proxy", PROXY),
            ("no_proxy", "localhost"),
        ]);
        #[cfg(windows)]
        crossing.extend([
            ("USERPROFILE", r"C:\made-up\home"),
            ("APPDATA", r"C:\made-up\roaming"),
            ("LOCALAPPDATA", r"C:\made-up\local"),
            ("TEMP", r"C:\made-up\temp"),
            ("TMP", r"C:\made-up\tmp"),
            ("SystemRoot", system_root.as_str()),
        ]);
        crossing
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect()
    }

    /// The Rust host has twice [`READY_WAIT`], a minute, to end; it
    /// takes well under a second.
    #[tokio::test]
    async fn the_child_gets_each_named_variable_with_its_value_and_no_other()
     {
        let home = Scratch::new("link-env");
        let mut command = tokio::process::Command::new(
            std::env::current_exe().unwrap(),
        );
        command
            .args([
                "--exact",
                "handshake::environment_host",
                "--nocapture",
            ])
            .env_clear()
            .envs(crossing())
            .env(ENVIRONMENT_HOME, &*home)
            .env("HAMLET_API_KEY", OUTSIDE_KEY)
            .env("ANTHROPIC_API_KEY", OUTSIDE_KEY)
            .env("HAMLET_ACTORS_TOKEN", OUTSIDE_KEY)
            .env("LANG", "C.UTF-8")
            .kill_on_drop(true);
        #[cfg(unix)]
        command.env("HAMLET_ACTOR_RAW", {
            use std::os::unix::ffi::OsStrExt;
            std::ffi::OsStr::from_bytes(b"not \xff UTF-8")
        });
        let output =
            tokio::time::timeout(2 * READY_WAIT, command.output())
                .await
                .expect("the Rust host ended")
                .unwrap();
        let printed = [&output.stdout[..], &output.stderr]
            .map(String::from_utf8_lossy)
            .join("\n");
        assert!(output.status.success(), "{printed}");
        assert!(printed.contains("the Node host ended"), "{printed}");
        for secret in
            [ACTOR_TOKEN, "made-up-proxy-password", OUTSIDE_KEY]
        {
            assert!(!printed.contains(secret), "{printed}");
        }

        let written =
            std::fs::read(home.join(WRITTEN).join("environment"))
                .unwrap();
        let mut got = serde_json::from_slice::<
            BTreeMap<String, String>,
        >(&written)
        .unwrap();
        // CoreFoundation sets it in each process that loads it.
        got.remove("__CF_USER_TEXT_ENCODING");
        let mut expected = crossing();
        expected.insert(
            "HAMLET_HOME".to_owned(),
            home.to_str().unwrap().to_owned(),
        );
        assert_eq!(got, expected);
    }

    /// Runs as a Rust host in a process of its own, with
    /// [`environment_child`] as its child, until that child has
    /// written its environment. Prints what it logged.
    #[tokio::test]
    async fn environment_host() {
        let Some(home) = std::env::var_os(ENVIRONMENT_HOME) else {
            return;
        };
        let home = PathBuf::from(home);
        let written = home.join(WRITTEN);
        std::fs::create_dir(&written).unwrap();
        let logs = Logs::default();
        let recording = logs.record(LevelFilter::TRACE);
        let exe = std::env::current_exe().unwrap();
        let (_host, supervisor) = open_with(
            &home,
            ChildOptions {
                argv: vec![
                    exe.to_str().unwrap().to_owned(),
                    "--exact".to_owned(),
                    "handshake::environment_child".to_owned(),
                ],
                kinds: vec!["agent".to_owned()],
            },
        );
        let deadline = Instant::now() + READY_WAIT;
        while !written.join("environment").exists() {
            assert!(Instant::now() < deadline, "{}", logs.text());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        supervisor.shutdown().await;
        drop(recording);
        write!(std::io::stdout(), "{}", logs.text()).unwrap();
    }

    /// Writes this process's environment, as JSON, to `environment`
    /// in the folder [`WRITTEN`] of the realm folder.
    #[test]
    fn environment_child() {
        let Some(home) = std::env::var_os("HAMLET_HOME") else {
            return;
        };
        let written = Path::new(&home).join(WRITTEN);
        if !written.is_dir() {
            return;
        }
        let environment = std::env::vars_os()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.to_string_lossy().into_owned(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let part = written.join("environment.part");
        std::fs::write(
            &part,
            serde_json::to_vec(&environment).unwrap(),
        )
        .unwrap();
        std::fs::rename(part, written.join("environment")).unwrap();
    }

    #[tokio::test]
    async fn what_a_child_wrote_to_the_standard_output_it_started_with_does_not_reach_the_link()
     {
        let home = Scratch::new("link-stray");
        let (_host, supervisor) = open(&home, &["--stray"]);
        wait_for(&mut supervisor.state(), LinkState::Ready).await;
        let status =
            supervisor.shutdown().await.expect("a child ran");
        assert!(status.success(), "{status}");
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
        let (_host, supervisor) = open(&home, &args);
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
                #[cfg(unix)]
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
        let (
            mut child,
            Pipes {
                input,
                output: _output,
                errors: _errors,
            },
        ) = own_stand_in(
            &home,
            &["--hold", "--starts", record.to_str().unwrap()],
        );
        wait_for_starts(&record, 1).await;
        drop(input);
        let status = tokio::time::timeout(GONE_WAIT, child.ended())
            .await
            .expect("the child outlived its link's input")
            .unwrap();
        #[cfg(unix)]
        assert_eq!(status.signal(), Some(9), "{status}");
        #[cfg(windows)]
        assert!(!status.success(), "{status}");
    }
}
