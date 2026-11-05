//! A lodge's copy in a remote workspace. The server's lodge holds
//! what git keeps; the remote `/lodge` is where the agent's tools
//! write during a turn. Both are kept against the files they last
//! held alike: a push sends what the server changed since, after
//! the server's changed; a pull brings home what the remote changed
//! since, after a tool may have written it. Neither undoes the
//! other's changes, and a first push, with nothing held alike yet,
//! makes the remote copy the server's.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use hamlet_workspace_wire::{
    InitLodge, MAX_FILE, Read, Remove, SetMode, TREE_BYTES,
    TREE_ENTRIES, TreeKind, TreeList, TreeWrite, Write,
};
use sha2::{Digest, Sha256};

use super::Session;
use crate::knowledge::folder::Folder;

/// Where an agent's lodge is in its workspace.
pub(crate) const LODGE: &str = "/lodge";

/// A regular file as one side has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct File {
    sha256: String,
    executable: bool,
}

/// A lodge's files as both copies last held them, by path.
pub(crate) type Synced = BTreeMap<String, File>;

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The remote lodge's regular files, by path.
async fn remote(session: &Session) -> Result<BTreeMap<String, File>> {
    session.call(InitLodge {}).await?;
    let listed = session
        .call(TreeList {
            path: LODGE.into(),
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

/// The server lodge's regular files with their bytes, by path.
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

/// Sends the remote copy what the server's lodge at `lodge` changed
/// since `base`, or all of it, replacing what is there, with no
/// `base`; gives what both hold alike after. A file past what one
/// request carries stays as it is there.
pub(crate) async fn push(
    session: &Session,
    lodge: &Path,
    base: Option<&Synced>,
) -> Result<Synced> {
    let folder = Folder::make(lodge)?;
    let theirs = remote(session).await?;
    let ours = local(&folder)?;
    let mut synced = base.cloned().unwrap_or_default();
    for (path, (bytes, executable)) in &ours {
        if bytes.len() > MAX_FILE {
            continue;
        }
        let file = File {
            sha256: sha256(bytes),
            executable: *executable,
        };
        // Unchanged here since: the remote's to change.
        if base.is_some_and(|base| base.get(path) == Some(&file)) {
            continue;
        }
        let at = format!("{LODGE}/{path}");
        let mode = if *executable { 0o755 } else { 0o644 };
        match theirs.get(path) {
            Some(held) if held.sha256 == file.sha256 => {
                if held.executable != *executable {
                    session.call(SetMode { path: at, mode }).await?;
                }
            }
            Some(_) => {
                session
                    .call(Write {
                        path: at.clone(),
                        data: hamlet_workspace_wire::Bytes(
                            bytes.clone(),
                        ),
                        append: false,
                        parents: true,
                        no_follow: true,
                    })
                    .await?;
                session.call(SetMode { path: at, mode }).await?;
            }
            None => {
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
                path: format!("{LODGE}/{path}"),
                recursive: false,
                force: true,
                no_follow: true,
            })
            .await?;
        synced.remove(&path);
    }
    Ok(synced)
}

/// What a pull brings home: each path's new bytes and whether it
/// runs, or nothing for a path gone.
pub(crate) struct Pulled(Vec<(String, Option<(Vec<u8>, bool)>)>);

impl Pulled {
    /// Writes what was pulled into the lodge at `lodge`.
    pub fn apply(self, lodge: &Path) -> Result<()> {
        let folder = Folder::make(lodge)?;
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

/// What the remote copy changed since `base`, which the server's
/// lodge takes in, and what both hold alike after.
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
                path: format!("{LODGE}/{path}"),
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
