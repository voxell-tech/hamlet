//! Linux bubblewrap.

use std::collections::BTreeSet;
use std::path::Path;

use hamlet_os::fs::path::text;

use super::Confined;
use super::policy::{
    Grant, Layer, Policy, above_roots, canon_text, depth,
    fixed_roots, is_dir, is_granted_writes, is_readable, layers,
    write_roots,
};

/// Where Homebrew installs on Linux.
pub(super) const LINUXBREW: &str = "/home/linuxbrew/.linuxbrew";

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
        // The system's programs, and Homebrew's Linux prefix as
        // Seatbelt reads /opt/homebrew.
        for path in ["/bin", "/sbin", "/lib", "/lib64", LINUXBREW] {
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
    // Each mount at its depth, so that a deeper one lies over the
    // ones that hold it: the deepest root decides, also for a hidden
    // folder inside a write root. Of two at the same place the later
    // rank lies on top.
    let mut mounts = Vec::<(usize, u8, Vec<String>)>::new();
    let mut covers = Vec::new();
    let mut mount = |path: &str, rank, words: &[&str]| {
        let words = words.iter().map(ToString::to_string).collect();
        mounts.push((depth(path), rank, words));
    };
    for Layer { path, grant } in layers(policy, tmp) {
        match grant {
            // An empty folder over it. One that is missing has
            // nothing to hide and cannot be mounted over.
            Grant::Nothing if is_dir(&path) => {
                mount(&path, 0, &["--tmpfs", &path]);
                covers.push(path);
            }
            Grant::Nothing => {}
            // A root that is missing has nothing to read, as under
            // Seatbelt, and stops no command.
            Grant::Read => {
                mount(&path, 1, &["--ro-bind-try", &path, &path]);
            }
            Grant::Write => {
                mount(&path, 2, &["--bind", &path, &path]);
            }
        }
    }
    // Mount points keep roots and their writable ancestors in place.
    // One under a hidden root is in its empty folder and needs none.
    let writes = write_roots(policy, tmp);
    let mut points = above_roots(policy, tmp);
    points.extend(fixed_roots(policy, tmp));
    let mut seen = BTreeSet::new();
    points.retain(|path| seen.insert(path.clone()));
    for path in &points {
        if Path::new(path).exists()
            && !writes.contains(path)
            && is_granted_writes(policy, tmp, &canon_text(path))
        {
            mount(path, 3, &["--bind", path, path]);
        }
    }
    for path in &policy.locked {
        let path = text(path);
        if is_readable(policy, tmp, &canon_text(&path)) {
            mount(&path, 4, &["--ro-bind-try", &path, &path]);
        }
    }
    mounts.sort_by_key(|(depth, rank, _)| (*depth, *rank));
    for (_, _, words) in &mounts {
        push(&words.iter().map(String::as_str).collect::<Vec<_>>());
    }
    // Read-only once the roots inside it are mounted, so that a
    // write there fails as it does under Seatbelt.
    for path in &covers {
        push(&["--remount-ro", path]);
    }
    push(&["--chdir", cwd, "--"]);
    args.extend(argv.iter().cloned());
    let mut env = policy.env.clone();
    env.insert("TMPDIR".into(), tmp.to_owned());
    Confined { argv: args, env }
}
