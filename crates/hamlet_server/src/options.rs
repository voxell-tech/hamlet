//! What the server is started with, read from its command line.

use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use hamlet_sandbox::Isolation;

const USAGE: &str = "usage: hamlet_server [--home <dir>] [--port \
                     <n>] [--faux <file>] [--insecure]";

/// The options the server is started with.
#[derive(Clone)]
pub struct Options {
    pub home: PathBuf,
    pub port: u16,
    pub faux: Option<PathBuf>,
    pub isolation: Isolation,
}

impl Options {
    pub fn parse(
        mut args: impl Iterator<Item = OsString>,
    ) -> Result<Self> {
        let mut home = std::env::var_os("HAMLET_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from);
        let mut port = 7313;
        let mut faux = None;
        let mut isolation = Isolation::Sandboxed;
        while let Some(flag) = args.next() {
            let mut value = || args.next().context(USAGE);
            match flag.to_str() {
                Some("--insecure") => isolation = insecure()?,
                Some("--home") => home = Some(value()?.into()),
                Some("--port") => {
                    port = value()?
                        .to_str()
                        .context("port must be a number")?
                        .parse()
                        .context("port must be 0 to 65535")?;
                }
                Some("--faux") => faux = Some(value()?.into()),
                _ => bail!(USAGE),
            }
        }
        let home = home
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|p| PathBuf::from(p).join(".hamlet"))
            })
            .context("set --home or HAMLET_HOME")?;
        Ok(Self {
            home,
            port,
            faux,
            isolation,
        })
    }
}

/// `--insecure`, which only a Windows build takes (C18).
#[cfg(windows)]
fn insecure() -> Result<Isolation> {
    Ok(Isolation::Insecure)
}

/// `--insecure`, which only a Windows build takes (C18).
#[cfg(not(windows))]
fn insecure() -> Result<Isolation> {
    bail!(
        "Insecure mode is only for Windows. This platform has a \
         sandbox."
    )
}
