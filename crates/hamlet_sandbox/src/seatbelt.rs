//! macOS Seatbelt sandbox through `sandbox-exec`, with a deny-default
//! profile after the base policy of OpenAI Codex.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use sha2::{Digest, Sha256};
use tokio::process::Command;

use crate::{Policy, Sandbox};

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

const BASE: &str = r#"(version 1)
(deny default)

; children inherit the policy
(allow process-exec)
(allow process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target same-sandbox))

; stat works everywhere so path traversal and getcwd succeed
(allow file-read-metadata)

(allow sysctl-read
  (sysctl-name-prefix "hw.")
  (sysctl-name-prefix "kern.")
  (sysctl-name-prefix "vm.")
  (sysctl-name-prefix "machdep.cpu.")
  (sysctl-name-prefix "net.routetable.")
  (sysctl-name "sysctl.proc_cputype"))
(allow sysctl-write (sysctl-name "kern.grade_cputype"))

(allow iokit-open (iokit-registry-entry-class "RootDomainUserClient"))
(allow mach-lookup
  (global-name "com.apple.system.opendirectoryd.libinfo")
  (global-name "com.apple.PowerManagement.control")
  (global-name "com.apple.cfprefsd.daemon")
  (global-name "com.apple.cfprefsd.agent"))
(allow ipc-posix-sem)

; devices
(allow file-read* file-write* file-ioctl
  (literal "/dev/null")
  (literal "/dev/zero")
  (literal "/dev/dtracehelper")
  (literal "/dev/tty"))
(allow file-read* (literal "/dev/random") (literal "/dev/urandom"))
(allow pseudo-tty)
(allow file-read* file-write* file-ioctl (literal "/dev/ptmx"))
(allow file-read* file-write* file-ioctl (regex #"^/dev/ttys[0-9]+"))

; system locations needed to run ordinary binaries
(allow file-read*
  (subpath "/usr")
  (subpath "/bin")
  (subpath "/sbin")
  (subpath "/System")
  (subpath "/Library/Apple")
  (subpath "/Library/Developer")
  (subpath "/Library/Preferences/Logging")
  (literal "/Library/Preferences/com.apple.dt.Xcode.plist")
  (subpath "/opt/homebrew")
  (subpath "/private/etc")
  (subpath "/private/var/db/timezone")
  (subpath "/private/var/db/dyld")
  (subpath "/private/var/db/xcode_select_link")
  (subpath "/Applications/Xcode.app/Contents")
  (literal "/")
  (literal "/dev"))
"#;

const NET: &str = r#"
(allow network*)
(allow system-socket)
(allow mach-lookup
  (global-name "com.apple.dnssd.service")
  (global-name "com.apple.SystemConfiguration.configd")
  (global-name "com.apple.SystemConfiguration.DNSConfiguration")
  (global-name "com.apple.SystemConfiguration.helper")
  (global-name "com.apple.networkd")
  (global-name "com.apple.nsurlsessiond")
  (global-name "com.apple.trustd")
  (global-name "com.apple.trustd.agent")
  (global-name "com.apple.securityd")
  (global-name "com.apple.SecurityServer")
  (global-name "com.apple.ocspd"))
(allow file-read*
  (subpath "/private/var/db/mds")
  (subpath "/private/var/db/DetachedSignatures")
  (subpath "/Library/Keychains")
  (subpath "/private/var/protected/trustd"))
"#;

/// macOS `sandbox-exec`.
pub struct Seatbelt;

/// The real path of `p`. The part of it that does not exist yet is
/// kept as written, after the real path of the part that does.
fn canon(p: &Path) -> PathBuf {
    let Some(known) = p.ancestors().find(|a| a.exists()) else {
        return p.to_path_buf();
    };
    let rest = p.strip_prefix(known).unwrap_or(Path::new(""));
    match known.canonicalize() {
        Ok(real) if rest.as_os_str().is_empty() => real,
        Ok(real) => real.join(rest),
        Err(_) => p.to_path_buf(),
    }
}

/// The folder that holds every private temp dir.
fn temp_root() -> PathBuf {
    canon(&std::env::temp_dir()).join("hamlet-sandbox-tmp")
}

/// The private temp dir the sandboxed process may write to: one for
/// each set of writable paths, so each house has its own.
fn private_tmp(policy: &Policy) -> PathBuf {
    let mut digest = Sha256::new();
    for path in &policy.rw {
        digest.update(canon(path).as_os_str().as_encoded_bytes());
        digest.update([0]);
    }
    let name = digest
        .finalize()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let dir = temp_root().join(name);
    if std::fs::create_dir_all(&dir).is_ok() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            &dir,
            std::fs::Permissions::from_mode(0o700),
        );
    }
    dir
}

