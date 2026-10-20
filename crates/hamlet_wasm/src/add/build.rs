//! Building a plugin's source with cargo, off the frame, for the
//! chief who said to.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;

use super::check::{Make, Plan};
use crate::install::WASM;

/// The target a plugin is built for.
const TARGET: &str = "wasm32-wasip2";

/// What the chief is told where there is no cargo.
const NO_CARGO: &str = "cargo was not found. Install Rust with \
                        rustup, then run `rustup target add \
                        wasm32-wasip2`, and add the plugin again.";

/// What the chief is told where the target is not installed.
const NO_TARGET: &str = "The wasm32-wasip2 target is not installed. \
                         Run `rustup target add wasm32-wasip2`, \
                         then add the plugin again.";

/// A build that did not make the plugin: why, and the last of what
/// it wrote.
#[derive(Debug)]
pub struct Failed {
    pub why: String,
    pub log: String,
}

/// Builds the source of `plan` with `cargo build --release --target
/// wasm32-wasip2`, giving `say` each line it writes, and gives the
/// plan with the component among its files. The last `log_bytes` of
/// what it wrote are kept for a build that fails. `stop` ends it.
pub async fn build(
    mut plan: Plan,
    wasm_bytes: u64,
    log_bytes: usize,
    say: impl Fn(String),
    mut stop: oneshot::Receiver<()>,
) -> Result<Plan, Failed> {
    let failed = |why: &str, log: String| Failed {
        why: why.into(),
        log,
    };
    let Make::Build(library) = plan.make.clone() else {
        return Ok(plan);
    };
    let Some(cargo) = cargo() else {
        return Err(failed(NO_CARGO, String::new()));
    };
    let target = plan.folder.join("target");
    let child = Command::new(cargo)
        .args([
            "build",
            "--release",
            "--target",
            TARGET,
            "--target-dir",
        ])
        .arg(&target)
        .current_dir(&plan.folder)
        // The toolchain is the one the folder's own files name, as
        // for a build from a shell there.
        .env_remove("RUSTUP_TOOLCHAIN")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            return Err(failed(
                &format!("cargo could not be started: {e}"),
                String::new(),
            ));
        }
    };
    let mut log = String::new();
    if let Some(stderr) = child.stderr.take() {
        let mut read = BufReader::new(stderr).lines();
        loop {
            tokio::select! {
                line = read.next_line() => match line {
                    Ok(Some(line)) => {
                        keep(&mut log, &line, log_bytes);
                        say(line);
                    }
                    _ => break,
                },
                _ = &mut stop => {
                    let _ = child.kill().await;
                    return Err(failed("the build was stopped", log));
                }
            }
        }
    }
    let status = tokio::select! {
        status = child.wait() => status,
        _ = &mut stop => {
            let _ = child.kill().await;
            return Err(failed("the build was stopped", log));
        }
    };
    match status {
        Ok(status) if status.success() => {}
        _ if log.contains("target may not be installed") => {
            return Err(failed(NO_TARGET, log));
        }
        _ => {
            return Err(failed(
                "the build failed; what the compiler said is below",
                log,
            ));
        }
    }
    let made = target
        .join(TARGET)
        .join("release")
        .join(format!("{library}.wasm"));
    if !made.is_file() {
        return Err(failed(
            &format!("the build made no {library}.wasm"),
            log,
        ));
    }
    plan.files.insert(0, (WASM.into(), made, wasm_bytes));
    plan.make = Make::Copy;
    Ok(plan)
}

/// Adds `line` to `log`, keeping its last `most` bytes.
fn keep(log: &mut String, line: &str, most: usize) {
    log.push_str(line);
    log.push('\n');
    if log.len() > most {
        let cut = log.ceil_char_boundary(log.len() - most);
        log.drain(..cut);
    }
}

/// The cargo on the path, or the one rustup puts in the home folder,
/// for an app started with a path that holds none.
fn cargo() -> Option<PathBuf> {
    let name = format!("cargo{}", std::env::consts::EXE_SUFFIX);
    let path = std::env::var_os("PATH").unwrap_or_default();
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| Path::new(&home).join(".cargo"))
        });
    std::env::split_paths(&path)
        .map(|folder| folder.join(&name))
        .chain(cargo_home.map(|home| home.join("bin").join(&name)))
        .find(|cargo| cargo.is_file())
}
