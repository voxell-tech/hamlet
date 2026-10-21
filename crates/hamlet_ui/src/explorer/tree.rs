//! A work folder as a tree: which folders are open, what each held
//! when last read, and the one entry selected.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use hamlet_desktop::files::Entry;

/// The most entries of one folder shown.
const SHOWN: usize = 500;

/// What a folder held when last read.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Listing {
    Read(Vec<Entry>),
    /// Why it could not be read.
    Failed(String),
}

/// A folder, and the folders opened in it.
#[derive(Debug)]
pub(crate) struct Tree {
    root: PathBuf,
    open: HashSet<PathBuf>,
    lists: HashMap<PathBuf, Listing>,
    selected: Option<PathBuf>,
}

/// One line of the tree as shown, `depth` folders in from the root.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Row {
    pub depth: usize,
    pub line: Line,
}

/// What a [`Row`] shows.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Line {
    Entry {
        entry: Entry,
        open: bool,
        selected: bool,
    },
    /// A folder that is being read for the first time.
    Reading(PathBuf),
    /// A folder that could not be read, and why.
    Failed(PathBuf, String),
    /// A folder, and how many of its entries are past the most shown.
    More(PathBuf, usize),
    /// The root, read and found empty.
    Empty,
}

impl Tree {
    pub fn new(root: PathBuf) -> Self {
        Tree {
            root,
            open: HashSet::new(),
            lists: HashMap::new(),
            selected: None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn select(&mut self, path: &Path) {
        self.selected = Some(path.to_path_buf());
    }

    /// Opens `folder` if it is closed, and closes it if it is open.
    /// The folders open inside it stay open, for when it opens again.
    pub fn toggle(&mut self, folder: &Path) {
        if !self.open.remove(folder) {
            self.open.insert(folder.to_path_buf());
        }
    }

    /// Closes every folder.
    pub fn collapse(&mut self) {
        self.open.clear();
    }

    /// Opens every folder between the root and `path`, and selects
    /// `path`.
    pub fn reveal(&mut self, path: &Path) {
        let inside = path
            .ancestors()
            .skip(1)
            .take_while(|folder| *folder != self.root)
            .filter(|folder| folder.starts_with(&self.root));
        self.open.extend(inside.map(Path::to_path_buf));
        self.select(path);
    }

    pub fn listing(&self, folder: &Path) -> Option<&Listing> {
        self.lists.get(folder)
    }

    /// Takes `listing` as what `folder` holds now. Gives the folders
    /// in it that are ignored now and were not, or the other way
    /// round: what they hold has to be read again.
    pub fn read(
        &mut self,
        folder: PathBuf,
        listing: Listing,
    ) -> Vec<PathBuf> {
        let before =
            self.entries(&folder).cloned().collect::<Vec<_>>();
        let turned = match &listing {
            Listing::Read(entries) => entries
                .iter()
                .filter(|entry| {
                    entry.is_folder()
                        && before.iter().any(|was| {
                            was.path == entry.path
                                && was.ignored != entry.ignored
                        })
                })
                .map(|entry| entry.path.clone())
                .collect(),
            Listing::Failed(_) => Vec::new(),
        };
        self.lists.insert(folder, listing);
        turned
    }

    /// Whether `folder` is ignored, as the listing of the folder it
    /// is in says.
    pub fn ignored(&self, folder: &Path) -> bool {
        let entries =
            match folder.parent().and_then(|p| self.lists.get(p)) {
                Some(Listing::Read(entries)) => entries,
                _ => return false,
            };
        entries
            .iter()
            .find(|entry| entry.path == folder)
            .is_some_and(|entry| entry.ignored)
    }

    /// The folders whose entries are shown: the root, and each open
    /// folder shown in another.
    pub fn shown(&self) -> HashSet<PathBuf> {
        let mut shown = HashSet::from([self.root.clone()]);
        let mut next = vec![self.root.clone()];
        while let Some(folder) = next.pop() {
            for entry in self.entries(&folder) {
                if entry.is_folder()
                    && self.open.contains(&entry.path)
                    && shown.insert(entry.path.clone())
                {
                    next.push(entry.path.clone());
                }
            }
        }
        shown
    }

    /// The lines shown, from the top.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        self.rows_of(&self.root, 0, &mut rows);
        if rows.is_empty() {
            rows.push(Row {
                depth: 0,
                line: Line::Empty,
            });
        }
        rows
    }

    /// The entries of `folder` that are shown, if it was read.
    fn entries(&self, folder: &Path) -> impl Iterator<Item = &Entry> {
        let entries = match self.lists.get(folder) {
            Some(Listing::Read(entries)) => entries.as_slice(),
            _ => &[],
        };
        entries.iter().take(SHOWN)
    }

    fn rows_of(
        &self,
        folder: &Path,
        depth: usize,
        rows: &mut Vec<Row>,
    ) {
        let mut push = |line| rows.push(Row { depth, line });
        let entries = match self.lists.get(folder) {
            None => return push(Line::Reading(folder.to_path_buf())),
            Some(Listing::Failed(why)) => {
                return push(Line::Failed(
                    folder.to_path_buf(),
                    why.clone(),
                ));
            }
            Some(Listing::Read(entries)) => entries,
        };
        for entry in self.entries(folder) {
            let open =
                entry.is_folder() && self.open.contains(&entry.path);
            rows.push(Row {
                depth,
                line: Line::Entry {
                    entry: entry.clone(),
                    open,
                    selected: self.selected.as_ref()
                        == Some(&entry.path),
                },
            });
            if open {
                self.rows_of(&entry.path, depth + 1, rows);
            }
        }
        if entries.len() > SHOWN {
            rows.push(Row {
                depth,
                line: Line::More(
                    folder.to_path_buf(),
                    entries.len() - SHOWN,
                ),
            });
        }
    }
}

/// The folders of `shown` a change at `path` may have changed: the
/// one it is in, itself if it is one, and every one under a git
/// ignore file that changed.
pub(crate) fn touched(
    shown: &HashSet<PathBuf>,
    path: &Path,
) -> Vec<PathBuf> {
    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    if path.file_name().is_some_and(|name| name == ".gitignore") {
        return shown
            .iter()
            .filter(|folder| folder.starts_with(parent))
            .cloned()
            .collect();
    }
    [path, parent]
        .into_iter()
        .filter(|folder| shown.contains(*folder))
        .map(Path::to_path_buf)
        .collect()
}

#[cfg(test)]
mod tests {
    use hamlet_desktop::files::Kind;

