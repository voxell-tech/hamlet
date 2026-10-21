//! One folder's entries, in the order a file explorer lists them.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::io;
use std::iter::Peekable;
use std::path::{Path, PathBuf};
use std::str::Chars;

use ignore::WalkBuilder;

/// The names never listed, as VS Code leaves them out by default.
const EXCLUDED: [&str; 6] =
    [".git", ".svn", ".hg", "CVS", ".DS_Store", "Thumbs.db"];

/// What an entry is, its links followed.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Folder,
    File,
}

/// One entry of a folder.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: Kind,
    /// Left out by the git ignore rules over it.
    pub ignored: bool,
}

impl Entry {
    pub fn is_folder(&self) -> bool {
        self.kind == Kind::Folder
    }
}

/// The entries of `folder`, folders first and then by name, without
/// the paths in `hidden` or the names always left out. Every entry
/// of a folder that is `ignored` is ignored too.
pub fn list(
    folder: &Path,
    hidden: &[PathBuf],
    ignored: bool,
) -> io::Result<Vec<Entry>> {
    let kept = if ignored {
        HashSet::new()
    } else {
        unignored(folder)
    };
    let mut entries = std::fs::read_dir(folder)?
        .filter_map(Result::ok)
        .filter_map(|found| {
            let name =
                found.file_name().to_string_lossy().into_owned();
            let path = found.path();
            if EXCLUDED.contains(&name.as_str())
                || hidden.contains(&path)
            {
                return None;
            }
            let kind = if path.is_dir() {
                Kind::Folder
            } else {
                Kind::File
            };
            Some(Entry {
                ignored: !kept.contains(&path),
                name,
                path,
                kind,
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(order);
    Ok(entries)
}

/// The paths directly in `folder` that git's ignore rules keep.
fn unignored(folder: &Path) -> HashSet<PathBuf> {
    WalkBuilder::new(folder)
        .max_depth(Some(1))
        .hidden(false)
        // `.ignore` files are ripgrep's, not git's.
        .ignore(false)
        .build()
        .filter_map(Result::ok)
        .filter(|found| found.depth() == 1)
        .map(|found| found.into_path())
        .collect()
}

/// Folders before files, then by [`natural`] order of their names.
fn order(a: &Entry, b: &Entry) -> Ordering {
    b.is_folder()
        .cmp(&a.is_folder())
        .then_with(|| natural(&a.name, &b.name))
        .then_with(|| a.name.cmp(&b.name))
}

/// `a` against `b` with no regard to case, and each run of digits
/// taken as a number: `file2` before `file10`.
fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        let found = match (a.peek(), b.peek()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y))
                if x.is_ascii_digit() && y.is_ascii_digit() =>
            {
                let (x, y) = (digits(&mut a), digits(&mut b));
                let (x, y) = (
                    x.trim_start_matches('0'),
                    y.trim_start_matches('0'),
                );
                x.len().cmp(&y.len()).then_with(|| x.cmp(y))
            }
            (Some(x), Some(y)) => {
                let found = x.to_lowercase().cmp(y.to_lowercase());
                a.next();
                b.next();
                found
            }
        };
        if found != Ordering::Equal {
            return found;
        }
    }
}

/// The run of digits `chars` starts with, taken from it.
fn digits(chars: &mut Peekable<Chars>) -> String {
    let mut run = String::new();
    while let Some(digit) = chars.next_if(char::is_ascii_digit) {
        run.push(digit);
    }
    run
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_ordered_as_a_person_reads_them() {
        let mut names = [
            "file10", "File2", "file1", "b", "A", "file02x", "a10b",
            "a9b", "_x",
        ];
        names.sort_by(|a, b| natural(a, b).then_with(|| a.cmp(b)));
        assert_eq!(
            names,
            [
                "_x", "A", "a9b", "a10b", "b", "file1", "File2",
                "file02x", "file10"
            ]
        );
    }

    #[test]
    fn a_folder_is_listed_as_an_explorer_shows_it() {
        let all = hamlet_testing::Scratch::new("desktop-listing");
        let home = all.join("home");
        for folder in [".git", "src", "target/debug", "Docs", "home"]
        {
            std::fs::create_dir_all(all.join(folder)).unwrap();
        }
        for file in [
            ".env",
            ".DS_Store",
            "run.log",
            "notes10.md",
            "notes9.md",
        ] {
            std::fs::write(all.join(file), "").unwrap();
        }
        std::fs::write(all.join(".gitignore"), "target/\n*.log\n")
            .unwrap();
        let shown = |entries: Vec<Entry>| {
            entries
                .into_iter()
                .map(|entry| (entry.name, entry.ignored))
                .collect::<Vec<_>>()
        };
        let named = |pairs: &[(&str, bool)]| {
            pairs
                .iter()
                .map(|(name, ignored)| (name.to_string(), *ignored))
                .collect::<Vec<_>>()
        };

        let top = list(&all, &[home], false).unwrap();
        assert_eq!(top[0].path, all.join("Docs"));
        assert_eq!(top[0].kind, Kind::Folder);
        assert_eq!(
            shown(top),
            named(&[
                ("Docs", false),
                ("src", false),
                ("target", true),
                (".env", false),
                (".gitignore", false),
                ("notes9.md", false),
                ("notes10.md", false),
                ("run.log", true),
            ])
        );
        assert_eq!(
            shown(list(&all.join("target"), &[], true).unwrap()),
            named(&[("debug", true)])
        );
        assert!(list(&all.join("gone"), &[], false).is_err());
    }
}
