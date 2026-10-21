//! Commands and file operations confined to a policy (S12, C18).

mod bwrap;
mod env;
pub(crate) mod path;
mod policy;
mod reach;
mod seatbelt;

use std::collections::BTreeMap;

use anyhow::bail;

pub(crate) use self::env::{
    Env, ExecError, FileError, FileInfo, FileKind,
};
pub use self::policy::{Limits, Policy, canon, policy_hash, within};
pub use self::reach::{closed_policy, open_policy};

/// A command confined to a policy: the program to run and its whole
/// environment.
#[derive(Debug)]
pub(crate) struct Confined {
    pub(crate) argv: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
}

/// A way to confine a command on one platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sandbox(Kind);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Bwrap,
    Seatbelt,
    /// A platform without one, where tools cannot run.
    Missing,
}

impl Sandbox {
    /// The sandbox of this machine's platform (C18).
    pub fn platform() -> Self {
        Self(if cfg!(target_os = "macos") {
            Kind::Seatbelt
        } else if cfg!(target_os = "linux") {
            Kind::Bwrap
        } else {
            Kind::Missing
        })
    }

    /// The name a policy is hashed with.
    pub fn name(&self) -> &str {
        match self.0 {
            Kind::Bwrap => "bwrap",
            Kind::Seatbelt => "seatbelt",
            Kind::Missing => std::env::consts::OS,
        }
    }

    /// Checks that the sandbox works on this machine, and fails with
    /// what it needs where it does not (C18).
    pub fn probe(&self) -> anyhow::Result<()> {
        let (argv, hint) = match self.0 {
            Kind::Bwrap => (&bwrap::PROBE[..], bwrap::HINT),
            Kind::Seatbelt => (&seatbelt::PROBE[..], seatbelt::HINT),
            Kind::Missing => bail!(missing()),
        };
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

    /// The environment tools start from.
    pub(crate) fn base_env(&self) -> BTreeMap<String, String> {
        match self.0 {
            Kind::Bwrap => BTreeMap::from([
                (
                    "PATH".into(),
                    "/usr/local/bin:/usr/bin:/bin".into(),
                ),
                ("HOME".into(), "/tmp".into()),
                ("LANG".into(), "C.UTF-8".into()),
            ]),
            Kind::Seatbelt | Kind::Missing => policy::host_env(),
        }
    }

    /// `argv`, run from `cwd`, confined to `policy`, with `tmp` as
    /// its temp folder.
    pub(crate) fn command(
        &self,
        policy: &Policy,
        argv: &[String],
        cwd: &str,
        tmp: &str,
    ) -> Result<Confined, String> {
        match self.0 {
            Kind::Bwrap => Ok(bwrap::command(policy, argv, cwd, tmp)),
            Kind::Seatbelt => {
                Ok(seatbelt::command(policy, argv, tmp))
            }
            Kind::Missing => Err(missing()),
        }
    }
}

fn missing() -> String {
    format!(
        "There is no sandbox for {}, so tools cannot run here.",
        std::env::consts::OS
    )
}

/// `argv`, run by bash once `limits` hold for it and for every
/// process it starts.
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
