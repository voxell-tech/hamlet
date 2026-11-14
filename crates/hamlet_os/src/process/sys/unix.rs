#![expect(
    unsafe_code,
    reason = "a leader is set up between fork and exec, and a bound \
              child adopts the ends its parent kept open for it"
)]

#[cfg(target_os = "linux")]
use std::convert::Infallible;
use std::fs::File;
use std::io::{self, PipeWriter, Write};
use std::os::fd::{
    AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd,
};
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Stdio};
#[cfg(target_os = "linux")]
use std::sync::mpsc;

use rustix::io::{FdFlags, fcntl_dupfd_cloexec, fcntl_setfd};
use rustix::process::{
    Pid, Signal, WaitId, WaitIdOptions, WaitIdStatus, getpid,
    kill_process, kill_process_group, waitid,
};
use tokio::process::{Child, ChildStderr, ChildStdout};
use tokio::signal::unix::{SignalKind, signal};

use crate::process::{Command, Ends, Limits, Preamble, no_ends};

/// A process group and the leader it was started with. The leader is
/// reaped only as the group is dropped, and an id stays taken while
/// its process is unreaped, so no other group can take the group's id
/// first: every signal reaches only this group.
pub(crate) struct Group {
    id: Pid,
    leader: Child,
    /// SIGCHLD, heard from before the leader started.
    exits: tokio::signal::unix::Signal,
    #[cfg(target_os = "linux")]
    forked_from: Option<mpsc::Sender<Infallible>>,
}

impl Group {
    /// Kills the group and its leader, which may have left it.
    pub(crate) fn kill(&mut self) {
        let _ = kill_process_group(self.id, Signal::KILL);
        let _ = self.leader.start_kill();
    }

    /// Waits for the leader to exit, and leaves it unreaped.
    pub(crate) async fn exited(&mut self) -> io::Result<ExitStatus> {
        loop {
            let options = WaitIdOptions::EXITED
                | WaitIdOptions::NOWAIT
                | WaitIdOptions::NOHANG;
            if let Some(status) =
                waitid(WaitId::Pid(self.id), options)?
            {
                return Ok(exit_status(&status));
            }
            self.exits.recv().await.ok_or_else(|| {
                io::Error::other("the runtime stopped listening")
            })?;
        }
    }

    /// Whether a process of the group other than its exited leader
    /// still runs.
    pub(crate) fn is_running(&self) -> bool {
        has_live_members(self.id)
    }

    /// Kills the group, then reaps its leader.
    pub(crate) fn end(self) {
        let _ = kill_process_group(self.id, Signal::KILL);
    }
}

/// The status `waitpid` would give for `waited`.
fn exit_status(waited: &WaitIdStatus) -> ExitStatus {
    let raw =
        match (waited.exit_status(), waited.terminating_signal()) {
            (Some(code), _) => (code & 0xff) << 8,
            (None, Some(signal)) if waited.dumped() => signal | 0x80,
            (None, Some(signal)) => signal,
            (None, None) => 0,
        };
    ExitStatus::from_raw(raw)
}

/// Whether a process that is not a zombie has `id` as its group.
#[cfg(target_os = "linux")]
fn has_live_members(id: Pid) -> bool {
    // Kept when unknown: ending them reaches only this group.
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return true;
    };
    entries.flatten().any(|entry| {
        let Ok(stat) =
            std::fs::read_to_string(entry.path().join("stat"))
        else {
            return false;
        };
        // The name, in parentheses, may itself hold spaces and ')'.
        let fields = stat
            .rfind(')')
            .map(|end| {
                stat[end + 1..].split_whitespace().collect::<Vec<_>>()
            })
            .unwrap_or_default();
        matches!(
            fields.as_slice(),
            [state, _, group, ..]
                if !matches!(*state, "Z" | "X")
                    && group.parse() == Ok(id.as_raw_nonzero().get())
        )
    })
}

/// Whether a process that is not a zombie has `id` as its group.
/// macOS's `killpg` skips zombies, and fails with `EPERM` rather than
/// `ESRCH` when the group holds only zombies.
#[cfg(not(target_os = "linux"))]
fn has_live_members(id: Pid) -> bool {
    rustix::process::test_kill_process_group(id).is_ok()
}

pub(crate) fn spawn(
    command: Command<'_>,
    stdin: Stdio,
    _limits: &Limits,
) -> io::Result<(Group, ChildStdout, ChildStderr)> {
    let mut spawn = command.program();
    spawn
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut group = start(spawn)?;
    let (Some(stdout), Some(stderr)) =
        (group.leader.stdout.take(), group.leader.stderr.take())
    else {
        unreachable!("both outputs are piped");
    };
    Ok((group, stdout, stderr))
}

