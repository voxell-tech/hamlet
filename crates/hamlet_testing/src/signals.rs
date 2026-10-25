//! Signals tests send to the processes they started.

use std::io;

use rustix::process::{Pid, Signal, kill_process, test_kill_process};

fn process(pid: u32) -> io::Result<Pid> {
    Pid::from_raw(pid as i32)
        .ok_or_else(|| io::ErrorKind::NotFound.into())
}

/// Ends the process `pid` at once.
pub fn kill(pid: u32) -> io::Result<()> {
    Ok(kill_process(process(pid)?, Signal::KILL)?)
}

/// Asks the process `pid` to stop.
pub fn terminate(pid: u32) -> io::Result<()> {
    Ok(kill_process(process(pid)?, Signal::TERM)?)
}

/// Whether the process `pid` still runs.
pub fn is_alive(pid: u32) -> bool {
    process(pid).is_ok_and(|pid| test_kill_process(pid).is_ok())
}
