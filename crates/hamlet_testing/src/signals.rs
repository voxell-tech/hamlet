//! Signals tests send to the processes they started.

#[cfg(unix)]
use std::io;

#[cfg(unix)]
use rustix::process::{Pid, Signal, kill_process, test_kill_process};

#[cfg(unix)]
fn process(pid: u32) -> io::Result<Pid> {
    Pid::from_raw(pid as i32)
        .ok_or_else(|| io::ErrorKind::NotFound.into())
}

/// Ends the process `pid` at once.
#[cfg(unix)]
pub fn kill(pid: u32) -> io::Result<()> {
    Ok(kill_process(process(pid)?, Signal::KILL)?)
}

/// Asks the process `pid` to stop.
#[cfg(unix)]
pub fn terminate(pid: u32) -> io::Result<()> {
    Ok(kill_process(process(pid)?, Signal::TERM)?)
}

/// Whether the process `pid` still runs.
#[cfg(unix)]
pub fn is_alive(pid: u32) -> bool {
    process(pid).is_ok_and(|pid| test_kill_process(pid).is_ok())
}

/// Whether the process `pid` still runs.
#[cfg(windows)]
pub fn is_alive(pid: u32) -> bool {
    let listed = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output();
    listed.is_ok_and(|listed| {
        String::from_utf8_lossy(&listed.stdout)
            .contains(&format!("\"{pid}\""))
    })
}
