//! The workspace manager, actor worker, and confined operation
//! supervisor.
//!
//! Every role relies on Linux namespaces, bubblewrap and `flock`, so
//! the binary is built only for Linux. Elsewhere it compiles to an
//! entry point that refuses to run, which keeps the Cargo workspace
//! building on the chief's own platform.

#[cfg(target_os = "linux")]
mod control;
#[cfg(target_os = "linux")]
mod files;
#[cfg(target_os = "linux")]
mod http;
#[cfg(target_os = "linux")]
mod manager;
#[cfg(target_os = "linux")]
mod proxy;
#[cfg(target_os = "linux")]
mod supervise;
#[cfg(target_os = "linux")]
mod wire;
#[cfg(target_os = "linux")]
mod worker;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    cli::run()
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("hamlet-workspace runs only on Linux.");
    std::process::ExitCode::FAILURE
}

#[cfg(target_os = "linux")]
fn signing_key(
    path: &std::path::Path,
) -> anyhow::Result<ring::signature::Ed25519KeyPair> {
    ring::signature::Ed25519KeyPair::from_pkcs8(&std::fs::read(path)?)
        .map_err(|_| anyhow::anyhow!("invalid workspace signing key"))
}

/// Command-line roles of the Linux binary.
#[cfg(target_os = "linux")]
mod cli {
    use std::fs;
    use std::future::Future;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::Path;

    use anyhow::{Context, Result, bail};
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    use crate::{files, manager, signing_key, supervise, worker};

    const USAGE: &str = "usage: hamlet-workspace manager|worker \
                         CONFIG; delete CONFIG WORKSPACE \
                         EXPECTED_UID; keygen NEW_KEY_FILE";

    pub fn run() -> Result<()> {
        let args = std::env::args().collect::<Vec<_>>();
        match args.get(1).map(String::as_str) {
            Some("init") => supervise::run(&args[2..]),
            Some("file") => files::run(),
            Some("netless") => supervise::netless(&args[2..]),
            Some("keygen") => {
                keygen(args.get(2).context(
                    "keygen requires a new private-key path",
                )?)
            }
            Some("manager") => {
                let config = configuration(&args)?;
                block_on(manager::run(config))
            }
            Some("worker") => {
                let config = configuration(&args)?;
                block_on(worker::run(config))
            }
            Some("delete") => {
                let [_, _, _, workspace, expected_uid] =
                    args.as_slice()
                else {
                    bail!("{USAGE}");
                };
                let config =
                    configuration::<manager::Configuration>(&args)?;
                block_on(manager::delete(
                    &config,
                    workspace,
                    expected_uid,
                ))?;
                println!(
                    "Deleted the actor of workspace {workspace}."
                );
                Ok(())
            }
            _ => bail!("{USAGE}"),
        }
    }

    /// Reads the JSON configuration named by the second argument.
    fn configuration<T: serde::de::DeserializeOwned>(
        args: &[String],
    ) -> Result<T> {
        let path = args
            .get(2)
            .context("a configuration file is required")?;
        let bytes = fs::read(path)
            .with_context(|| format!("reading {path}"))?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing {path}"))
    }

    fn block_on<T>(
        future: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(future)
    }

    fn keygen(path: &str) -> Result<()> {
        let key =
            Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
                .map_err(|_| {
                    anyhow::anyhow!("generating signing key")
                })?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(key.as_ref())?;
        file.sync_all()?;
        let pair = signing_key(Path::new(path))?;
        println!("{}", STANDARD.encode(pair.public_key().as_ref()));
        Ok(())
    }
}