    use super::*;

    fn entry(path: &str, kind: Kind) -> Entry {
        let path = PathBuf::from(path);
        Entry {
            name: path.file_name().unwrap().to_string_lossy().into(),
            path,
            kind,
            ignored: false,
        }
    }

    /// A tree of `/w` with `src/main.rs`, `src/bin/` and `README.md`,
    /// each folder read.
    fn tree() -> Tree {
        let mut tree = Tree::new("/w".into());
        tree.read(
            "/w".into(),
            Listing::Read(vec![
                entry("/w/src", Kind::Folder),
                entry("/w/README.md", Kind::File),
            ]),
        );
        tree.read(
            "/w/src".into(),
            Listing::Read(vec![
                entry("/w/src/bin", Kind::Folder),
                entry("/w/src/main.rs", Kind::File),
            ]),
        );
        tree
    }

    /// Each row as its depth and a word for what it shows.
    fn drawn(tree: &Tree) -> Vec<(usize, String)> {
        tree.rows()
            .into_iter()
            .map(|row| {
                let word = match row.line {
                    Line::Entry {
                        entry,
                        open,
                        selected,
                    } => format!(
                        "{}{}{}",
                        entry.name,
                        if open { "/" } else { "" },
                        if selected { "*" } else { "" },
                    ),
                    Line::Reading(_) => "reading".into(),
                    Line::Failed(_, why) => why,
                    Line::More(_, n) => format!("+{n}"),
                    Line::Empty => "empty".into(),
                };
                (row.depth, word)
            })
            .collect()
    }

