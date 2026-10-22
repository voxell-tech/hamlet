//! The tools that only read: a file, a search of file contents and a
//! search of file names. They change nothing, so they never wait for
//! a chief.

use std::path::{Path, PathBuf};

use regex::RegexBuilder;
use serde_json::{Value, json};

/// The names of these tools.
pub(super) const NAMES: [&str; 3] = ["read", "search", "find"];

/// Folders a search does not go into.
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

/// The most lines `read` gives at once.
const LINES: usize = 400;
/// The most of one line that is shown, in characters.
const WIDTH: usize = 300;
/// The most matches or names a search gives.
const FOUND: usize = 100;
/// The most files a search looks at.
const VISITED: usize = 20_000;
/// The largest file a search reads, in bytes.
const LARGEST: u64 = 1024 * 1024;

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({"type": "function", "function": {
            "name": "read",
            "description": "Read a text file, with its line numbers. Gives up to 400 lines at a time. \
                            Never waits for approval: use it rather than cat, head or sed.",
            "parameters": {"type": "object", "properties": {
                "path": {"type": "string", "description": "Relative to the project, or a full path."},
                "from": {"type": "integer", "description": "The first line to give, counting from 1. Leave out for the start."},
                "lines": {"type": "integer", "description": "How many lines to give. Leave out for 400."}},
                "required": ["path"]}}}),
        json!({"type": "function", "function": {
            "name": "search",
            "description": "Search the contents of files under a folder for a regular expression. Gives each \
                            match as path:line: text, up to 100. Skips .git and build folders such as target \
                            and node_modules. Never waits for approval: use it rather than grep or rg.",
            "parameters": {"type": "object", "properties": {
                "pattern": {"type": "string", "description": "A regular expression, such as `fn main` or `TODO|FIXME`."},
                "path": {"type": "string", "description": "The folder or file to search. Leave out for the whole project."},
                "name": {"type": "string", "description": "Only files whose name matches, such as `*.rs`."},
                "ignore_case": {"type": "boolean"}},
                "required": ["pattern"]}}}),
        json!({"type": "function", "function": {
            "name": "find",
            "description": "List a folder, or find files by name under it. Without `name` it lists what is \
                            directly in the folder. With `name` it gives every file under the folder whose \
                            name matches, up to 100. Never waits for approval: use it rather than ls or find.",
            "parameters": {"type": "object", "properties": {
                "path": {"type": "string", "description": "The folder. Leave out for the project."},
                "name": {"type": "string", "description": "A file name with `*` and `?`, such as `*.toml` or `mod.rs`. \
                                                            With a `/` in it, it is matched against the path under the folder."}},
                "required": []}}}),
    ]
}

/// The folders these tools may look in.
pub(super) struct Sight<'a> {
    /// The folder a relative path starts from.
    pub base: &'a Path,
    /// A folder that is not looked into.
    pub hidden: &'a Path,
    /// Folders inside `hidden` that are looked into all the same.
    pub shown: &'a [PathBuf],
}

impl Sight<'_> {
    fn sees(&self, real: &Path) -> bool {
        !real.starts_with(self.hidden)
            || self.shown.iter().any(|shown| real.starts_with(shown))
    }

    /// The real path `path` names, when it exists and may be read.
    fn resolve(&self, path: Option<&str>) -> Result<PathBuf, String> {
        let given = path.unwrap_or("").trim();
        let full = self.base.join(given);
        let real = full
            .canonicalize()
            .map_err(|e| format!("{}: {e}", full.display()))?;
        if self.sees(&real) {
            Ok(real)
        } else {
            Err(format!(
                "{} is Hamlet's own and cannot be read",
                full.display()
            ))
        }
    }

    /// `path` as it is shown: under the base where it lies there.
    fn names(&self, path: &Path) -> String {
        path.strip_prefix(self.base)
            .unwrap_or(path)
            .display()
            .to_string()
    }
}

fn clip(line: &str) -> String {
    if line.chars().count() <= WIDTH {
        line.to_string()
    } else {
        format!("{}...", line.chars().take(WIDTH).collect::<String>())
    }
}

