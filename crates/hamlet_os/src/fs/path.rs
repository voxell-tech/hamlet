//! POSIX path functions on text, as pi's tools resolve paths, since
//! the policy and pi's tools compare paths as text. On Windows an
//! absolute path starts at a drive, as `C:/Users` does, and `\`
//! separates names too.

use std::path::Path;

/// What separates the names of a path.
const SEPARATORS: &[char] =
    if cfg!(windows) { &['/', '\\'] } else { &['/'] };

/// `path` as text. On Windows a verbatim `\\?\C:\x` reads as `C:/x`.
pub fn text(path: &Path) -> String {
    let text = path.to_string_lossy();
    if !cfg!(windows) {
        return text.into_owned();
    }
    let text = text
        .strip_prefix(r"\\?\")
        .filter(|rest| root_len(rest) > 0)
        .unwrap_or(&text);
    let mut text = text.replace('\\', "/");
    if root_len(&text) > 0 {
        text[..1].make_ascii_uppercase();
    }
    text
}

/// How many bytes the root of `path` takes: `/`, or on Windows a
/// drive such as `C:/`. None in a relative path.
fn root_len(path: &str) -> usize {
    if !cfg!(windows) {
        return usize::from(path.starts_with('/'));
    }
    match path.as_bytes() {
        [drive, b':', b'/' | b'\\', ..]
            if drive.is_ascii_alphabetic() =>
        {
            3
        }
        _ => 0,
    }
}

pub fn is_absolute(path: &str) -> bool {
    root_len(path) > 0
}

/// The root `path` starts at, and the rest. A relative path is taken
/// from `/`.
pub(crate) fn split_root(path: &str) -> (&str, &str) {
    match root_len(path) {
        0 => ("/", path),
        len => path.split_at(len),
    }
}

/// `path` without `.` and `..` parts or doubled slashes, keeping a
/// last slash.
pub fn normalize(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let root = &path[..root_len(path)];
    let absolute = !root.is_empty();
    let mut parts = Vec::<&str>::new();
    for part in path[root.len()..].split(SEPARATORS) {
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
    if !out.is_empty() && path.ends_with(SEPARATORS) {
        out.push('/');
    }
    if absolute {
        format!("{}{out}", text(Path::new(root)))
    } else {
        out
    }
}

/// The parts joined by slashes, normalized.
pub fn join<S: AsRef<str>>(parts: &[S]) -> String {
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
pub fn resolve(base: &str, path: &str) -> String {
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
pub fn dirname(path: &str) -> String {
    let root = &path[..root_len(path)];
    let trimmed = path.trim_end_matches(SEPARATORS);
    if trimmed.is_empty() || trimmed.len() < root.len() {
        return if root.is_empty() { "." } else { root }.into();
    }
    match trimmed.rfind(SEPARATORS) {
        None => ".".into(),
        Some(at) => {
            let parent = trimmed[..at].trim_end_matches(SEPARATORS);
            if parent.len() < root.len() {
                root
            } else {
                parent
            }
            .into()
        }
    }
}

/// The last part of `path`.
pub fn basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches(SEPARATORS);
    trimmed.rsplit(SEPARATORS).next().unwrap_or("")
}

/// `to` from `from`, both resolved.
pub fn relative(from: &str, to: &str) -> String {
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

    #[cfg(unix)]
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

    #[cfg(windows)]
    #[test]
    fn a_windows_path_starts_at_its_drive() {
        assert_eq!(normalize(r"c:\a\.\b\..\c"), "C:/a/c");
        assert_eq!(normalize("C:/../a"), "C:/a");
        assert_eq!(join(&["C:/h", "../1/NOTES.md"]), "C:/1/NOTES.md");
        assert_eq!(dirname("C:/a"), "C:/");
        assert_eq!(dirname("C:/"), "C:/");
        assert_eq!(text(Path::new(r"\\?\c:\Users\x")), "C:/Users/x");
        assert!(!is_absolute("/a"));
    }
}
