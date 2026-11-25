//! The Node host as the server watches it.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::process::Stdio;
use std::time::{Duration, Instant};

use hamlet_acceptance::Client;
use hamlet_os::process::{Command, Limits, Tree};
use hamlet_testing::signals::is_alive;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};

use super::lines::processes;
use super::{PATIENCE, kill_node, serve};

#[tokio::test(flavor = "multi_thread")]
async fn a_node_host_killed_by_a_signal_is_logged_with_its_name() {
    let (mut server, home, _side) = serve(json!(["Up."])).await;
    let mut chief = Client::join(&server).await;
    let hand = chief.hand();
    chief.say(hand, "Are you up?").await;
    kill_node(&home).await;
    let began = Instant::now();
    while !server.log().contains(
        "WARN hamlet_host::child: the Node host ended \
         status=signal: 9 (SIGKILL)",
    ) {
        assert!(began.elapsed() < PATIENCE, "{}", server.log());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    chief.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}

/// The diagnosis of a macOS fault, where a Node host stopped with
/// SIGSTOP ended at once. Fails on purpose with what it saw of a
/// stopped child started as the server starts the Node host, then of
/// one in this test's own group.
#[tokio::test(flavor = "multi_thread")]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "the diagnosis of a macOS fault"
)]
async fn a_stopped_child_is_seen_as_stopped() {
    let mut facts = String::new();
    let argv = [
        "sh".to_owned(),
        "-c".into(),
        "echo $$; exec sleep 30".into(),
    ];
    let env = BTreeMap::from([(
        "PATH".to_owned(),
        std::env::var("PATH").unwrap_or_default(),
    )]);
    let (mut tree, stdout, _) = Tree::spawn(
        Command {
            argv: &argv,
            env: &env,
            cwd: "/",
        },
        Stdio::null(),
        &Limits::default(),
    )
    .unwrap();
    let line = BufReader::new(stdout).lines().next_line().await;
    let pid: u32 = line.unwrap().unwrap().parse().unwrap();
    writeln!(facts, "In a session of its own, {pid}:").unwrap();
    stopped(&mut facts, pid);
    let ended =
        tokio::time::timeout(Duration::from_secs(3), tree.ended())
            .await;
    writeln!(facts, "the server's wait within 3 s: {ended:?}")
        .unwrap();
    after(&mut facts, pid).await;
    let ended = tokio::time::timeout(PATIENCE, tree.ended()).await;
    writeln!(facts, "the server's wait once killed: {ended:?}")
        .unwrap();

    #[expect(
        clippy::disallowed_methods,
        reason = "a plain child, as the diagnosis compares it"
    )]
    let mut own = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = own.id();
    writeln!(facts, "\nIn this test's group, {pid}:").unwrap();
    stopped(&mut facts, pid);
    tokio::time::sleep(Duration::from_secs(3)).await;
    after(&mut facts, pid).await;
    writeln!(facts, "reaped: {:?}", own.wait()).unwrap();
    panic!("{facts}");
}

/// Stops `pid` and notes what a wait for its exit reads then.
fn stopped(facts: &mut String, pid: u32) {
    let sent = signal::send(pid, signal::STOP);
    std::thread::sleep(Duration::from_millis(100));
    writeln!(facts, "kill -STOP: {sent:?}").unwrap();
    writeln!(facts, "waitid once stopped: {}", signal::waited(pid))
        .unwrap();
    writeln!(facts, "{}", processes(&pid.to_string())).unwrap();
}

/// Notes whether the stopped `pid` still runs, then kills it and
/// notes what a wait for its exit reads.
async fn after(facts: &mut String, pid: u32) {
    writeln!(facts, "alive after 3 s: {}", is_alive(pid)).unwrap();
    writeln!(facts, "waitid after 3 s: {}", signal::waited(pid))
        .unwrap();
    writeln!(facts, "{}", processes(&pid.to_string())).unwrap();
    let sent = signal::send(pid, signal::KILL);
    tokio::time::sleep(Duration::from_millis(200)).await;
    writeln!(facts, "kill -KILL: {sent:?}").unwrap();
    writeln!(facts, "waitid once killed: {}", signal::waited(pid))
        .unwrap();
}

/// The system calls of the diagnosis, as the server's wait makes
/// them.
#[expect(
    unsafe_code,
    reason = "the diagnosis reads waitid's own answer, unfiltered"
)]
mod signal {
    use std::ffi::{c_int, c_uint};
    use std::io;

    pub(super) const STOP: c_int = 17 * MACOS + 19 * LINUX;
    pub(super) const KILL: c_int = 9;
    const MACOS: c_int = cfg!(target_os = "macos") as c_int;
    const LINUX: c_int = 1 - MACOS;
    const P_PID: c_int = 1;
    const WNOHANG: c_int = 1;
    const WEXITED: c_int = 4;
    const WNOWAIT: c_int = 0x20 * MACOS + 0x0100_0000 * LINUX;
    /// Where `si_pid` and `si_status` are, in 4-byte words.
    const PID: usize = 3 * MACOS as usize + 4 * LINUX as usize;
    const STATUS: usize = 5 * MACOS as usize + 6 * LINUX as usize;

    /// A `siginfo_t`, as 4-byte words.
    #[repr(C, align(8))]
    struct Info([i32; 32]);

    unsafe extern "C" {
        fn waitid(
            idtype: c_int,
            id: c_uint,
            info: *mut Info,
            options: c_int,
        ) -> c_int;
        fn kill(pid: c_int, signal: c_int) -> c_int;
    }

    pub(super) fn send(pid: u32, signal: c_int) -> io::Result<()> {
        // SAFETY: kill reads only its two numbers.
        match unsafe { kill(pid as c_int, signal) } {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    }

    /// What `waitid` answers for `pid` with the server's options.
    pub(super) fn waited(pid: u32) -> String {
        let mut info = Info([0; 32]);
        // SAFETY: `info` is as large as a siginfo_t on both systems
        // and aligned as one, and outlives the call.
        let answer = unsafe {
            waitid(
                P_PID,
                pid,
                &raw mut info,
                WEXITED | WNOWAIT | WNOHANG,
            )
        };
        let error = (answer != 0).then(io::Error::last_os_error);
        let words = &info.0;
        format!(
            "returned {answer} {error:?}, si_signo {} si_code {} \
             si_pid {} si_status {}, words {:?}",
            words[0],
            words[2],
            words[PID],
            words[STATUS],
            &words[..8]
        )
    }
}