pub(crate) fn spawn_bound(
    mut spawn: tokio::process::Command,
    mut told: PipeWriter,
    ends: Ends,
) -> io::Result<Group> {
    let ends = [
        above_standard(ends.input.into())?,
        above_standard(ends.output.into())?,
    ];
    told.write_all(
        &Preamble {
            input: ends[0].as_raw_fd() as u64,
            output: ends[1].as_raw_fd() as u64,
            owner: number(getpid()),
        }
        .encode(),
    )?;
    drop(told);
    keep_open(&mut spawn, ends.each_ref().map(AsRawFd::as_raw_fd));
    #[cfg(target_os = "linux")]
    let group = start_from_a_lasting_thread(spawn);
    #[cfg(not(target_os = "linux"))]
    let group = start(spawn);
    group
}

fn start(mut spawn: tokio::process::Command) -> io::Result<Group> {
    let exits = signal(SignalKind::child())?;
    session(&mut spawn);
    let leader = spawn.spawn()?;
    let id = leader
        .id()
        .and_then(|id| Pid::from_raw(id as i32))
        .ok_or_else(|| io::Error::other("the leader has no id"))?;
    Ok(Group {
        id,
        leader,
        exits,
        #[cfg(target_os = "linux")]
        forked_from: None,
    })
}

/// Starts `spawn` from a thread that lives as long as the group:
/// Linux sends a bound leader its parent-death signal when the thread
/// that forked it ends.
#[cfg(target_os = "linux")]
fn start_from_a_lasting_thread(
    mut spawn: tokio::process::Command,
) -> io::Result<Group> {
    end_with_forking_thread(&mut spawn);
    let runtime = tokio::runtime::Handle::current();
    let (send_group, group) = mpsc::sync_channel(1);
    let (hold, held) = mpsc::channel::<Infallible>();
    std::thread::Builder::new()
        .name("bound-child".to_owned())
        .spawn(move || {
            let _runtime = runtime.enter();
            let _ = send_group.send(start(spawn));
            let _ = held.recv();
        })?;
    let mut group = group.recv().map_err(|_| {
        io::Error::other("the starting thread ended")
    })??;
    group.forked_from = Some(hold);
    Ok(group)
}

/// `end`, at a number above the standard streams', which a leader's
/// own replace before `pre_exec` runs.
fn above_standard(end: OwnedFd) -> io::Result<OwnedFd> {
    if end.as_raw_fd() > 2 {
        return Ok(end);
    }
    Ok(fcntl_dupfd_cloexec(&end, 3)?)
}

fn keep_open(spawn: &mut tokio::process::Command, ends: [RawFd; 2]) {
    // SAFETY: between fork and exec the leader calls only fcntl,
    // which is async-signal-safe, allocates nothing and touches no
    // shared state. `ends` are open in it: this process holds them
    // across the fork.
    unsafe {
        spawn.pre_exec(move || {
            for end in ends {
                fcntl_setfd(
                    BorrowedFd::borrow_raw(end),
                    FdFlags::empty(),
                )?;
            }
            Ok(())
        });
    }
}

#[cfg(target_os = "linux")]
fn end_with_forking_thread(spawn: &mut tokio::process::Command) {
    let parent = getpid();
    // SAFETY: between fork and exec the leader calls only prctl and
    // getppid, which are async-signal-safe, allocates nothing and
    // touches no shared state.
    unsafe {
        spawn.pre_exec(move || {
            rustix::process::set_parent_process_death_signal(Some(
                Signal::KILL,
            ))?;
            // A parent that ended before the signal was set sent
            // none.
            if rustix::process::getppid() != Some(parent) {
                return Err(io::ErrorKind::NotFound.into());
            }
            Ok(())
        });
    }
}

/// Makes `spawn`'s process the leader of a session of its own, and so
/// of a group whose id is its own: no terminal is left to its
/// processes, and they stay in the group the leader's end kills.
fn session(spawn: &mut tokio::process::Command) {
    // SAFETY: the child calls only setsid, which is
    // async-signal-safe, before exec, allocates nothing and touches
    // no shared state.
    unsafe {
        spawn.pre_exec(|| {
            rustix::process::setsid()?;
            Ok(())
        });
    }
}

