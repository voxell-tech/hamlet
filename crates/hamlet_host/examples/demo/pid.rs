//! The process id of the demo's Node host, which
//! hosts/node/src/agent/host.ts writes in the realm folder as
//! its digits and a newline.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::time::Instant;

/// The same file as `PID_FILE` of hosts/node/src/agent/host.ts.
pub const PID_FILE: &str = "demo-node.pid";

const POLL: Duration = Duration::from_millis(25);

/// The pid of the Node host that runs now. Just after a Node host
/// dies, [`PID_FILE`] still names it until the next one starts, and
/// while the next one writes it the file is empty or holds part of
/// its number: both are read again, for up to `within`.
pub async fn live_pid(home: &Path, within: Duration) -> Result<u32> {
    let until = Instant::now() + within;
    loop {
        let written = written(home)?;
        if let Some(pid) = written
            && !hamlet_os::process::has_ended(pid)?
        {
            return Ok(pid);
        }
        if Instant::now() >= until {
            match written {
                Some(pid) => bail!(
                    "the Node host {pid} has ended, and no other \
                     started within {} s",
                    within.as_secs()
                ),
                None => bail!(
                    "{} held no whole pid within {} s",
                    home.join(PID_FILE).display(),
                    within.as_secs()
                ),
            }
        }
        tokio::time::sleep(POLL).await;
    }
}

/// The pid [`PID_FILE`] holds once it is written whole.
fn written(home: &Path) -> Result<Option<u32>> {
    let path = home.join(PID_FILE);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))?;
    let Some(digits) = text.strip_suffix('\n') else {
        return Ok(None);
    };
    let pid = digits.parse().with_context(|| {
        format!("{} holds no pid", path.display())
    })?;
    Ok(Some(pid))
}
