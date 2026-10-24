//! POSIX path functions on text, as pi's tools resolve paths, since
//! the policy and pi's tools compare paths as text.

use std::path::Path;

/// `path` as text.
pub(crate) fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(crate) fn is_absolute(path: &str) -> bool {
    path.starts_with('/')
}

/// `path` without `.` and `..` parts or doubled slashes, keeping a
/// last slash.
pub(crate) fn normalize(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let absolute = is_absolute(path);
    let mut parts = Vec::<&str>::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            _ => parts.push(part),
        }
    }
    let mut out = parts.join("/");
    if out.is_empty() && !absolute {
        out.push('.');
    }
    if !out.is_empty() && path.ends_with('/') {
        out.push('/');
    }
    if absolute { format!("/{out}") } else { out }
}

/// The parts joined by slashes, normalized.
pub(crate) fn join<S: AsRef<str>>(parts: &[S]) -> String {
    let joined = parts
        .iter()
        .map(AsRef::as_ref)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    if joined.is_empty() {
        ".".into()
    } else {
        normalize(&joined)
    }
}

/// The absolute path `path` names from `base`, itself from the
/// process's working directory when relative.
pub(crate) fn resolve(base: &str, path: &str) -> String {
    let joined = if is_absolute(path) {
        path.to_owned()
    } else if is_absolute(base) {
        format!("{base}/{path}")
    } else {
        let cwd = std::env::current_dir()
            .map(|cwd| text(&cwd))
            .unwrap_or_else(|_| "/".into());
        format!("{cwd}/{base}/{path}")
    };
    let normal = normalize(&joined);
    match normal.strip_suffix('/') {
        Some(stripped) if !stripped.is_empty() => stripped.to_owned(),
        _ => normal,
    }
}

/// The folder `path` is in.
pub(crate) fn dirname(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.is_empty() { "." } else { "/" }.into();
    }
    match trimmed.rfind('/') {
        None => ".".into(),
        Some(at) => {
            let parent = trimmed[..at].trim_end_matches('/');
            if parent.is_empty() { "/" } else { parent }.into()
        }
    }
}

/// The last part of `path`.
pub(crate) fn basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or("")
}

/// `to` from `from`, both resolved.
pub(crate) fn relative(from: &str, to: &str) -> String {
    let from = resolve("/", from);
    let to = resolve("/", to);
    if from == to {
        return String::new();
    }
    let parts = |path: &str| {
        path.split('/')
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let (from, to) = (parts(&from), parts(&to));
    let common =
        from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut out = vec![".."; from.len() - common];
    out.extend(to[common..].iter().map(String::as_str));
    out.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_as_pi_s_tools_do() {
        assert_eq!(normalize("/a/./b/../c/"), "/a/c/");
        assert_eq!(normalize("a/../../b"), "../b");
        assert_eq!(normalize("/../a"), "/a");
        assert_eq!(join(&["/h", "../1/NOTES.md"]), "/1/NOTES.md");
        assert_eq!(dirname("/a/b/"), "/a");
        assert_eq!(dirname("/a"), "/");
        assert_eq!(relative("/a/b", "/a/c/d"), "../c/d");
        assert_eq!(relative("/a", "/a/b"), "b");
    }
}
