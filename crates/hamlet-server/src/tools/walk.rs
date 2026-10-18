//! The walk under a folder that `search` and `find` make, and the
//! words of their refusals.

use std::collections::HashSet;

use crate::sandbox::{Env, FileInfo, FileKind, path};

/// Folders a walk does not go into.
const SKIPPED: [&str; 9] = [
    ".git",
    ".hamlet",
    "target",
    "node_modules",
    "dist",
    "build",
    ".venv",
    "__pycache__",
    ".next",
];

/// The most files a walk looks at.
const VISITED: usize = 20_000;

/// The most hits or names a search gives.
pub(super) const FOUND: usize = 100;

/// A search or listing refused before it looked.
pub(super) enum Failure {
    PatternEmpty,
    PatternNotLinear,
    PatternInvalid(String),
    NotAFolder(String),
    Unreadable { path: String, reason: &'static str },
}

impl Failure {
    /// The words for an agent, with a next action it may take.
    pub(super) fn words(&self) -> String {
        match self {
            Self::PatternEmpty => "`pattern` is required. Supply a \
                                   regular expression."
                .into(),
            Self::PatternNotLinear => {
                "`pattern` holds a backreference, a lookaround or \
                 another part a search in linear time cannot take. \
                 Use a linear-time pattern."
                    .into()
            }
            Self::PatternInvalid(reason) => format!(
                "`pattern` is not a regular expression: {reason}. \
                 Correct the pattern."
            ),
            Self::NotAFolder(path) => format!(
                "{path} is not a folder. Choose a folder, or use \
                 search to read a file."
            ),
            Self::Unreadable { path, reason } => format!(
                "{path} could not be read ({reason}). Choose a \
                 readable path within your reach, or tell the chief."
            ),
        }
    }
}

/// Visits each file under `root`: a folder's files in name order,
/// then its folders, the first first. Links are not followed, and a
/// folder that cannot be listed, such as Hamlet's home, is gone
/// around. It stops when `visit` gives false, or after [`VISITED`]
/// files, and says whether it stopped at that cap.
pub(super) fn walk(
    env: &Env,
    root: &str,
    mut visit: impl FnMut(&str, &FileInfo) -> bool,
) -> bool {
    let skipped = HashSet::from(SKIPPED);
    let mut seen = 0;
    let mut todo = vec![root.to_owned()];
    while let Some(dir) = todo.pop() {
        let Ok(mut listed) = env.list_dir(&dir) else {
            continue;
        };
        listed.sort_by(|a, b| a.name.cmp(&b.name));
        let mut below = Vec::new();
        for entry in &listed {
            let at = path::join(&[dir.as_str(), &entry.name]);
            match entry.kind {
                FileKind::Directory
                    if !skipped.contains(entry.name.as_str()) =>
                {
                    below.push(at);
                }
                FileKind::File => {
                    seen += 1;
                    if seen > VISITED {
                        return true;
                    }
                    if !visit(&at, entry) {
                        return false;
                    }
                }
                _ => {}
            }
        }
        todo.extend(below.into_iter().rev());
    }
    false
}

/// The full path `path` names, from the folder commands run in, and
/// what is there; it fails when that cannot be read.
pub(super) fn reached(
    env: &Env,
    path: Option<&str>,
) -> Result<(String, FileInfo), Failure> {
    let given =
        path.unwrap_or("").trim_matches(crate::change::js_space);
    let full = if path::is_absolute(given) {
        given.to_owned()
    } else {
        path::join(&[env.cwd(), given])
    };
    let failed = |reason| Failure::Unreadable {
        path: shown(env, &full),
        reason,
    };
    let info =
        env.file_info(&full).map_err(|error| failed(error.code))?;
    // A folder's reach is its listing's, and a file's its opening's.
    if info.kind == FileKind::Directory {
        env.list_dir(&full).map_err(|error| failed(error.code))?;
    } else {
        env.open_file(&full).map_err(|error| failed(error.code))?;
    }
    Ok((full, info))
}

/// `path` as a search shows it: under the folder commands run in
/// where it lies there.
pub(super) fn shown(env: &Env, at: &str) -> String {
    let under = path::relative(env.cwd(), at);
    if under.is_empty()
        || under.starts_with("..")
        || path::is_absolute(&under)
    {
        at.to_owned()
    } else {
        under
    }
}

/// `at` under `root`, with `/` between its parts.
pub(super) fn under(root: &str, at: &str) -> String {
    path::relative(root, at)
}

/// The findings of a search, or `none`, with a note where it was cut
/// short.
pub(super) fn report(
    found: &[String],
    capped: bool,
    none: &str,
) -> String {
    let mut lines =
        found.iter().take(FOUND).cloned().collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push(none.into());
    }
    if found.len() > FOUND {
        lines.push(format!(
            "[more than {FOUND}. Narrow it with `path` or `name`]"
        ));
    }
    if capped {
        lines.push(format!(
            "[stopped after {VISITED} files. Narrow it with `path`]"
        ));
    }
    lines.join("\n")
}

/// True when the file at `at` under the searched folder is one
/// `pattern` asks for: by its name, or by its path when `pattern` has
/// a `/`.
pub(super) fn wanted(pattern: &str, at: &str) -> bool {
    let name = if pattern.contains('/') {
        at
    } else {
        at.rsplit('/').next().unwrap_or("")
    };
    glob_matches(pattern, name)
}

/// True when `name` matches `pattern`, where `*` stands for any run
/// of characters and `?` for one.
fn glob_matches(pattern: &str, name: &str) -> bool {
    let want = pattern.chars().collect::<Vec<_>>();
    let have = name.chars().collect::<Vec<_>>();
    let (mut p, mut n) = (0, 0);
    // The place of the last `*`, and how much of the name it took.
    let mut star = None;
    while n < have.len() {
        if p < want.len() && (want[p] == '?' || want[p] == have[n]) {
            p += 1;
            n += 1;
        } else if p < want.len() && want[p] == '*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((at, took)) = &mut star {
            *took += 1;
            p = *at + 1;
            n = *took;
        } else {
            return false;
        }
    }
    want[p..].iter().all(|c| *c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_a_name_or_a_path_under_the_folder() {
        assert!(wanted("*.rs", "src/deep/mod.rs"));
        assert!(wanted("mod.?s", "src/mod.rs"));
        assert!(!wanted("*.rs", "src/mod.rsx"));
        assert!(wanted("src/*/mod.rs", "src/deep/mod.rs"));
        assert!(!wanted("deep/*.rs", "src/deep/mod.rs"));
        assert!(wanted("a*b*c", "aXbYbZc"));
    }
}