/// Adds `rule` once for each of `paths` as written and once as it
/// really is, with `{}` in it standing for the parameter's name.
fn rules(
    (sbpl, params): &mut (String, Vec<(String, String)>),
    kind: &str,
    rule: &str,
    paths: &[PathBuf],
) {
    for (i, p) in paths.iter().enumerate() {
        for (suffix, path) in [("", p.clone()), ("_REAL", canon(p))] {
            let name = format!("{kind}{i}{suffix}");
            sbpl.push_str(&rule.replace("{}", &name));
            sbpl.push('\n');
            params.push((name, path.to_string_lossy().into_owned()));
        }
    }
}

/// The profile text and the `-D` parameters it reads, which keep
/// paths from needing escapes. Of two rules that match, the later one
/// decides.
fn profile(
    policy: &Policy,
    tmp: &Path,
) -> (String, Vec<(String, String)>) {
    let mut out = (String::from(BASE), Vec::new());
    if policy.open {
        out.0.push_str("(allow file-read*)\n");
        let mut hidden = policy.hidden.clone();
        // Another house's temp dir is as private as the house.
        hidden.push(temp_root());
        // The base profile allows metadata by its own name, which
        // this does not undo: a path through a hidden folder to a
        // granted one can still be walked.
        rules(
            &mut out,
            "HIDDEN",
            r#"(deny file-read* file-write* (subpath (param "{}")))"#,
            &hidden,
        );
    }
    rules(
        &mut out,
        "TMP",
        r#"(allow file-read* file-write* (subpath (param "{}")))"#,
        &[tmp.to_path_buf()],
    );
    rules(
        &mut out,
        "RO",
        r#"(allow file-read* (subpath (param "{}")))"#,
        &policy.ro,
    );
    rules(
        &mut out,
        "RW",
        r#"(allow file-read* file-write* (subpath (param "{}")))"#,
        &policy.rw,
    );
    rules(
        &mut out,
        "LOCKED",
        r#"(deny file-write* (subpath (param "{}")))"#,
        &policy.locked,
    );
    // Moving a folder a locked path lies in would carry it out from
    // under the lock.
    let mut above = policy
        .locked
        .iter()
        .flat_map(|locked| locked.ancestors().skip(1))
        .filter(|a| {
            policy.rw.iter().any(|rw| a.starts_with(rw) && a != rw)
        })
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    above.sort();
    above.dedup();
    rules(
        &mut out,
        "ABOVE",
        r#"(deny file-write-unlink (literal (param "{}")))"#,
        &above,
    );
    if policy.net {
        out.0.push_str(NET);
    }
    out
}

