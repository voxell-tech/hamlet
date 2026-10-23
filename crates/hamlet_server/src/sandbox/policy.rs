//! What a sandboxed command may read, write and reach, and the paths
//! it compares as the system resolves them.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::path::{self, text};

/// The resources one command may use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Seconds of CPU time.
    pub cpu_secs: u64,
    /// Seconds of wall time.
    pub wall_secs: u64,
    /// Processes of the user at once.
    pub max_procs: u64,
    /// The largest file written, in MiB.
    pub max_file_mb: u64,
}

impl Default for Limits {
    /// A command also never dumps core.
    fn default() -> Self {
        Self {
            cpu_secs: 60,
            wall_secs: 120,
            max_procs: 4096,
            max_file_mb: 512,
        }
    }
}

/// The longest wall time in seconds that a command's timer takes.
const MAX_WALL_SECS: u64 = 2_147_483;

/// The most symbolic links Linux follows in one path, more than macOS
/// does.
pub(super) const MAX_LINKS: usize = 40;

/// The reads, writes and reach of a sandboxed command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    /// Paths readable at the same location.
    pub ro: Vec<PathBuf>,
    /// Paths readable and writable at the same location.
    pub rw: Vec<PathBuf>,
    /// The whole machine can be read, but for `hidden`.
    pub open: bool,
    /// Paths that stay unreadable when `open`. A path of `ro` or
    /// `rw` inside one is still granted.
    pub hidden: Vec<PathBuf>,
    /// Paths inside `rw` that cannot be written, renamed or removed.
    pub locked: Vec<PathBuf>,
    /// Extra roots whose own location is fixed, without granting
    /// reads or writes.
    pub fixed: Vec<PathBuf>,
    /// The network is reachable.
    pub net: bool,
    /// The complete environment; everything else is cleared.
    pub env: BTreeMap<String, String>,
    pub limits: Limits,
}

impl Policy {
    /// Refuses limits unless each is at least 1 and the wall time
    /// fits the timer.
    pub(crate) fn check_limits(&self) -> Result<(), String> {
        let limits = &self.limits;
        for (name, value) in [
            ("cpuSecs", limits.cpu_secs),
            ("wallSecs", limits.wall_secs),
            ("maxProcs", limits.max_procs),
            ("maxFileMb", limits.max_file_mb),
        ] {
            if value < 1 {
                return Err(format!(
                    "The limit {name} is not a whole number of at \
                     least 1: {value}"
                ));
            }
        }
        if limits.wall_secs > MAX_WALL_SECS {
            return Err(format!(
                "The limit wallSecs is more than {MAX_WALL_SECS} \
                 seconds: {}",
                limits.wall_secs
            ));
        }
        Ok(())
    }
}

/// A short digest of `policy` as `sandbox` runs it, which names the
/// policy in a command's record.
pub fn policy_hash(policy: &Policy, sandbox: &str) -> String {
    let mut fields = vec![("sandbox", sandbox.to_owned())];
    let paths = |tag, list: &[PathBuf]| {
        list.iter()
            .map(|each| (tag, text(each)))
            .collect::<Vec<_>>()
    };
    fields.extend(paths("ro", &policy.ro));
    fields.extend(paths("rw", &policy.rw));
    if policy.open {
        fields.push(("open", "true".into()));
    }
    fields.extend(paths("hidden", &policy.hidden));
    fields.extend(paths("locked", &policy.locked));
    fields.extend(paths("fixed", &policy.fixed));
    fields.push(("net", policy.net.to_string()));
    for (name, value) in &policy.env {
        fields.push(("envk", name.clone()));
        fields.push(("envv", value.clone()));
    }
    let limits = &policy.limits;
    fields.push(("cpu", limits.cpu_secs.to_string()));
    fields.push(("wall", limits.wall_secs.to_string()));
    fields.push(("procs", limits.max_procs.to_string()));
    fields.push(("file", limits.max_file_mb.to_string()));
    let text = fields
        .iter()
        .map(|(tag, shown)| {
            format!("{tag}={}:{shown}\n", shown.len())
        })
        .collect::<String>();
    let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
    digest[..12].to_owned()
}

