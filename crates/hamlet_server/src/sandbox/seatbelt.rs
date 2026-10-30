//! macOS Seatbelt, through `sandbox-exec`.

use std::collections::BTreeSet;

use hamlet_os::fs::path::{self, text};

use super::Confined;
use super::policy::{
    Policy, above_roots, canon_text, fixed_roots, temp_root,
};

pub(super) const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

pub(super) const PROBE: [&str; 5] = [
    SANDBOX_EXEC,
    "-p",
    "(version 1)(allow default)",
    "--",
    "/usr/bin/true",
];

pub(super) const HINT: &str =
    "macOS sandboxing needs /usr/bin/sandbox-exec";

// A deny-default profile after the base policy of OpenAI Codex.
const BASE: &str = r#"(version 1)
(deny default)

; system aliases, shell selection, and the realpath the /usr/bin
; developer shims such as python3 take of Xcode
(allow file-read-metadata
  (literal "/etc") (literal "/tmp") (literal "/var")
  (literal "/opt") (literal "/Library") (literal "/Applications")
  (literal "/Applications/Xcode.app")
  (literal "/private") (literal "/private/var")
  (subpath "/private/var/select"))

; children inherit the policy
(allow process-exec)
(allow process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target same-sandbox))

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

/// The profile text and the `-D` parameters it reads, which keep
/// paths from needing escapes.
#[derive(Default)]
struct Profile {
    sbpl: String,
    params: Vec<(String, String)>,
}

impl Profile {
    /// Adds `rule` once for each of `paths` as written and once as it
    /// really is, with `{}` standing for the parameter's name.
    fn rules(&mut self, kind: &str, rule: &str, paths: &[String]) {
        for (i, path) in paths.iter().enumerate() {
            for (suffix, value) in
                [("", path.clone()), ("_REAL", canon_text(path))]
            {
                let name = format!("{kind}{i}{suffix}");
                self.sbpl.push_str(&rule.replacen("{}", &name, 1));
                self.sbpl.push('\n');
                self.params.push((name, value));
            }
        }
    }
}

/// Of two rules that match, the later one decides.
fn profile(policy: &Policy, tmp: &str) -> Profile {
    let mut out = Profile {
        sbpl: BASE.into(),
        ..Profile::default()
    };
    let texts = |list: &[std::path::PathBuf]| {
        list.iter().map(|p| text(p)).collect::<Vec<_>>()
    };
    let mut parents = Vec::<String>::new();
    let mut seen = BTreeSet::new();
    let mut granted = texts(&policy.ro);
    granted.extend(texts(&policy.rw));
    granted.push(tmp.to_owned());
    for each in &granted {
        for spelling in [each.clone(), canon_text(each)] {
            let mut up = path::dirname(&spelling);
            while up != path::dirname(&up) {
                if seen.insert(up.clone()) {
                    parents.push(up.clone());
                }
                up = path::dirname(&up);
            }
        }
    }
    out.rules(
        "PARENTS",
        r#"(allow file-read-metadata (literal (param "{}")))"#,
        &parents,
    );
    if policy.open {
        out.sbpl.push_str("(allow file-read*)\n");
        // Another house's temp dir is as private as the house.
        let mut hidden = texts(&policy.hidden);
        hidden.push(temp_root());
        out.rules(
            "HIDDEN",
            r#"(deny file-read* file-write* (subpath (param "{}")))"#,
            &hidden,
        );
    }
    out.rules(
        "TMP",
        r#"(allow file-read* file-write* (subpath (param "{}")))"#,
        &[tmp.to_owned()],
    );
    out.rules(
        "RO",
        r#"(allow file-read* (subpath (param "{}")))"#,
        &texts(&policy.ro),
    );
    out.rules(
        "RW",
        r#"(allow file-read* file-write* (subpath (param "{}")))"#,
        &texts(&policy.rw),
    );
    out.rules(
        "ROOTS",
        r#"(deny file-write-unlink file-write-create (literal (param "{}")))"#,
        &fixed_roots(policy, tmp),
    );
    out.rules(
        "LOCKED",
        r#"(deny file-write* (subpath (param "{}")))"#,
        &texts(&policy.locked),
    );
    out.rules(
        "ABOVE",
        r#"(deny file-write-unlink (literal (param "{}")))"#,
        &above_roots(policy, tmp),
    );
    if policy.net {
        out.sbpl.push_str(NET);
    }
    out
}

/// `argv` confined to `policy`, with `tmp` as its temp folder.
pub(super) fn command(
    policy: &Policy,
    argv: &[String],
    tmp: &str,
) -> Confined {
    let Profile { sbpl, params } = profile(policy, tmp);
    let mut args = vec![SANDBOX_EXEC.to_owned()];
    for (name, value) in params {
        args.push("-D".into());
        args.push(format!("{name}={value}"));
    }
    args.extend(["-p".into(), sbpl, "--".into()]);
    args.extend(argv.iter().cloned());
    let mut env = policy.env.clone();
    env.insert("TMPDIR".into(), tmp.to_owned());
    Confined { argv: args, env }
}