impl Seatbelt {
    /// Checks that `sandbox-exec` exists and can run a trivial
    /// command.
    pub fn probe() -> Result<()> {
        let hint = "macOS sandboxing needs /usr/bin/sandbox-exec. \
                    Pass --insecure to run tools unsandboxed";
        let out = std::process::Command::new(SANDBOX_EXEC)
            .args([
                "-p",
                "(version 1)(allow default)",
                "--",
                "/usr/bin/true",
            ])
            .output()
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to run {SANDBOX_EXEC}: {e}. {hint}"
                )
            })?;
        if !out.status.success() {
            bail!(
                "sandbox-exec probe failed ({}): {}. {hint}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }
}

impl Sandbox for Seatbelt {
    fn name(&self) -> &'static str {
        "seatbelt"
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
        let tmp = private_tmp(policy);
        let (sbpl, params) = profile(policy, &tmp);
        let mut c = Command::new(SANDBOX_EXEC);
        for (k, v) in &params {
            c.arg("-D").arg(format!("{k}={v}"));
        }
        c.arg("-p").arg(sbpl).arg("--").args(argv);
        c.env_clear();
        c.envs(
            policy
                .env
                .iter()
                .filter(|(k, _)| k != "TMPDIR")
                .map(|(k, v)| (k, v)),
        );
        c.env("TMPDIR", &tmp);
        c.current_dir(cwd);
        c
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::tests::s;
    use crate::{RunOutput, run};

    fn policy(rw: &Path) -> Policy {
        let mut env = crate::host_env();
        env.retain(|(k, _)| k != "HOME");
        env.push(("HOME".into(), rw.to_string_lossy().into_owned()));
        Policy {
            rw: vec![rw.to_path_buf()],
            env,
            ..Policy::default()
        }
    }

    fn scratch(tag: &str) -> Scratch {
        Scratch::new(&format!("sb-{tag}"))
    }

    async fn sh(p: &Policy, cwd: &Path, script: &str) -> RunOutput {
        run(&Seatbelt, p, &s(&["bash", "-c", script]), cwd, None)
            .await
            .unwrap()
    }

    #[test]
    fn profile_uses_params_not_paths() {
        let mut p = policy(Path::new("/work"));
        p.ro = vec!["/odd \"path\"".into()];
        let (sbpl, params) = profile(&p, Path::new("/tmp/x"));
        assert!(!sbpl.contains("odd"));
        assert!(sbpl.contains("(deny default)"));
        assert!(!sbpl.contains("(allow network*)"));
        assert!(
            params
                .iter()
                .any(|(k, v)| k == "RO0" && v == "/odd \"path\"")
        );
        p.net = true;
        assert!(
            profile(&p, Path::new("/tmp/x"))
                .0
                .contains("(allow network*)")
        );
    }

    #[test]
    fn an_open_profile_hides_then_grants_then_locks() {
        let mut p = policy(Path::new("/work"));
        p.open = true;
        p.hidden = vec!["/home".into()];
        p.locked = vec!["/work/.git/hooks".into()];
        let (sbpl, params) = profile(&p, Path::new("/tmp/x"));
        let at = |rule: &str| {
            sbpl.find(rule)
                .unwrap_or_else(|| panic!("{rule}\n{sbpl}"))
        };
        // The later rule decides, so the order is the policy.
        let all = at("(allow file-read*)\n");
        let hide = at(
            r#"(deny file-read* file-write* (subpath (param "HIDDEN0")))"#,
        );
        let grant = at(
            r#"(allow file-read* file-write* (subpath (param "RW0")))"#,
        );
        let lock =
            at(r#"(deny file-write* (subpath (param "LOCKED0")))"#);
        let above = at(
            r#"(deny file-write-unlink (literal (param "ABOVE0")))"#,
        );
        assert!(all < hide && hide < grant && grant < lock);
        assert!(lock < above);
        let value = |name: &str| {
            params
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(value("ABOVE0"), Some("/work/.git"));
        assert_eq!(value("ABOVE1"), None);
        // Every house's temp dir is hidden with the rest.
        assert!(value("HIDDEN1").is_some());
        let closed =
            profile(&policy(Path::new("/work")), Path::new("/t"));
        assert!(!closed.0.contains("(allow file-read*)\n"));
        assert!(!closed.0.contains("deny file-"));
    }

    #[tokio::test]
    async fn writes_only_inside_rw() {
        let d = scratch("write");
        let p = policy(&d);
        let r = sh(&p, &d, "echo a > in.txt && cat in.txt").await;
        assert_eq!(r.output, "a\n", "{}", r.output);
        let outside = canon(&std::env::temp_dir()).join(format!(
            "hamlet-sb-outside-{}",
            std::process::id()
        ));
        let r =
            sh(&p, &d, &format!("echo a > {}", outside.display()))
                .await;
        assert_ne!(r.exit_code, 0);
        assert!(!outside.exists());
        let r = sh(
            &p,
            &d,
            "echo t > \"$TMPDIR/t.txt\" && cat \"$TMPDIR/t.txt\"",
        )
        .await;
        assert_eq!(r.output, "t\n", "{}", r.output);
    }

    #[tokio::test]
    async fn home_is_unreadable_unless_granted() {
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let d = scratch("home");
        // A file under $HOME that exists on any account.
        let probe = home.join(".hamlet-sb-probe");
        std::fs::write(&probe, "secret").unwrap();
        let mut p = policy(&d);
        let cat = format!("cat {}", probe.display());
        let denied = sh(&p, &d, &cat).await;
        let keys =
            sh(&p, &d, "ls ~/.ssh 2>&1; cat ~/.ssh/id_* 2>&1; true")
                .await;
        p.ro.push(probe.clone());
        let granted = sh(&p, &d, &cat).await;
        // Removed before anything can fail, so no run leaves it
        // behind.
        let _ = std::fs::remove_file(&probe);
        assert_ne!(denied.exit_code, 0, "{}", denied.output);
        assert!(!denied.output.contains("secret"));
        assert!(!keys.output.contains("PRIVATE KEY"));
        assert_eq!(granted.output, "secret", "{}", granted.output);
    }

    #[tokio::test]
    async fn network_denied_by_default() {
        let d = scratch("net0");
        // A listener on this machine: nothing leaves it either way.
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let r = sh(
            &policy(&d),
            &d,
            &format!("curl -sS -m 5 http://127.0.0.1:{port}/ 2>&1"),
        )
        .await;
        assert_ne!(r.exit_code, 0, "{}", r.output);
        assert!(listener.accept().is_err(), "a connection got out");
    }

    /// The same request with the network granted, so that the test
    /// above fails for the sandbox and not for a broken command.
    #[tokio::test]
    async fn local_network_allowed_when_granted() {
        let d = scratch("net2");
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut p = policy(&d);
        p.net = true;
        // Nothing answers, so curl times out; reaching the listener
        // is what counts.
        sh(
            &p,
            &d,
            &format!("curl -sS -m 2 http://127.0.0.1:{port}/ 2>&1"),
        )
        .await;
        assert!(listener.accept().is_ok(), "no connection arrived");
    }

    #[tokio::test]
    #[cfg(feature = "live-tests")]
    async fn network_allowed_when_granted() {
        let d = scratch("net1");
        let mut p = policy(&d);
        p.net = true;
        let r = sh(&p, &d, "curl -sS -m 15 https://example.com 2>&1")
            .await;
        assert_eq!(r.exit_code, 0, "{}", r.output);
        assert!(r.output.contains("Example Domain"));
    }

    #[tokio::test]
    async fn common_tools_run() {
        Seatbelt::probe().unwrap();
        let d = scratch("tools");
        let p = policy(&d);
        for script in [
            "ls .",
            "cat /etc/hosts >/dev/null",
            "find . -name x",
            "git init -q . && git status --short && git --version",
            "python3 -c 'print(1+1)'",
            "/usr/bin/python3 -c 'print(1+1)'",
        ] {
            let r = sh(&p, &d, script).await;
            assert_eq!(r.exit_code, 0, "{script}: {}", r.output);
        }
    }

    #[tokio::test]
    async fn an_open_policy_reads_widely_and_keeps_its_locks() {
        let (project, home, outside) = (
            scratch("open"),
            scratch("open-home"),
            scratch("open-out"),
        );
        let house = home.join("house");
        std::fs::create_dir_all(&house).unwrap();
        std::fs::write(home.join("api_key.x"), "secret").unwrap();
        std::fs::write(outside.join("seen.txt"), "seen").unwrap();
        std::fs::create_dir_all(project.join(".git/hooks")).unwrap();
        let mut p = policy(&project);
        p.rw.push(house.clone());
        p.open = true;
        p.hidden = vec![home.to_path_buf()];
        p.locked = vec![
            project.join(".git/hooks"),
            project.join(".git/config"),
        ];
        let (home, outside) = (home.display(), outside.display());
        let passes = [
            format!("cat {outside}/seen.txt"),
            "echo a > in.txt".into(),
            "echo a > .git/HEAD".into(),
            format!("echo a > {home}/house/n && cat {home}/house/n"),
        ];
        for script in passes {
            let r = sh(&p, &project, &script).await;
            assert_eq!(r.exit_code, 0, "{script}: {}", r.output);
        }
        let fails = [
            format!("echo a > {outside}/new.txt"),
            format!("cat {home}/api_key.x"),
            format!("ls {home}"),
            "echo a > .git/hooks/pre-commit".into(),
            "echo a > .git/config".into(),
            "mv .git .git2".into(),
            "ln -s .git/hooks h && echo a > h/pre-commit".into(),
        ];
        for script in fails {
            let r = sh(&p, &project, &script).await;
            assert_ne!(r.exit_code, 0, "{script}: {}", r.output);
            assert!(!r.output.contains("secret"), "{script}");
        }
        assert!(!project.join(".git/hooks/pre-commit").exists());
        let _ = std::fs::remove_dir_all(private_tmp(&p));
    }

    #[tokio::test]
    async fn each_house_has_its_own_temp_folder() {
        let (a, b) = (scratch("tmp-a"), scratch("tmp-b"));
        let (pa, pb) = (policy(&a), policy(&b));
        assert_ne!(private_tmp(&pa), private_tmp(&pb));
        assert_eq!(private_tmp(&pa), private_tmp(&pa));

        let left =
            sh(&pa, &a, "echo secret > \"$TMPDIR/note\"").await;
        assert_eq!(left.exit_code, 0, "{}", left.output);
        let theirs = private_tmp(&pa).join("note");
        assert!(theirs.exists());
        let read =
            sh(&pb, &b, &format!("cat '{}' 2>&1", theirs.display()))
                .await;
        assert_ne!(read.exit_code, 0, "{}", read.output);
        let _ = std::fs::remove_dir_all(private_tmp(&pa));
        let _ = std::fs::remove_dir_all(private_tmp(&pb));
    }
}