    fn rows(rows: &[(usize, &str)]) -> Vec<(usize, String)> {
        rows.iter().map(|(d, w)| (*d, w.to_string())).collect()
    }

    #[test]
    fn open_folders_are_drawn_inside_their_parents() {
        let mut tree = tree();
        assert_eq!(
            drawn(&tree),
            rows(&[(0, "src"), (0, "README.md")])
        );

        tree.reveal(Path::new("/w/src/main.rs"));
        tree.toggle(Path::new("/w/src/bin"));
        assert_eq!(
            drawn(&tree),
            rows(&[
                (0, "src/"),
                (1, "bin/"),
                (2, "reading"),
                (1, "main.rs*"),
                (0, "README.md"),
            ])
        );
        assert_eq!(
            tree.shown(),
            HashSet::from([
                "/w".into(),
                "/w/src".into(),
                "/w/src/bin".into()
            ])
        );

        // Closed, src keeps bin open inside it for when it opens.
        tree.toggle(Path::new("/w/src"));
        assert_eq!(
            drawn(&tree),
            rows(&[(0, "src"), (0, "README.md")])
        );
        assert_eq!(tree.shown(), HashSet::from(["/w".into()]));
        tree.toggle(Path::new("/w/src"));
        assert_eq!(tree.shown().len(), 3);

        tree.collapse();
        assert_eq!(
            drawn(&tree),
            rows(&[(0, "src"), (0, "README.md")])
        );
    }

    #[test]
    fn a_long_folder_is_cut_short_and_an_empty_root_says_so() {
        let mut tree = Tree::new("/w".into());
        assert_eq!(drawn(&tree), rows(&[(0, "reading")]));
        tree.read("/w".into(), Listing::Read(Vec::new()));
        assert_eq!(drawn(&tree), rows(&[(0, "empty")]));

        let many = (0..SHOWN + 3)
            .map(|n| entry(&format!("/w/{n}"), Kind::File))
            .collect();
        tree.read("/w".into(), Listing::Read(many));
        let drawn = drawn(&tree);
        assert_eq!(drawn.len(), SHOWN + 1);
        assert_eq!(drawn[SHOWN], (0, "+3".to_string()));
    }

    #[test]
    fn a_folder_newly_ignored_is_named_to_be_read_again() {
        let mut tree = tree();
        let mut ignored = entry("/w/src", Kind::Folder);
        ignored.ignored = true;
        let turned = tree.read(
            "/w".into(),
            Listing::Read(vec![
                ignored,
                entry("/w/README.md", Kind::File),
            ]),
        );
        assert_eq!(turned, [PathBuf::from("/w/src")]);
        assert!(tree.ignored(Path::new("/w/src")));
    }

    #[test]
    fn a_change_reaches_the_folders_it_may_have_changed() {
        let shown = HashSet::from([
            PathBuf::from("/w"),
            "/w/src".into(),
            "/w/src/bin".into(),
            "/w/docs".into(),
        ]);
        let touched = |path: &str| {
            let mut found = touched(&shown, Path::new(path));
            found.sort();
            found
        };

        assert_eq!(
            touched("/w/src/main.rs"),
            [PathBuf::from("/w/src")]
        );
        assert_eq!(
            touched("/w/src/bin"),
            [PathBuf::from("/w/src"), "/w/src/bin".into()]
        );
        assert_eq!(touched("/w/lib/x.rs"), Vec::<PathBuf>::new());
        assert_eq!(
            touched("/w/src/.gitignore"),
            [PathBuf::from("/w/src"), "/w/src/bin".into()]
        );
    }
}
