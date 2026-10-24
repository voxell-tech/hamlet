//! Verified copies of house trees, including empty folders and
//! ordinary modes: one snapshot read and one tree write for every
//! workspace.

use std::collections::BTreeMap;

use anyhow::{Result, bail, ensure};
use hamlet_workspace_wire::{
    TREE_BYTES, TREE_ENTRIES, TreeEntry, TreeKind,
};

use super::files::{Files, has_bytes};
use super::tree::Entry;

/// Links and special files met while copying a tree that refuses
/// them.
pub(crate) const UNSAFE: &str =
    "A skill gift cannot include links or special files.";

/// A copy stopped for a reason the chief is told in these words.
#[derive(Debug)]
pub(crate) struct Stopped(pub &'static str);

impl std::fmt::Display for Stopped {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Stopped {}

pub(crate) struct Snapshot {
    /// The tree's folders and files, `.` first and each folder
    /// before what is in it.
    pub entries: Vec<TreeEntry>,
    pub files: BTreeMap<String, Entry>,
    /// The links and special files left out.
    pub skipped: Vec<String>,
}

impl Snapshot {
    /// The tree of `source`, within `cap` bytes and `count` entries.
    /// Links and special files are refused, or left out with `skip`.
    pub async fn read(
        source: &Files,
        cap: u64,
        count: usize,
        skip: bool,
    ) -> Result<Self> {
        let mut result = Self {
            entries: Vec::new(),
            files: BTreeMap::new(),
            skipped: Vec::new(),
        };
        for entry in source.entries(cap, count).await? {
            match entry.kind {
                TreeKind::Directory => {}
                TreeKind::File => {
                    let bytes =
                        source.read_file(&entry.path, cap).await?;
                    ensure!(
                        has_bytes(&entry, &bytes),
                        "The tree changed while it was copied. \
                         Inspect it before retrying."
                    );
                    result.files.insert(
                        entry.path.clone(),
                        Entry {
                            bytes,
                            mode: entry.mode & 0o777,
                        },
                    );
                }
                TreeKind::Link | TreeKind::Special => {
                    if !skip {
                        bail!(Stopped(UNSAFE));
                    }
                    result.skipped.push(entry.path);
                    continue;
                }
            }
            result.entries.push(entry);
        }
        result.entries.sort_by_key(|entry| {
            (
                entry.path != ".",
                entry.path.split('/').count(),
                entry.path.clone(),
            )
        });
        Ok(result)
    }

    /// The same tree as the folder `name` of a tree.
    pub fn under(self, name: &str) -> Self {
        let at = |path: &str| {
            if path == "." {
                name.to_owned()
            } else {
                format!("{name}/{path}")
            }
        };
        let mut entries = vec![folder(".")];
        entries.extend(self.entries.into_iter().map(|entry| {
            TreeEntry {
                path: at(&entry.path),
                ..entry
            }
        }));
        Self {
            entries,
            files: self
                .files
                .into_iter()
                .map(|(path, entry)| (at(&path), entry))
                .collect(),
            skipped: self
                .skipped
                .iter()
                .map(|path| at(path))
                .collect(),
        }
    }

    /// A snapshot of `files` under `.`, with the folders they need.
    pub fn of(files: BTreeMap<String, Entry>) -> Self {
        let mut folders = std::collections::BTreeSet::new();
        for path in files.keys() {
            let mut parts = path.split('/').collect::<Vec<_>>();
            parts.pop();
            while !parts.is_empty() {
                folders.insert(parts.join("/"));
                parts.pop();
            }
        }
        let mut entries = vec![folder(".")];
        entries.extend(folders.iter().map(|path| folder(path)));
        entries.extend(files.iter().map(|(path, entry)| TreeEntry {
            path: path.clone(),
            kind: TreeKind::File,
            mode: entry.mode,
            size: entry.bytes.len() as u64,
            sha256: None,
        }));
        entries.sort_by_key(|entry| {
            (
                entry.path != ".",
                entry.path.split('/').count(),
                entry.path.clone(),
            )
        });
        Self {
            entries,
            files,
            skipped: Vec::new(),
        }
    }

    /// Delivers the trees `roots` of the snapshot into `shelf`, each
    /// under a free name: its own, else numbered with `number`. Gives
    /// the names used.
    pub async fn deliver(
        &self,
        shelf: &Files,
        roots: &[String],
        number: bool,
    ) -> Result<Vec<String>> {
        let mut targets = Vec::new();
        for root in roots {
            let mut chosen = root.clone();
            let mut count = 1;
            while shelf.kind(&chosen).await?.is_some() {
                ensure!(number, "gift target already exists");
                count += 1;
                let split = root
                    .rfind('.')
                    .filter(|i| *i > 0)
                    .unwrap_or(root.len());
                chosen = format!(
                    "{}-{count}{}",
                    &root[..split],
                    &root[split..]
                );
            }
            ensure!(
                !targets.contains(&chosen),
                "Gift names collide after numbering; no files were \
                 delivered."
            );
            targets.push(chosen);
        }
        let mapped = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (name, rest) = entry
                    .path
                    .split_once('/')
                    .unwrap_or((&entry.path, ""));
                let index =
                    roots.iter().position(|root| root == name)?;
                let path = if rest.is_empty() {
                    targets[index].clone()
                } else {
                    format!("{}/{rest}", targets[index])
                };
                Some((entry, path))
            })
            .collect::<Vec<_>>();
        self.write(shelf, &mapped).await?;
        for (root, chosen) in roots.iter().zip(&targets) {
            let expected = mapped
                .iter()
                .filter(|(entry, _)| {
                    entry.path == *root
                        || entry.path.starts_with(&format!("{root}/"))
                })
                .collect::<Vec<_>>();
            let fine = match expected.first() {
                Some((entry, _)) if entry.kind == TreeKind::File => {
                    let bytes =
                        shelf.read_file(chosen, entry.size).await?;
                    bytes == self.files[&entry.path].bytes
                }
                _ => {
                    let actual = shelf
                        .directory(chosen, false)
                        .await?
                        .entries(TREE_BYTES, expected.len() + 1)
                        .await?;
                    actual.len() == expected.len()
                        && expected.iter().all(|(entry, path)| {
                            let relative = path
                                .strip_prefix(&format!("{chosen}/"))
                                .unwrap_or(".");
                            actual.iter().any(|actual| {
                                actual.path == relative
                                    && is_same(
                                        actual,
                                        entry,
                                        &self.files,
                                    )
                            })
                        })
                }
            };
            if !fine {
                bail!(Stopped(
                    "The delivery changed before verification. \
                     Inspect the target before repeating it."
                ));
            }
        }
        Ok(targets)
    }

    /// Writes each entry of `mapped` at its path in `target`, which
    /// must be free, then gives the folders their modes. On failure
    /// it takes back what it made.
    pub async fn write(
        &self,
        target: &Files,
        mapped: &[(&TreeEntry, String)],
    ) -> Result<()> {
        let mut claimed = Vec::new();
        let mut written = Ok(());
        for (entry, path) in mapped {
            written = if entry.kind == TreeKind::Directory {
                target.mkdir(path).await
            } else {
                target.put(path, &self.files[&entry.path]).await
            };
            if written.is_err() {
                break;
            }
            claimed.push(path);
        }
        if let Err(error) = written {
            let mut complete = true;
            for path in claimed.into_iter().rev() {
                complete &= target.remove(path, false).await.is_ok();
            }
            if crate::workspace::is_uncertain(&error) {
                bail!(Stopped(
                    "The workspace did not confirm the tree \
                     operation, so part of it may have happened. \
                     Inspect the target before repeating it."
                ));
            }
            if !complete {
                bail!(Stopped(
                    "The tree operation stopped after changing part \
                     of its target. Inspect the target and keep or \
                     remove what is there before repeating it."
                ));
            }
            return Err(error);
        }
        for (entry, path) in mapped
            .iter()
            .rev()
            .filter(|(entry, _)| entry.kind == TreeKind::Directory)
        {
            target.set_mode(path, entry.mode).await?;
        }
        Ok(())
    }

    /// Makes `house` hold the snapshot: removes what differs, writes
    /// what is missing and checks the result.
    pub async fn replace(&self, house: &Files) -> Result<()> {
        let listed =
            house.entries(TREE_BYTES, TREE_ENTRIES as usize).await?;
        let mut kept = std::collections::BTreeSet::new();
        let mut removed = Vec::<String>::new();
        for entry in listed {
            if entry.path == "."
                || removed.iter().any(|parent| {
                    entry.path.starts_with(&format!("{parent}/"))
                })
            {
                continue;
            }
            if self.entries.iter().any(|wanted| {
                wanted.path == entry.path
                    && (is_same(&entry, wanted, &self.files)
                        || wanted.kind == TreeKind::Directory
                            && entry.kind == TreeKind::Directory)
            }) {
                kept.insert(entry.path);
            } else {
                house.remove(&entry.path, true).await?;
                removed.push(entry.path);
            }
        }
        let mapped = self
            .entries
            .iter()
            .filter(|entry| {
                entry.path != "." && !kept.contains(&entry.path)
            })
            .map(|entry| (entry, entry.path.clone()))
            .collect::<Vec<_>>();
        self.write(house, &mapped).await?;
        let after =
            house.entries(TREE_BYTES, TREE_ENTRIES as usize).await?;
        ensure!(
            after.len() == self.entries.len()
                && self.entries.iter().all(|entry| after.iter().any(
                    |actual| actual.path == entry.path
                        && (entry.path == "."
                            && actual.kind == TreeKind::Directory
                            || is_same(actual, entry, &self.files))
                )),
            "The copy did not match it afterwards."
        );
        Ok(())
    }
}

fn folder(path: &str) -> TreeEntry {
    TreeEntry {
        path: path.into(),
        kind: TreeKind::Directory,
        mode: 0o700,
        size: 0,
        sha256: None,
    }
}

/// Whether the listed `actual` is the snapshot's `wanted`, whose
/// bytes are in `files`.
fn is_same(
    actual: &TreeEntry,
    wanted: &TreeEntry,
    files: &BTreeMap<String, Entry>,
) -> bool {
    actual.kind == wanted.kind
        && actual.mode & 0o777 == wanted.mode & 0o777
        && (actual.kind != TreeKind::File
            || files
                .get(&wanted.path)
                .is_some_and(|entry| has_bytes(actual, &entry.bytes)))
}
