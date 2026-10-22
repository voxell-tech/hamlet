//! Linux bubblewrap.

use std::collections::BTreeSet;
use std::path::Path;

use super::Confined;
use super::path::text;
use super::policy::{
    Policy, above_roots, fixed_roots, is_dir, temp_root, within_text,
    write_roots,
};

pub(super) const PROBE: [&str; 6] =
    ["bwrap", "--unshare-all", "--ro-bind", "/", "/", "true"];

pub(super) const HINT: &str =
    "bubblewrap is required for sandboxing. Install it (e.g. `apt \
     install bubblewrap`). On Ubuntu 24.04+, AppArmor restricts \
     unprivileged user namespaces; allow bwrap via an AppArmor \
     profile or set kernel.apparmor_restrict_unprivileged_userns=0";

/// `argv`, run from `cwd`, confined to `policy`, with `tmp` as its
/// temp folder.
pub(super) fn command(
    policy: &Policy,
    argv: &[String],
    cwd: &str,
    tmp: &str,
) -> Confined {
    let mut args = Vec::<String>::new();
    let mut push = |parts: &[&str]| {
        args.extend(parts.iter().map(ToString::to_string));
    };
    push(&["bwrap", "--unshare-all"]);
    if policy.net {
        push(&["--share-net"]);
    }
    push(&["--die-with-parent", "--new-session"]);
    if policy.open {
        push(&["--ro-bind", "/", "/"]);
    } else {
        push(&["--ro-bind", "/usr", "/usr"]);
        for path in ["/bin", "/sbin", "/lib", "/lib64"] {
            push(&["--ro-bind-try", path, path]);
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
            let path = format!("/etc/{name}");
            push(&["--ro-bind-try", &path, &path]);
        }
    }
    push(&["--proc", "/proc", "--dev", "/dev"]);
    if !policy.open {
        push(&["--tmpfs", "/tmp"]);
    }
    if policy.open {
        // An empty folder over each. One that is missing has nothing
        // to hide and cannot be mounted over.
        let mut hidden =
            policy.hidden.iter().map(|p| text(p)).collect::<Vec<_>>();
        hidden.push(temp_root());
        for path in hidden.iter().filter(|path| is_dir(path)) {
            push(&["--tmpfs", path]);
        }
    }
    for path in &policy.ro {
        let path = text(path);
        push(&["--ro-bind", &path, &path]);
    }
    let writes = write_roots(policy, tmp);
    for path in &writes {
        push(&["--bind", path, path]);
    }
    // Mount points keep roots and their writable ancestors in place.
    let mut seen = BTreeSet::new();
    let mut points = above_roots(policy, tmp);
    points.extend(fixed_roots(policy, tmp));
    points.retain(|path| seen.insert(path.clone()));
    points.sort_by_key(String::len);
    for path in &points {
        if Path::new(path).exists()
            && writes
                .iter()
                .any(|rw| path != rw && within_text(path, rw))
        {
            push(&["--bind", path, path]);
        }
    }
    for path in &policy.locked {
        let path = text(path);
        push(&["--ro-bind-try", &path, &path]);
    }
    push(&["--chdir", cwd, "--"]);
    args.extend(argv.iter().cloned());
    let mut env = policy.env.clone();
    env.insert("TMPDIR".into(), tmp.to_owned());
    Confined { argv: args, env }
}
