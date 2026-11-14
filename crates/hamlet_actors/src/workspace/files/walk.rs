//! A search of the lines in the files under a folder, and a find of
//! the files under it by name (C16).
// Copied from the server's `tools/{walk,search,find}.rs`, deleted
// with them.

use hamlet_os::fs::path;
use hamlet_sandbox::{Env, FileInfo, FileKind};
use hamlet_workspace_wire::{Found, Listed, Walked};
use regex::Regex;

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
const FOUND: usize = 100;

/// The most characters of a line a search shows.
const WIDTH: usize = 300;

/// The largest file a search reads, in bytes.
const LARGEST: u64 = 1024 * 1024;

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

/// The lines matching `pattern` in the text files under `under`. A
/// pattern runs in linear time; a file over 1 MiB, one that is not
/// UTF-8 and one holding NUL are passed over.
pub(super) fn search(
    env: &Env,
    under: &str,
    pattern: &str,
    name: Option<&str>,
    ignore_case: bool,
) -> Result<String, Failure> {
    if pattern.is_empty() {
        return Err(Failure::PatternEmpty);
    }
    let matches = compile(pattern, ignore_case)?;
    let (root, info) = reached(env, under)?;
    let walked = if info.kind == FileKind::Directory {
        walk(env, &root)
    } else {
        Walked {
            files: vec![Found {
                path: root.clone(),
                size: info.size,
            }],
            capped: false,
        }
    };
    let mut found = Vec::new();
    let mut capped = walked.capped;
    'files: for file in walked.files {
        if let Some(name) = name.filter(|name| !name.is_empty())
            && !is_wanted(name, &path::relative(&root, &file.path))
        {
            continue;
        }
        if file.size > LARGEST {
            continue;
        }
        let Ok(text) = env.read_text(&file.path) else {
            continue;
        };
        if text.contains('\0') {
            continue;
        }
        for (index, line) in lines_of(&text).into_iter().enumerate() {
            if !matches(line) {
                continue;
            }
            found.push(format!(
                "{}:{}: {}",
                shown(env, &file.path),
                index + 1,
                clip(line.trim_end_matches(is_pi_space), WIDTH)
            ));
            if found.len() > FOUND {
                capped = false;
                break 'files;
            }
        }
    }
    Ok(report(&found, capped, "no match"))
}

/// A folder's entries, sorted with `/` after each folder, or the
/// files under it whose name `name` matches.
pub(super) fn find(
    env: &Env,
    under: &str,
    name: Option<&str>,
) -> Result<String, Failure> {
    let (root, info) = reached(env, under)?;
    if info.kind != FileKind::Directory {
        return Err(Failure::NotAFolder(shown(env, &root)));
    }
    let Some(name) = name.filter(|name| !name.is_empty()) else {
        let listed = env.list_dir(&root).map_err(|error| {
            Failure::Unreadable {
                path: shown(env, &root),
                reason: error.code.as_str(),
            }
        })?;
        let mut entries = listed
            .into_iter()
            .map(|entry| match entry.kind {
                FileKind::Directory => format!("{}/", entry.name),
                _ => entry.name,
            })
            .collect::<Vec<_>>();
        entries.sort();
        return Ok(if entries.is_empty() {
            "(empty)".into()
        } else {
            entries.join("\n")
        });
    };
    let mut found = Vec::new();
    let walked = walk(env, &root);
    let mut capped = walked.capped;
    for file in walked.files {
        if is_wanted(name, &path::relative(&root, &file.path)) {
            found.push(shown(env, &file.path));
        }
        if found.len() > FOUND {
            capped = false;
            break;
        }
    }
    Ok(report(&found, capped, "no file by that name"))
}

/// The regular files under `root`, as the policy lets them be listed.
fn walk(env: &Env, root: &str) -> Walked {
    let skip = SKIPPED.map(String::from);
    hamlet_workspace_wire::walk(root, &skip, VISITED as u64, |dir| {
        env.list_dir(dir).map(|listed| {
            listed
                .into_iter()
                .map(|entry| {
                    let kind = match entry.kind {
                        FileKind::File => Listed::File(entry.size),
                        FileKind::Directory => Listed::Directory,
                        FileKind::Symlink => Listed::Other,
                    };
                    (entry.name, kind)
                })
                .collect()
        })
    })
}

/// `under` and what is there; it fails when that cannot be read.
fn reached(
    env: &Env,
    under: &str,
) -> Result<(String, FileInfo), Failure> {
    let full = under.trim_matches(is_pi_space).to_owned();
    let failed = |reason| Failure::Unreadable {
        path: shown(env, &full),
        reason,
    };
    let info = env
        .file_info(&full)
        .map_err(|error| failed(error.code.as_str()))?;
    // A folder's reach is its listing's, and a file's its opening's.
    if info.kind == FileKind::Directory {
        env.list_dir(&full)
            .map_err(|error| failed(error.code.as_str()))?;
    } else {
        env.open_file(&full)
            .map_err(|error| failed(error.code.as_str()))?;
    }
    Ok((full, info))
}

