//! An agent's filesystem copied to a remote workspace: a push makes
//! the remote `/lodge` hold what the server's copy holds.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use hamlet_workspace_wire::{
    InitLodge, MAX_FILE, Remove, SetMode, TREE_BYTES, TREE_ENTRIES,
    TreeKind, TreeList, TreeWrite, Write,
};
use sha2::{Digest, Sha256};

use super::Session;
use crate::folder::Folder;

/// Where an agent's filesystem is in its workspace.
pub(crate) const LODGE: &str = "/lodge";

/// A regular file as the remote copy has it.
struct File {
    sha256: String,
    executable: bool,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The remote copy's regular files, by path.
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

/// The agent's filesystem's regular files with their bytes, by path.
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

/// Makes the remote copy hold what the agent's filesystem at
/// `filesystem` holds: what differs is written and what it lacks is
/// removed. A file past what one request carries stays as it is
/// there.
pub(crate) async fn push(
    session: &Session,
    filesystem: &Path,
) -> Result<()> {
    let folder = Folder::make(filesystem)?;
    let theirs = remote(session).await?;
    let ours = local(&folder)?;
    for (path, (bytes, executable)) in &ours {
        if bytes.len() > MAX_FILE {
            continue;
        }
        let at = format!("{LODGE}/{path}");
        let mode = if *executable { 0o755 } else { 0o644 };
        match theirs.get(path) {
            Some(held) if held.sha256 == sha256(bytes) => {
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
    }
    for path in theirs.keys().filter(|path| !ours.contains_key(*path))
    {
        session
            .call(Remove {
                path: format!("{LODGE}/{path}"),
                recursive: false,
                force: true,
                no_follow: true,
            })
            .await?;
    }
    Ok(())
}
