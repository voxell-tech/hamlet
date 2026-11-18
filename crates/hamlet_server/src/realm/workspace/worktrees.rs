//! A worktree's copy in a remote workspace. The server's worktree
//! holds what git keeps; the remote `/worktree` is where the agent's
//! tools write during a turn. Both are kept against the files they
//! last held alike: a push sends what the server changed since, after
//! the server's changed; a pull brings home what the remote changed
//! since, after a tool may have written it. Neither undoes the
//! other's changes, and a first push, with nothing held alike yet,
//! makes the remote copy the server's.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use hamlet_workspace_wire::{
    InitWorktree, MAX_FILE, Read, Remove, SetMode, TREE_BYTES,
    TREE_ENTRIES, TreeKind, TreeList, TreeWrite, Write,
};
use sha2::{Digest, Sha256};

use super::Session;
use crate::knowledge::folder::Folder;

/// The path of an agent's worktree in its workspace.
pub(crate) const WORKTREE: &str = "/worktree";

/// A regular file as one side has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct File {
    sha256: String,
    executable: bool,
}

/// A worktree's files as both copies last held them, by path.
pub(crate) type Synced = BTreeMap<String, File>;

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The remote worktree's regular files, by path.
async fn remote(session: &Session) -> Result<BTreeMap<String, File>> {
    session.call(InitWorktree {}).await?;
    let listed = session
        .call(TreeList {
            path: WORKTREE.into(),
            entries: TREE_ENTRIES,
            bytes: TREE_BYTES,
        })
        .await?;
    Ok(listed
        .into_iter()
        .filter(|entry| entry.kind == TreeKind::File)
        .filter_map(|entry| {
            Some((
                entry.path,
                File {
                    sha256: entry.sha256?,
                    executable: entry.mode & 0o100 != 0,
                },
            ))
        })
        .collect())
}

/// The server worktree's regular files with their bytes, by path.
fn local(
    folder: &Folder,
) -> Result<BTreeMap<String, (Vec<u8>, bool)>> {
    folder
        .list(&|_| false)?
        .into_iter()
        .map(|entry| {
            Ok((
                entry.path.clone(),
                (folder.read(&entry.path)?, entry.executable),
            ))
        })
        .collect()
}

/// Writes `bytes` at `path` of the remote workspace, in pieces of at
/// most `MAX_FILE` bytes, which is all one request carries.
async fn write_pieces(
    session: &Session,
    path: &str,
    bytes: &[u8],
) -> Result<()> {
    let pieces = if bytes.is_empty() {
        vec![bytes]
    } else {
        bytes.chunks(MAX_FILE).collect()
    };
    for (piece, bytes) in pieces.into_iter().enumerate() {
        session
            .call(Write {
                path: path.to_owned(),
                data: hamlet_workspace_wire::Bytes(bytes.to_vec()),
                append: piece > 0,
                parents: true,
                no_follow: true,
            })
            .await?;
    }
    Ok(())
}

/// Sends the remote copy what the server's worktree at `worktree`
/// changed since `base`, or all of it, replacing what is there, with
/// no `base`; gives what both hold alike after. A file past what one
/// request carries goes in pieces.
pub(crate) async fn push(
    session: &Session,
    worktree: &Path,
    base: Option<&Synced>,
) -> Result<Synced> {
    let folder = Folder::make(worktree)?;
    let theirs = remote(session).await?;
    let ours = local(&folder)?;
    let mut synced = base.cloned().unwrap_or_default();
    for (path, (bytes, executable)) in &ours {
        let file = File {
            sha256: sha256(bytes),
            executable: *executable,
        };
        // Unchanged here since: the remote's to change.
        if base.is_some_and(|base| base.get(path) == Some(&file)) {
            continue;
        }
        let at = format!("{WORKTREE}/{path}");
        let mode = if *executable { 0o755 } else { 0o644 };
        match theirs.get(path) {
            Some(held) if held.sha256 == file.sha256 => {
                if held.executable != *executable {
                    session.call(SetMode { path: at, mode }).await?;
                }
            }
            held if held.is_some() || bytes.len() > MAX_FILE => {
                write_pieces(session, &at, bytes).await?;
                session.call(SetMode { path: at, mode }).await?;
            }
            _ => {
                session
                    .call(TreeWrite {
                        path: at,
                        data: hamlet_workspace_wire::Bytes(
                            bytes.clone(),
                        ),
                        mode,
                    })
                    .await?;
            }
        }
        synced.insert(path.clone(), file);
    }
    // What the server removed since, or all it lacks on a first push.
    let gone = match base {
        Some(base) => base
            .keys()
            .filter(|path| !ours.contains_key(*path))
            .cloned()
            .collect::<Vec<_>>(),
        None => theirs
            .keys()
            .filter(|path| !ours.contains_key(*path))
            .cloned()
            .collect(),
    };
    for path in gone {
        session
            .call(Remove {
                path: format!("{WORKTREE}/{path}"),
                recursive: false,
                force: true,
                no_follow: true,
            })
            .await?;
        synced.remove(&path);
    }
    Ok(synced)
}

/// A pull's result: each path's new bytes and whether it runs, or
/// nothing for a path gone.
pub(crate) struct Pulled(Vec<(String, Option<(Vec<u8>, bool)>)>);

impl Pulled {
    /// Writes what was pulled into the worktree at `worktree`.
    pub fn apply(self, worktree: &Path) -> Result<()> {
        let folder = Folder::make(worktree)?;
        for (path, file) in self.0 {
            match file {
                Some((bytes, executable)) => {
                    folder.write(&path, &bytes, executable)?;
                }
                None => folder.remove(&path)?,
            }
        }
        Ok(())
    }
}

/// The changes of the remote copy since `base`, which the server's
/// worktree takes in, and what both hold alike after.
pub(crate) async fn pull(
    session: &Session,
    base: &Synced,
) -> Result<(Pulled, Synced)> {
    let theirs = remote(session).await?;
    let mut synced = base.clone();
    let mut pulled = Vec::new();
    for (path, file) in &theirs {
        if base.get(path) == Some(file) {
            continue;
        }
        let read = session
            .call(Read {
                path: format!("{WORKTREE}/{path}"),
                no_follow: true,
            })
            .await?;
        if read.data.0.len() > MAX_FILE {
            continue;
        }
        pulled.push((
            path.clone(),
            Some((read.data.0, file.executable)),
        ));
        synced.insert(path.clone(), file.clone());
    }
    for path in base.keys().filter(|path| !theirs.contains_key(*path))
    {
        pulled.push((path.clone(), None));
        synced.remove(path);
    }
    Ok((Pulled(pulled), synced))
}
