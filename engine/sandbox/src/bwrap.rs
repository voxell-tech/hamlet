//! Linux bubblewrap sandbox.

use std::path::Path;

use anyhow::{Result, bail};
use tokio::process::Command;

use crate::{Policy, Sandbox};

/// Linux bubblewrap.
pub struct Bwrap;

impl Bwrap {
    /// Checks that bwrap exists and can create its namespaces.
    pub fn probe() -> Result<()> {
        let hint = "bubblewrap is required for sandboxing. Install \
                    it (e.g. `apt install bubblewrap`). On Ubuntu \
                    24.04+, AppArmor restricts unprivileged user \
                    namespaces; allow bwrap via an AppArmor profile \
                    or set kernel.\
                    apparmor_restrict_unprivileged_userns=0";
        let out = std::process::Command::new("bwrap")
            .args(["--unshare-all", "--ro-bind", "/", "/", "true"])
            .output()
            .map_err(|e| {
                anyhow::anyhow!("failed to run bwrap: {e}. {hint}")
            })?;
        if !out.status.success() {
            bail!(
                "bwrap probe failed ({}): {}. {hint}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }
}

impl Sandbox for Bwrap {
    fn base_env(&self) -> Vec<(String, String)> {
        vec![
            ("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()),
            ("HOME".into(), "/tmp".into()),
            ("LANG".into(), "C.UTF-8".into()),
        ]
    }

    fn name(&self) -> &'static str {
        "bwrap"
    }

    fn degraded(&self) -> bool {
        false
    }

    fn command(
        &self,
        policy: &Policy,
        argv: &[String],
        cwd: &Path,
    ) -> Command {
        let mut c = Command::new("bwrap");
        c.args(["--unshare-all"]);
        if policy.net {
            c.arg("--share-net");
        }
        c.args(["--die-with-parent", "--new-session", "--clearenv"]);
        for (k, v) in &policy.env {
            c.args(["--setenv", k, v]);
        }
        if policy.open {
            c.args(["--ro-bind", "/", "/"]);
        } else {
            c.args(["--ro-bind", "/usr", "/usr"]);
            for p in ["/bin", "/sbin", "/lib", "/lib64"] {
                c.args(["--ro-bind-try", p, p]);
            }
            let mut etc = vec![
                "alternatives",
                "ssl",
                "ca-certificates",
                "passwd",
                "group",
                "localtime",
                "ld.so.cache",
                "hosts",
            ];
            if policy.net {
                etc.push("resolv.conf");
            }
            for name in etc {
                let p = format!("/etc/{name}");
                c.args(["--ro-bind-try", &p, &p]);
            }
        }
        c.args([
            "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp",
        ]);
        if policy.open {
            // An empty folder over each. One that is missing has
            // nothing to hide and cannot be mounted over.
            for p in policy.hidden.iter().filter(|p| p.is_dir()) {
                c.arg("--tmpfs").arg(p);
            }
        }
        for p in &policy.ro {
            c.arg("--ro-bind").arg(p).arg(p);
        }
        for p in &policy.rw {
            c.arg("--bind").arg(p).arg(p);
        }
        // A mount is also what keeps the folder above from being
        // moved. A path that does not exist yet cannot be mounted,
        // so it is not guarded here.
        for p in &policy.locked {
            c.arg("--ro-bind-try").arg(p).arg(p);
        }
        c.arg("--chdir").arg(cwd);
        c.arg("--").args(argv);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{policy, s};

    fn args(c: &Command) -> Vec<String> {
        c.as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn pos(a: &[String], seq: &[&str]) -> usize {
        a.windows(seq.len())
            .position(|w| w.iter().zip(seq).all(|(x, y)| x == y))
            .unwrap_or_else(|| panic!("missing {seq:?} in {a:?}"))
    }

    #[test]
    fn bwrap_argv_shape() {
        let mut p = policy();
        p.ro = vec!["/opt/skills".into()];
        p.rw = vec!["/work".into()];
        let c =
            Bwrap.command(&p, &s(&["ls", "-l"]), Path::new("/work"));
        assert_eq!(c.as_std().get_program(), "bwrap");
        let a = args(&c);
        assert_eq!(a[0], "--unshare-all");
        assert!(!a.contains(&"--share-net".to_string()));
        assert!(!a.iter().any(|x| x.contains("resolv.conf")));
        for f in ["--die-with-parent", "--new-session", "--clearenv"]
        {
            assert!(a.contains(&f.to_string()));
        }
        pos(&a, &["--setenv", "PATH", "/usr/bin:/bin"]);
        pos(&a, &["--ro-bind", "/usr", "/usr"]);
        pos(&a, &["--ro-bind-try", "/etc/ssl", "/etc/ssl"]);
        let tmp = pos(&a, &["--tmpfs", "/tmp"]);
        assert!(
            pos(&a, &["--ro-bind", "/opt/skills", "/opt/skills"])
                > tmp
        );
        assert!(pos(&a, &["--bind", "/work", "/work"]) > tmp);
        assert!(!a.windows(3).any(|w| w == ["--ro-bind", "/", "/"]));
        let chdir = pos(&a, &["--chdir", "/work"]);
        let dd = pos(&a, &["--", "ls", "-l"]);
        assert!(chdir < dd);
        assert_eq!(&a[dd..], ["--", "ls", "-l"]);
    }

    #[test]
    fn bwrap_net_shares_and_resolves() {
        let mut p = policy();
        p.net = true;
        let a =
            args(&Bwrap.command(&p, &s(&["true"]), Path::new("/")));
        assert!(a.contains(&"--share-net".to_string()));
        pos(
            &a,
            &[
                "--ro-bind-try",
                "/etc/resolv.conf",
                "/etc/resolv.conf",
            ],
        );
    }

    #[test]
    fn an_open_policy_binds_the_machine_then_hides_and_locks() {
        let hidden = hamlet_testing::Scratch::new("bwrap-hidden");
        let mut p = policy();
        p.open = true;
        p.hidden = vec![hidden.to_path_buf(), "/no/such/home".into()];
        p.rw = vec!["/work".into()];
        p.locked = vec!["/work/.git/hooks".into()];
        let a =
            args(&Bwrap.command(&p, &s(&["true"]), Path::new("/")));
        let all = pos(&a, &["--ro-bind", "/", "/"]);
        let hide = pos(&a, &["--tmpfs", &hidden.to_string_lossy()]);
        let work = pos(&a, &["--bind", "/work", "/work"]);
        let lock = pos(
            &a,
            &[
                "--ro-bind-try",
                "/work/.git/hooks",
                "/work/.git/hooks",
            ],
        );
        // Each mount covers the ones before it.
        assert!(all < pos(&a, &["--tmpfs", "/tmp"]));
        assert!(all < hide && hide < work && work < lock);
        assert!(!a.contains(&"/no/such/home".to_string()));
    }
}