/// The host variables a normal process needs.
pub(crate) fn host_env() -> BTreeMap<String, String> {
    let mut env = BTreeMap::from([("LANG".into(), "C.UTF-8".into())]);
    for name in ["PATH", "HOME", "LANG"] {
        if let Ok(value) = std::env::var(name) {
            env.insert(name.into(), value);
        }
    }
    env
}

/// True when `path` is `parent` or lies inside it.
pub fn within(path: &Path, parent: &Path) -> bool {
    within_text(&text(path), &text(parent))
}

pub(crate) fn within_text(path: &str, parent: &str) -> bool {
    path == parent
        || if parent.ends_with('/') {
            path.starts_with(parent)
        } else {
            path.strip_prefix(parent)
                .is_some_and(|rest| rest.starts_with('/'))
        }
}

/// The real path of `path`, where a file made at it lands, with each
/// name in the case its volume keeps. Every symbolic link on the way
/// is followed, also one whose chain ends at a missing file. The part
/// that does not exist yet is kept as written.
pub(crate) fn canon_text(path: &str) -> String {
    let mut rest = path::resolve(".", path)
        .split('/')
        .map(str::to_owned)
        .collect::<VecDeque<_>>();
    let mut real = "/".to_owned();
    let mut links = 0;
    while let Some(part) = rest.pop_front() {
        if part.is_empty() || part == "." {
            continue;
        }
        // `real` holds no link, so its parent is the folder `..`
        // leads to.
        if part == ".." {
            real = path::dirname(&real);
            continue;
        }
        let next = path::join(&[real.as_str(), &part]);
        let link = match fs::symlink_metadata(&next) {
            Ok(meta) if !meta.file_type().is_symlink() => {
                real = next;
                continue;
            }
            Ok(_) => fs::read_link(&next),
            Err(error) => Err(error),
        };
        let Ok(link) = link else {
            let mut parts = vec![cased(&real), part];
            parts.extend(rest);
            return path::join(&parts);
        };
        // The system refuses a path through more links, so it leads
        // nowhere.
        links += 1;
        if links > MAX_LINKS {
            return path.to_owned();
        }
        let link = text(&link);
        if path::is_absolute(&link) {
            real = "/".into();
        }
        for piece in link.split('/').rev() {
            rest.push_front(piece.to_owned());
        }
    }
    cased(&real)
}

/// `real`, a path without symbolic links, with each name in the case
/// its volume keeps.
pub(super) fn cased(real: &str) -> String {
    fs::canonicalize(real)
        .map(|path| text(&path))
        .unwrap_or_else(|_| real.to_owned())
}

/// A directory root's canonical spelling. Links and special files are
/// refused.
pub(crate) fn directory_root(path: &str) -> Result<String, String> {
    let absolute = path::resolve(".", path);
    match fs::symlink_metadata(&absolute) {
        Ok(meta) if !meta.is_dir() => {
            return Err(format!(
                "The sandbox root must be a real directory: {path}"
            ));
        }
        Err(error)
            if error.kind() != std::io::ErrorKind::NotFound =>
        {
            return Err(error.to_string());
        }
        _ => {}
    }
    Ok(canon_text(&absolute))
}

/// The folder that holds every private temp dir.
pub(crate) fn temp_root() -> String {
    path::join(&[
        canon_text(&text(&std::env::temp_dir())),
        "hamlet-sandbox-tmp".into(),
    ])
}