/// True when `name` matches `pattern`, in which `*` stands for any
/// run of characters and `?` for one.
fn matches(pattern: &str, name: &str) -> bool {
    let (pattern, name) = (
        pattern.chars().collect::<Vec<_>>(),
        name.chars().collect::<Vec<_>>(),
    );
    let (mut p, mut n) = (0, 0);
    // The place of the last `*`, and the length of name it took.
    let mut star = None::<(usize, usize)>;
    while n < name.len() {
        if p < pattern.len()
            && (pattern[p] == '?' || pattern[p] == name[n])
        {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((at, took)) = star {
            star = Some((at, took + 1));
            p = at + 1;
            n = took + 1;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

/// True when the file at `rel` under the searched folder is one
/// `pattern` asks for.
fn wanted(pattern: &str, rel: &Path) -> bool {
    if pattern.contains('/') {
        matches(pattern, &rel.to_string_lossy())
    } else {
        rel.file_name().is_some_and(|name| {
            matches(pattern, &name.to_string_lossy())
        })
    }
}

/// Every file under `root`, in name order, until `visit` says stop
/// or `VISITED` files were seen. Links are not followed. Returns
/// whether it stopped at the cap.
fn walk(
    sight: &Sight,
    root: &Path,
    visit: &mut dyn FnMut(&Path) -> bool,
) -> bool {
    let mut seen = 0;
    let mut todo = vec![root.to_path_buf()];
    while let Some(dir) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries =
            entries.flatten().map(|e| e.path()).collect::<Vec<_>>();
        entries.sort();
        let mut below = Vec::new();
        for path in entries {
            let Ok(meta) = path.symlink_metadata() else {
                continue;
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if meta.is_dir() {
                if !SKIPPED.contains(&name.as_str())
                    && sight.sees(&path)
                {
                    below.push(path);
                }
            } else if meta.is_file() {
                seen += 1;
                if seen > VISITED {
                    return true;
                }
                if !visit(&path) {
                    return false;
                }
            }
        }
        // Popped from the end, so the first folder is searched first.
        todo.extend(below.into_iter().rev());
    }
    false
}

pub(super) fn read(
    sight: &Sight,
    args: &Value,
) -> Result<String, String> {
    let path = sight.resolve(args["path"].as_str())?;
    if path.is_dir() {
        return Err(format!(
            "{} is a folder. List it with `find`",
            sight.names(&path)
        ));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("{}: {e}", sight.names(&path)))?;
    let from = args["from"].as_u64().unwrap_or(1).max(1) as usize;
    let most = (args["lines"].as_u64().unwrap_or(LINES as u64)
        as usize)
        .clamp(1, LINES);
    let total = text.lines().count();
    let mut out = text
        .lines()
        .enumerate()
        .skip(from - 1)
        .take(most)
        .map(|(i, line)| format!("{}\t{}\n", i + 1, clip(line)))
        .collect::<String>();
    let last = (from - 1 + most).min(total);
    if from > total && total > 0 {
        out = format!("[the file has {total} lines]");
    } else if last < total {
        out.push_str(&format!(
            "[lines {from} to {last} of {total}. Pass `from`: {} \
             for more]",
            last + 1
        ));
    }
    Ok(out)
}

pub(super) fn search(
    sight: &Sight,
    args: &Value,
) -> Result<String, String> {
    let pattern = args["pattern"]
        .as_str()
        .filter(|p| !p.is_empty())
        .ok_or("`pattern` is required")?;
    let regex = RegexBuilder::new(pattern)
        .case_insensitive(args["ignore_case"].as_bool() == Some(true))
        .build()
        .map_err(|e| {
            format!("`pattern` is not a regular expression: {e}")
        })?;
    let root = sight.resolve(args["path"].as_str())?;
    let name = args["name"].as_str().filter(|n| !n.is_empty());
    let mut found = Vec::new();
    let mut look = |path: &Path| {
        let rel = path.strip_prefix(&root).unwrap_or(path);
        if name.is_some_and(|name| !wanted(name, rel)) {
            return true;
        }
        if path.metadata().is_ok_and(|m| m.len() > LARGEST) {
            return true;
        }
        // A file that is not text is not searched.
        let Ok(text) = std::fs::read_to_string(path) else {
            return true;
        };
        if text.contains('\0') {
            return true;
        }
        for (i, line) in text.lines().enumerate() {
            if regex.is_match(line) {
                found.push(format!(
                    "{}:{}: {}",
                    sight.names(path),
                    i + 1,
                    clip(line.trim_end())
                ));
                if found.len() > FOUND {
                    return false;
                }
            }
        }
        true
    };
    let capped = if root.is_file() {
        look(&root);
        false
    } else {
        walk(sight, &root, &mut look)
    };
    Ok(report(found, capped, "no match"))
}

pub(super) fn find(
    sight: &Sight,
    args: &Value,
) -> Result<String, String> {
    let root = sight.resolve(args["path"].as_str())?;
    if !root.is_dir() {
        return Err(format!(
            "{} is not a folder",
            sight.names(&root)
        ));
    }
    let Some(name) = args["name"].as_str().filter(|n| !n.is_empty())
    else {
        let mut entries = std::fs::read_dir(&root)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| {
                let mut name =
                    e.file_name().to_string_lossy().into_owned();
                if e.path().is_dir() {
                    name.push('/');
                }
                name
            })
            .collect::<Vec<_>>();
        entries.sort();
        return Ok(match entries.len() {
            0 => "(empty)".to_string(),
            _ => entries.join("\n"),
        });
    };
    let mut found = Vec::new();
    let capped = walk(sight, &root, &mut |path| {
        if wanted(name, path.strip_prefix(&root).unwrap_or(path)) {
            found.push(sight.names(path));
        }
        found.len() <= FOUND
    });
    Ok(report(found, capped, "no file by that name"))
}

/// The findings of a search, with a note where it was cut short.
fn report(
    mut found: Vec<String>,
    capped: bool,
    none: &str,
) -> String {
    let more = found.len() > FOUND;
    found.truncate(FOUND);
    if found.is_empty() {
        found.push(none.to_string());
    }
    if more {
        found.push(format!(
            "[more than {FOUND}. Narrow it with `path` or `name`]"
        ));
    }
    if capped {
        found.push(format!(
            "[stopped after {VISITED} files. Narrow it with `path`]"
        ));
    }
    found.join("\n")
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn reading_is_bounded_and_keeps_out_of_what_is_hidden() {
        let (dir, outer) =
            (Scratch::new("reading"), Scratch::new("reading-outer"));
        let home = outer.join("home");
        std::fs::create_dir(&home).unwrap();
        let write = |rel: &str, text: &str| {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write("src/main.rs", "fn main() {\n    todo!()\n}\n");
        write("src/deep/mod.rs", "// TODO later\n");
        write("target/debug/main.rs", "fn main() {}\n");
        write(".git/config", "fn main\n");
        write("blob.bin", "\u{0}fn main");
        std::fs::write(dir.join("raw"), [0xff, 0xfe]).unwrap();
        std::fs::write(home.join("api_key.x"), "fn main").unwrap();
        std::fs::create_dir(home.join("workspace")).unwrap();
        std::fs::write(home.join("workspace/NOTES.md"), "mine")
            .unwrap();
        let shown = [home.join("workspace")];
        let sight = Sight {
            base: &dir,
            hidden: &home,
            shown: &shown,
        };

        let two =
            read(&sight, &json!({"path": "src/main.rs", "lines": 2}))
                .unwrap();
        assert_eq!(
            two,
            "1\tfn main() {\n2\t    todo!()\n[lines 1 to 2 of 3. \
             Pass `from`: 3 for more]"
        );
        let last =
            read(&sight, &json!({"path": "src/main.rs", "from": 3}));
        assert_eq!(last.unwrap(), "3\t}\n");

        let hits =
            search(&sight, &json!({"pattern": "fn\\s+main|todo"}))
                .unwrap();
        assert_eq!(
            hits,
            "src/main.rs:1: fn main() {\nsrc/main.rs:2:     todo!()"
        );
        let hits = search(
            &sight,
            &json!({"pattern": "todo", "ignore_case": true, "name": "mod.*"}),
        );
        assert_eq!(hits.unwrap(), "src/deep/mod.rs:1: // TODO later");
        assert!(search(&sight, &json!({"pattern": "("})).is_err());

        assert_eq!(
            find(&sight, &json!({"name": "*.rs"})).unwrap(),
            "src/main.rs\nsrc/deep/mod.rs"
        );
        assert_eq!(
            find(&sight, &json!({"path": "src"})).unwrap(),
            "deep/\nmain.rs"
        );

        // Hamlet's own folder is closed, but for what is shown.
        let key = home.join("api_key.x");
        let key = json!({"path": key.to_str().unwrap()});
        assert!(
            read(&sight, &key).unwrap_err().contains("Hamlet's own")
        );
        let all = json!({"pattern": "fn main", "path": home.to_str().unwrap()});
        assert!(search(&sight, &all).is_err());
        let notes = home.join("workspace/NOTES.md");
        let notes = json!({"path": notes.to_str().unwrap()});
        assert_eq!(read(&sight, &notes).unwrap(), "1\tmine\n");
        // A search from above goes around it.
        let above = Sight {
            base: &outer,
            ..sight
        };
        let from_above = search(
            &above,
            &json!({"pattern": "fn main", "name": "api_key.*"}),
        );
        assert_eq!(from_above.unwrap(), "no match");
    }
}