pub(crate) fn limited(
    limits: &Limits,
    argv: &[String],
) -> Vec<String> {
    // bash counts file sizes in KiB. A lower machine limit stays in
    // force.
    let script = [
        ("t", limits.cpu_secs),
        ("u", limits.max_procs),
        ("f", limits.max_file_mb * 1024),
        ("c", 0),
    ]
    .iter()
    .map(|(flag, value)| {
        format!(
            "ulimit -{flag} {value} 2>/dev/null || ulimit -{flag} \
             hard"
        )
    })
    .collect::<Vec<_>>()
    .join("\n");
    let mut out = vec![
        "bash".to_owned(),
        "-c".into(),
        format!("{script}\nexec \"$@\""),
        "bash".into(),
    ];
    out.extend(argv.iter().cloned());
    out
}

pub(crate) fn unenforced(
    _limits: &Limits,
) -> &'static [&'static str] {
    &[]
}

pub(crate) fn is_executable(path: &str) -> bool {
    rustix::fs::access(path, rustix::fs::Access::EXEC_OK).is_ok()
}

/// Unix has no such job, so a crashed server leaves the leftovers of
/// its Seatbelt commands running.
pub(crate) fn reap_with_server() -> io::Result<()> {
    Ok(())
}

pub(crate) fn standard_input() -> io::Result<(File, u64)> {
    let stdin = io::stdin();
    let waiting = rustix::io::ioctl_fionread(&stdin)?;
    Ok((File::from(stdin.as_fd().try_clone_to_owned()?), waiting))
}

pub(crate) fn take_ends(preamble: &Preamble) -> io::Result<Ends> {
    let parent = rustix::process::getppid();
    #[cfg(not(target_os = "linux"))]
    if parent.is_none_or(Pid::is_init) {
        end_now();
    }
    if parent.map(number) != Some(preamble.owner) {
        return Err(no_ends());
    }
    let fd =
        |value: u64| RawFd::try_from(value).map_err(io::Error::other);
    let (input, output) = (fd(preamble.input)?, fd(preamble.output)?);
    // SAFETY: this process's parent kept both open across exec for
    // it, and the preamble that names them is read once, so nothing
    // else owns them.
    let (input, output) = unsafe {
        (OwnedFd::from_raw_fd(input), OwnedFd::from_raw_fd(output))
    };
    for end in [&input, &output] {
        fcntl_setfd(end, FdFlags::CLOEXEC)?;
    }
    #[cfg(not(target_os = "linux"))]
    if let Some(parent) = parent {
        watch_parent(parent)?;
    }
    Ok(Ends {
        input: input.into(),
        output: output.into(),
    })
}

#[cfg(not(target_os = "linux"))]
fn watch_parent(parent: Pid) -> io::Result<()> {
    use std::mem::MaybeUninit;

    use rustix::event::kqueue::{
        Event, EventFilter, EventFlags, ProcessEvents, kevent, kqueue,
    };

    let queue = kqueue()?;
    let exit = Event::new(
        EventFilter::Proc {
            pid: parent,
            flags: ProcessEvents::EXIT,
        },
        EventFlags::ADD | EventFlags::ONESHOT,
        std::ptr::null_mut(),
    );
    let mut no_events: [Event; 0] = [];
    // SAFETY: the event names a process, and no file descriptor that
    // would have to outlive the queue.
    let added =
        unsafe { kevent(&queue, &[exit], &mut no_events, None) };
    match added {
        Err(rustix::io::Errno::SRCH) => end_now(),
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    // A parent that exited before the event was added may have given
    // its id to another process.
    if rustix::process::getppid() != Some(parent) {
        end_now();
    }
    std::thread::Builder::new()
        .name("parent-watch".to_owned())
        .spawn(move || {
            loop {
                let mut exit =
                    [const { MaybeUninit::<Event>::uninit() }; 1];
                // SAFETY: as above.
                match unsafe { kevent(&queue, &[], &mut exit, None) }
                {
                    Err(rustix::io::Errno::INTR) => {}
                    Ok(_) | Err(_) => end_now(),
                }
            }
        })?;
    Ok(())
}

/// The number a preamble names `pid` by.
fn number(pid: Pid) -> u64 {
    pid.as_raw_nonzero().get().unsigned_abs().into()
}

pub(crate) fn end_now() -> ! {
    if kill_process(getpid(), Signal::KILL).is_err() {
        std::process::abort();
    }
    // macOS can deliver the signal after this call returns.
    loop {
        std::thread::park();
    }
}
