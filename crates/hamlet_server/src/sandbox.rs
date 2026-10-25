//! Commands and file operations confined to a policy (S12, C18).

mod bwrap;
mod env;
mod policy;
mod reach;
mod seatbelt;

use std::collections::BTreeMap;

use anyhow::bail;
use hamlet_protocol::client::Confinement;

pub(crate) use self::env::{
    AgentLeftovers, AllLeftovers, Env, ExecError, FileError,
    FileInfo, FileKind, Launch, Ran,
};
pub(crate) use self::policy::{LimitError, SetupError};
pub use self::policy::{Policy, is_within};
pub use self::reach::{closed_policy, open_policy};

/// A command confined to a policy: the program to run and its whole
/// environment.
#[derive(Debug)]
pub(crate) struct Confined {
    pub(crate) argv: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
}

/// What the chief asked for when the server started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Isolation {
    Sandboxed,
    /// Commands run without a sandbox. Only a Windows build has it,
    /// so no Linux or macOS server can run unconfined (C18).
    #[cfg(windows)]
    Insecure,
}

impl Isolation {
    /// The isolation tests run their commands in: insecure mode on
    /// Windows.
    #[cfg(all(test, windows))]
    pub(crate) const TESTING: Self = Self::Insecure;
    #[cfg(all(test, not(windows)))]
    pub(crate) const TESTING: Self = Self::Sandboxed;
}

/// A way to confine a command on one platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sandbox(Kind);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Bwrap,
    Seatbelt,
    /// Insecure mode: commands run as the server's account does.
    #[cfg(windows)]
    Insecure,
}

impl Sandbox {
    /// The sandbox `isolation` asks for on this platform, refused on
    /// a platform without one (C18).
    pub fn open(isolation: Isolation) -> Result<Self, NoSandbox> {
        match isolation {
            Isolation::Sandboxed if cfg!(target_os = "macos") => {
                Ok(Self(Kind::Seatbelt))
            }
            Isolation::Sandboxed if cfg!(target_os = "linux") => {
                Ok(Self(Kind::Bwrap))
            }
            Isolation::Sandboxed => Err(NoSandbox),
            #[cfg(windows)]
            Isolation::Insecure => Ok(Self(Kind::Insecure)),
        }
    }

    #[cfg(test)]
    pub(crate) fn testing() -> Self {
        Self::open(Isolation::TESTING).expect("a sandbox for tests")
    }

    /// The name a policy is hashed with.
    pub fn name(&self) -> &str {
        match self.0 {
            Kind::Bwrap => "bwrap",
            Kind::Seatbelt => "seatbelt",
            #[cfg(windows)]
            Kind::Insecure => "insecure",
        }
    }

    /// How it confines commands, as a client is told (C18).
    pub fn confinement(&self) -> Confinement {
        match self.0 {
            Kind::Bwrap | Kind::Seatbelt => Confinement::Sandbox {
                name: self.name().into(),
            },
            #[cfg(windows)]
            Kind::Insecure => Confinement::Insecure,
        }
    }

    /// Checks that the sandbox works on this machine, and fails with
    /// what it needs where it does not (C18).
    pub fn probe(&self) -> anyhow::Result<()> {
        let (argv, hint) = match self.0 {
            Kind::Bwrap => (&bwrap::PROBE[..], bwrap::HINT),
            Kind::Seatbelt => (&seatbelt::PROBE[..], seatbelt::HINT),
            #[cfg(windows)]
            Kind::Insecure => return Ok(()),
        };
        #[expect(
            clippy::disallowed_methods,
            reason = "the probe runs the sandbox itself, outside \
                      any tree"
        )]
        let ran = std::process::Command::new(argv[0])
            .args(&argv[1..])
            .stdin(std::process::Stdio::null())
            .output();
        let ran = match ran {
            Ok(ran) => ran,
            Err(error) => bail!(
                "failed to run {}: {}. {hint}",
                argv[0],
                env::errno_name(&error)
            ),
        };
        if !ran.status.success() {
            let status = env::status_word(&ran.status);
            bail!(
                "{} probe failed ({status}): {}. {hint}",
                self.name(),
                String::from_utf8_lossy(&ran.stderr).trim()
            );
        }
        Ok(())
    }

    /// `argv`, run from `cwd`, confined to `policy`, with `tmp` as
    /// its temp folder.
    pub(crate) fn command(
        &self,
        policy: &Policy,
        argv: &[String],
        cwd: &str,
        tmp: &str,
    ) -> Confined {
        match self.0 {
            Kind::Bwrap => bwrap::command(policy, argv, cwd, tmp),
            Kind::Seatbelt => seatbelt::command(policy, argv, tmp),
            #[cfg(windows)]
            Kind::Insecure => {
                let mut env = policy.env.clone();
                for name in ["TEMP", "TMP"] {
                    env.insert(name.into(), tmp.to_owned());
                }
                Confined {
                    argv: argv.to_vec(),
                    env,
                }
            }
        }
    }
}

/// The environment tools start from on this platform.
pub(crate) fn base_env() -> BTreeMap<String, String> {
    if cfg!(target_os = "linux") {
        BTreeMap::from([
            ("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()),
            ("HOME".into(), "/tmp".into()),
            ("LANG".into(), "C.UTF-8".into()),
        ])
    } else {
        policy::host_env()
    }
}

/// This platform has no sandbox.
#[derive(Debug, thiserror::Error)]
#[cfg_attr(
    windows,
    error(
        "Windows has no sandbox for agents' commands yet. Start the \
         server with --insecure to run them without one, with this \
         account's full reach."
    )
)]
#[cfg_attr(
    not(windows),
    error(
        "There is no sandbox for {os}, so tools cannot run here.",
        os = std::env::consts::OS
    )
)]
pub struct NoSandbox;

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn sandboxed_is_refused_on_windows() {
        let refused =
            Sandbox::open(Isolation::Sandboxed).unwrap_err();
        assert!(
            refused.to_string().contains("--insecure"),
            "{refused}"
        );
    }
}
