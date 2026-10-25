use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Stdio};

use rustix::process::{
    Pid, Signal, WaitId, WaitIdOptions, WaitIdStatus,
    kill_process_group, waitid,
};
use tokio::process::{Child, ChildStderr, ChildStdout};
use tokio::signal::unix::{SignalKind, signal};

use crate::process::{Command, Limits};

/// A process group and the leader it was started with. The leader is
/// reaped only as the group is dropped, and an id stays taken while
/// its process is unreaped, so no other group can take the group's id
/// first: every signal reaches only this group.
pub(crate) struct Group {
    id: Pid,
    leader: Child,
    /// SIGCHLD, heard from before the leader started.
    exits: tokio::signal::unix::Signal,
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
    _limits: &Limits,
) -> io::Result<(Group, ChildStdout, ChildStderr)> {
    let exits = signal(SignalKind::child())?;
    let mut spawn = tokio::process::Command::new(&command.argv[0]);
    spawn
        .args(&command.argv[1..])
        .env_clear()
        .envs(command.env)
        .current_dir(command.cwd)
        .stdin(command.stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    let mut leader = spawn.spawn()?;
    let id = leader
        .id()
        .and_then(|id| Pid::from_raw(id as i32))
        .ok_or_else(|| io::Error::other("the leader has no id"))?;
    let (Some(stdout), Some(stderr)) =
        (leader.stdout.take(), leader.stderr.take())
    else {
        unreachable!("both outputs are piped");
    };
    Ok((Group { id, leader, exits }, stdout, stderr))
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
