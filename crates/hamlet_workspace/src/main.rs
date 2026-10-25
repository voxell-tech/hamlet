//! The workspace worker and its confined operation supervisor.
//!
//! Every role relies on Linux namespaces, bubblewrap and `flock`, so
//! the binary is built only for Linux. Elsewhere it compiles to an
//! entry point that refuses to run, which keeps the Cargo workspace
//! building on the chief's own platform.

#[cfg(target_os = "linux")]
mod files;
#[cfg(target_os = "linux")]
mod http;
#[cfg(target_os = "linux")]
mod proxy;
#[cfg(target_os = "linux")]
mod supervise;
#[cfg(target_os = "linux")]
use hamlet_workspace_wire as wire;
#[cfg(target_os = "linux")]
mod worker;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    cli::run()
}

#[cfg(not(target_os = "linux"))]
#[expect(
    clippy::print_stderr,
    reason = "it says why it refuses to run"
)]
fn main() -> std::process::ExitCode {
    eprintln!("hamlet_workspace runs only on Linux.");
    std::process::ExitCode::FAILURE
}

/// Command-line roles of the Linux binary.
#[cfg(target_os = "linux")]
mod cli {
    use std::fs;

    use anyhow::{Context, Result, bail};

    use crate::{files, supervise, worker};

    const USAGE: &str = "usage: hamlet_workspace worker CONFIG";

    pub fn run() -> Result<()> {
        let args = std::env::args().collect::<Vec<_>>();
        match args.get(1).map(String::as_str) {
            Some("init") => supervise::run(&args[2..]),
            Some("file") => files::run(),
            Some("netless") => supervise::netless(&args[2..]),
            Some("worker") => {
                let path = args
                    .get(2)
                    .context("a configuration file is required")?;
                let bytes = fs::read(path)
                    .with_context(|| format!("reading {path}"))?;
                let config = serde_json::from_slice(&bytes)
                    .with_context(|| format!("parsing {path}"))?;
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?
                    .block_on(worker::run(config))
            }
            _ => bail!("{USAGE}"),
        }
    }
}