/// The private temp dir a sandboxed command may write to: one for
/// each set of writable paths, so each house has its own.
pub(crate) fn private_tmp(policy: &Policy) -> Result<String, String> {
    let mut digest = Sha256::new();
    for each in &policy.rw {
        digest.update(canon_text(&text(each)).as_bytes());
        digest.update(b"\0");
    }
    let hex = format!("{:x}", digest.finalize());
    let root = temp_root();
    let dir = path::join(&[root.as_str(), &hex[..16]]);
    directory_root(&root)?;
    directory_root(&dir)?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    directory_root(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    Ok(dir)
}

/// The writable roots of a policy, including its private temp folder.
pub(crate) fn write_roots(policy: &Policy, tmp: &str) -> Vec<String> {
    let mut roots =
        policy.rw.iter().map(|p| text(p)).collect::<Vec<_>>();
    roots.push(tmp.to_owned());
    roots
}

/// Every granted or fixed root, including the private temp folder.
pub(crate) fn fixed_roots(policy: &Policy, tmp: &str) -> Vec<String> {
    let mut roots = write_roots(policy, tmp);
    for each in
        [&policy.ro, &policy.locked, &policy.hidden, &policy.fixed]
            .into_iter()
            .flatten()
    {
        roots.push(text(each));
    }
    let mut seen = BTreeSet::new();
    roots.retain(|root| seen.insert(root.clone()));
    roots
}

fn in_any(path: &str, paths: &[String]) -> bool {
    paths
        .iter()
        .any(|each| within_text(path, &canon_text(each)))
}

/// True when `path` lies in any of `paths`, compared without case. A
/// refusal uses it, so that a volume that ignores case cannot carry a
/// path past one.
fn in_any_folded(path: &str, paths: &[String]) -> bool {
    let folded = path.to_lowercase();
    paths.iter().any(|each| {
        within_text(&folded, &canon_text(each).to_lowercase())
    })
}

/// True when `policy` lets a command with `tmp` as its temp folder
/// read `real`, a path without links.
pub(crate) fn readable(
    policy: &Policy,
    tmp: &str,
    real: &str,
) -> bool {
    let mut granted =
        policy.ro.iter().map(|p| text(p)).collect::<Vec<_>>();
    granted.extend(write_roots(policy, tmp));
    if in_any(real, &granted) {
        return true;
    }
    let mut closed =
        policy.hidden.iter().map(|p| text(p)).collect::<Vec<_>>();
    closed.push(temp_root());
    policy.open && !in_any_folded(real, &closed)
}

/// True when `policy` lets a command with `tmp` as its temp folder
/// write `real`, a path without links, or with `move` also move or
/// remove it.
pub(crate) fn writable(
    policy: &Policy,
    tmp: &str,
    real: &str,
    moved: bool,
) -> bool {
    let locked =
        policy.locked.iter().map(|p| text(p)).collect::<Vec<_>>();
    in_any(real, &write_roots(policy, tmp))
        && !in_any_folded(real, &locked)
        && !(moved
            && fixed_roots(policy, tmp).iter().any(|root| {
                within_text(
                    &canon_text(root).to_lowercase(),
                    &real.to_lowercase(),
                )
            }))
}

/// True when `path` is a directory.
pub(crate) fn is_dir(path: &str) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// Writable ancestors whose move would carry a fixed root away.
pub(crate) fn above_roots(policy: &Policy, tmp: &str) -> Vec<String> {
    let mut above = BTreeSet::new();
    let writes = write_roots(policy, tmp)
        .iter()
        .map(|root| canon_text(root))
        .collect::<Vec<_>>();
    for root in fixed_roots(policy, tmp) {
        let mut up = path::dirname(&root);
        while up != path::dirname(&up) {
            let real = canon_text(&up);
            if writes
                .iter()
                .any(|rw| within_text(&real, rw) && real != *rw)
            {
                above.insert(up.clone());
            }
            up = path::dirname(&up);
        }
    }
    above.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_policy_hash_is_stable() {
        let policy = Policy {
            ro: vec!["/shared/tools".into()],
            rw: vec!["/home/houses/home/0".into()],
            env: BTreeMap::from([
                ("PATH".into(), "/usr/bin:/bin".into()),
                ("HOME".into(), "/home/houses/home/0".into()),
            ]),
            ..Policy::default()
        };
        assert_eq!(policy_hash(&policy, "bwrap"), "92bff1be7aec");
        let open = Policy {
            rw: vec!["/f".into(), "/h".into()],
            open: true,
            hidden: vec!["/home".into()],
            net: true,
            env: BTreeMap::from([("LANG".into(), "C.UTF-8".into())]),
            ..Policy::default()
        };
        assert_eq!(policy_hash(&open, "seatbelt"), "2888fdba2c83");
    }
}
