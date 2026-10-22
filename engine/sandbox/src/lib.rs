//! Process sandboxes: a `Policy` made into a command by a `Sandbox`,
//! and `run` to execute it.

// Built on every OS so its argv tests run on any dev machine.
mod bwrap;
mod policy;
mod run;
#[cfg(target_os = "macos")]
mod seatbelt;

use std::path::Path;

use anyhow::Result;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use anyhow::bail;
use tokio::process::Command;

pub use crate::bwrap::Bwrap;
pub use crate::policy::{Limits, Policy};
pub use crate::run::{RunOutput, run};
#[cfg(target_os = "macos")]
pub use crate::seatbelt::Seatbelt;

/// A way to confine a command on one platform.
pub trait Sandbox: Send + Sync {
    fn name(&self) -> &'static str;
    /// True when enforcement is weaker than the policy asks for.
    fn degraded(&self) -> bool;
    fn command(
        &self,
        policy: &Policy,
        argv: &[String],
        cwd: &Path,
    ) -> Command;
    /// Environment tools start from.
    fn base_env(&self) -> Vec<(String, String)> {
        host_env()
    }
}

/// The host variables a normal process needs.
pub fn host_env() -> Vec<(String, String)> {
    let mut names = vec!["PATH", "HOME", "LANG"];
    if cfg!(windows) {
        names.retain(|n| *n != "HOME");
        names.extend([
            "USERPROFILE",
            "SystemRoot",
            "windir",
            "TEMP",
            "TMP",
            "PATHEXT",
            "COMSPEC",
            "APPDATA",
            "LOCALAPPDATA",
            "ProgramData",
            "ProgramFiles",
        ]);
    }
    let mut env = names
        .into_iter()
        .filter_map(|n| {
            std::env::var(n).ok().map(|v| (n.to_string(), v))
        })
        .collect::<Vec<_>>();
    if cfg!(windows) {
        // Many unix-flavoured tools (and Git Bash) look at HOME.
        if let Some((_, v)) =
            env.iter().find(|(k, _)| k == "USERPROFILE").cloned()
        {
            env.push(("HOME".into(), v));
        }
    }
    if !env.iter().any(|(k, _)| k == "LANG") {
        env.push(("LANG".into(), "C.UTF-8".into()));
    }
    env
}

/// Picks the sandbox for this OS, or `Unsandboxed` when `insecure`.
pub fn detect(insecure: bool) -> Result<&'static dyn Sandbox> {
    if insecure {
        return Ok(&Unsandboxed);
    }
    #[cfg(target_os = "linux")]
    {
        Bwrap::probe()?;
        Ok(&Bwrap)
    }
    #[cfg(target_os = "macos")]
    {
        Seatbelt::probe()?;
        Ok(&Seatbelt)
    }
    #[cfg(windows)]
    {
        bail!(
            "there is no Windows sandbox yet. Pass --insecure to \
             run tools unsandboxed, or run hamlet inside WSL"
        )
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "macos",
        windows
    )))]
    {
        bail!(
            "no sandbox is available on this platform. Pass \
             --insecure to run tools unsandboxed"
        )
    }
}

/// Dev only: no isolation, just a cleared environment and cwd.
pub struct Unsandboxed;

impl Sandbox for Unsandboxed {
    fn name(&self) -> &'static str {
        "unsandboxed"
    }

    fn degraded(&self) -> bool {
        true
    }

    fn command(
        &self,
        policy: &Policy,
        argv: &[String],
        cwd: &Path,
    ) -> Command {
        let mut c = Command::new(
            argv.first().map(String::as_str).unwrap_or(""),
        );
        c.args(argv.iter().skip(1));
        c.env_clear();
        c.envs(policy.env.iter().map(|(k, v)| (k, v)));
        c.current_dir(cwd);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    pub(super) fn policy() -> Policy {
        Policy {
            env: vec![("PATH".into(), "/usr/bin:/bin".into())],
            ..Policy::default()
        }
    }
    #[test]
    fn host_env_passes_only_listed_variables() {
        // cargo sets CARGO_MANIFEST_DIR for every test.
        assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
        let env = host_env();
        assert!(env.iter().all(|(k, _)| !k.starts_with("CARGO")));
        assert!(env.iter().any(|(k, _)| k == "PATH"));
        assert!(env.iter().any(|(k, _)| k == "LANG"));
    }
}