/// `path` as a search shows it: under the folder commands run in
/// where it lies there.
fn shown(env: &Env, at: &str) -> String {
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

/// The findings of a search, or `none`, with a note where it was cut
/// short.
fn report(found: &[String], capped: bool, none: &str) -> String {
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
fn is_wanted(pattern: &str, at: &str) -> bool {
    let name = if pattern.contains('/') {
        at
    } else {
        at.rsplit('/').next().unwrap_or("")
    };
    is_glob_match(pattern, name)
}

/// True when `name` matches `pattern`, where `*` stands for any run
/// of characters and `?` for one.
fn is_glob_match(pattern: &str, name: &str) -> bool {
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

/// A test of a line against `pattern`. For `ignore_case` the pattern
/// and the line are put in lower case, but for the letter that
/// follows a backslash.
fn compile(
    pattern: &str,
    ignore_case: bool,
) -> Result<impl Fn(&str) -> bool, Failure> {
    let mut source = pattern.to_owned();
    if ignore_case {
        source.clear();
        let mut chars = pattern.chars().peekable();
        while let Some(c) = chars.next() {
            match chars.peek() {
                Some(&next) if c == '\\' => {
                    source.push(c);
                    source.push(next);
                    chars.next();
                }
                _ => source.extend(c.to_lowercase()),
            }
        }
    }
    let regex = Regex::new(&source).map_err(|error| {
        let words = error.to_string();
        let reason = words
            .rsplit_once("error: ")
            .map_or(words.as_str(), |(_, reason)| reason)
            .trim()
            .to_owned();
        if reason.contains("look-around")
            || reason.contains("backreferences")
        {
            Failure::PatternNotLinear
        } else {
            Failure::PatternInvalid(reason)
        }
    })?;
    Ok(move |line: &str| {
        if ignore_case {
            regex.is_match(&line.to_lowercase())
        } else {
            regex.is_match(line)
        }
    })
}

/// The lines of `text`, without a last empty one or a `\r` at their
/// end.
fn lines_of(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = text.split('\n').collect::<Vec<_>>();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// At most `most` characters of `text`, ending with `...` when cut.
fn clip(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_owned(),
    }
}

/// Whether pi counts `c` as a space.
pub(super) fn is_pi_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `*` stands for any run of characters, `/` included, and `?`
    /// for one; a `*` that took too little takes more.
    #[test]
    fn names_match_as_globs() {
        assert!(is_glob_match("*.rs", "main.rs"));
        assert!(!is_glob_match("*.rs", "main.rsx"));
        assert!(is_glob_match("a*b*c", "aXbYbZc"));
        assert!(is_glob_match("a*bc", "abcbc"));
        assert!(!is_glob_match("a*bc", "abcb"));
        assert!(is_glob_match("?.md", "a.md"));
        assert!(!is_glob_match("?.md", "ab.md"));
        assert!(is_glob_match("**", ""));
        assert!(!is_glob_match("", "a"));
    }

    /// A pattern with a `/` is matched against the path under the
    /// folder searched, and one without against the name alone.
    #[test]
    fn a_pattern_with_a_slash_matches_the_path() {
        assert!(is_wanted("*.rs", "src/main.rs"));
        assert!(is_wanted("src/*.rs", "src/main.rs"));
        assert!(is_wanted("src/*.rs", "src/deep/main.rs"));
        assert!(!is_wanted("src/*.rs", "lib/src/main.rs"));
        assert!(!is_wanted("main", "src/main.rs"));
    }

    /// A pattern that needs more than linear time and one that is not
    /// a regular expression are refused in words that say which.
    #[test]
    fn a_bad_pattern_is_refused_and_said_why() {
        let refused = |pattern| match compile(pattern, false) {
            Ok(_) => panic!("{pattern} compiled"),
            Err(failure) => failure.words(),
        };
        let not_linear = "`pattern` holds a backreference, a \
                          lookaround or another part a search in \
                          linear time cannot take. Use a \
                          linear-time pattern.";
        assert_eq!(refused("a(?=b)"), not_linear);
        assert_eq!(refused("(a)\\1"), not_linear);
        assert_eq!(
            refused("(a"),
            "`pattern` is not a regular expression: unclosed group. \
             Correct the pattern."
        );
    }

    /// Ignoring case lowers the pattern but for the letter after a
    /// backslash, so `\D` stays a non-digit.
    #[test]
    fn ignoring_case_keeps_an_escape() {
        let matches = compile("HeLLo \\D+", true).ok().unwrap();
        assert!(matches("say HELLO abc"));
        assert!(!matches("say hello 123"));
        let exact = compile("HeLLo", false).ok().unwrap();
        assert!(!exact("hello"));
    }

    /// Lines lose a `\r` and a last empty line, and a long one is
    /// clipped with `...`.
    #[test]
    fn lines_are_split_and_clipped() {
        assert_eq!(lines_of("a\r\nb\n"), ["a", "b"]);
        assert_eq!(lines_of("a\n\n"), ["a", ""]);
        assert!(lines_of("").is_empty());
        assert_eq!(
            clip("\u{e9}\u{e9}\u{e9}\u{e9}", 3),
            "\u{e9}\u{e9}\u{e9}..."
        );
        assert_eq!(clip("abc", 3), "abc");
    }
}
