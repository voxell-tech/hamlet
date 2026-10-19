//! House trees on the selected backend; local stages and the library
//! remain server-owned.

use std::collections::BTreeMap;
use std::fs::File;

use anyhow::{Result, ensure};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::tree::{self, Entry};
use crate::sandbox::FileKind;
use crate::workspace::Remote;

pub(crate) enum Files {
    Local(File),
    Remote { env: Remote, path: String },
}

#[derive(Deserialize)]
pub(crate) struct TreeEntry {
    pub path: String,
    pub kind: String,
    pub mode: u32,
    pub size: u64,
    pub sha256: Option<String>,
}

fn valid(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|part| {
            !part.is_empty()
                && ![".", ".."].contains(&part)
                && !part.contains(['\\', '\0'])
        })
}

fn file_error(error: crate::sandbox::FileError) -> anyhow::Error {
    anyhow::anyhow!("{}", error.message)
}

impl Files {
    pub async fn directory(
        &self,
        name: &str,
        create: bool,
    ) -> Result<Self> {
        ensure!(
            valid(name) && !name.contains('/'),
            "invalid tree directory"
        );
        match self {
            Self::Local(root) => {
                tree::directory(root, name, create).map(Self::Local)
            }
            Self::Remote { env, path } => {
                let path = format!("{path}/{name}");
                env.parents(&path, false)
                    .await
                    .map_err(file_error)?;
                if create
                    && !env.exists(&path).await.map_err(file_error)?
                {
                    env.call::<()>(json!({"kind":"mkdir", "path":path,"recursive":false})).await?;
                }
                ensure!(
                    env.info(&path).await.map_err(file_error)?.kind
                        == FileKind::Directory,
                    "The house directory is not safe to open."
                );
                Ok(Self::Remote {
                    env: env.clone(),
                    path,
                })
            }
        }
    }

    pub async fn present(&self, name: &str) -> Result<bool> {
        ensure!(valid(name), "invalid tree path");
        match self {
            Self::Local(root) => {
                match rustix::fs::statat(
                    root,
                    name,
                    rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
                ) {
                    Ok(_) => Ok(true),
                    Err(rustix::io::Errno::NOENT) => Ok(false),
                    Err(error) => Err(error.into()),
                }
            }
            Self::Remote { env, path } => env
                .exists(&format!("{path}/{name}"))
                .await
                .map_err(file_error),
        }
    }

    pub async fn read(
        &self,
        cap: u64,
        count: usize,
    ) -> Result<BTreeMap<String, Entry>> {
        let Self::Remote { env, path } = self else {
            let Self::Local(root) = self else {
                unreachable!()
            };
            return tree::read(root, cap, count);
        };
        let entries = remote_entries(env, path, cap, count).await?;
        ensure!(
            entries.iter().any(|entry| entry.path == "."
                && entry.kind == "directory"),
            "The tree is not a directory."
        );
        let mut files = BTreeMap::new();
        for entry in entries {
            match entry.kind.as_str() {
                "directory" => {}
                "file" => {
                    let bytes = env
                        .read(&format!("{path}/{}", entry.path), true)
                        .await
                        .map_err(file_error)?;
                    ensure!(
                        bytes.len() as u64 == entry.size
                            && entry.sha256.as_deref()
                                == Some(&format!(
                                    "{:x}",
                                    Sha256::digest(&bytes)
                                )),
                        "The remote tree changed while it was read. \
                         Inspect it before retrying."
                    );
                    ensure!(
                        files
                            .insert(
                                entry.path,
                                Entry {
                                    bytes,
                                    mode: entry.mode & 0o777
                                }
                            )
                            .is_none(),
                        "Duplicate remote tree entry."
                    );
                }
                _ => anyhow::bail!(
                    "Links and special files cannot be copied."
                ),
            }
        }
        Ok(files)
    }

    pub async fn put(&self, path: &str, entry: &Entry) -> Result<()> {
        ensure!(valid(path), "invalid tree path");
        match self {
            Self::Local(root) => tree::put(root, path, entry),
            Self::Remote { env, path: root } => {
                ensure!(
                    entry.bytes.len() <= crate::workspace::MAX_FILE,
                    "Remote files are limited to 4 MiB per transfer."
                );
                let full = format!("{root}/{path}");
                env.parents(&full, true).await.map_err(file_error)?;
                match env.read(&full, true).await {
                    Ok(bytes) => {
                        ensure!(
                            bytes == entry.bytes,
                            "The merge target changed; inspect the \
                             houses before retrying."
                        );
                        return Ok(());
                    }
                    Err(error) if error.code == "not_found" => {}
                    Err(error) => return Err(file_error(error)),
                }
                env.call::<()>(json!({"kind":"tree_write", "path":full,"data":STANDARD.encode(&entry.bytes),"mode":entry.mode & 0o777})).await?;
                Ok(())
            }
        }
    }

    pub async fn replace(
        &self,
        path: &str,
        bytes: &[u8],
    ) -> Result<()> {
        ensure!(valid(path), "invalid tree path");
        match self {
            Self::Local(root) => tree::replace(root, path, bytes),
            Self::Remote { env, path: root } => {
                let full = format!("{root}/{path}");
                env.parents(&full, false)
                    .await
                    .map_err(file_error)?;
                ensure!(
                    env.info(&full).await.map_err(file_error)?.kind
                        == FileKind::File,
                    "The notes are not a regular file."
                );
                ensure!(
                    bytes.len() <= crate::workspace::MAX_FILE,
                    "Remote files are limited to 4 MiB per transfer."
                );
                env.call::<()>(json!({"kind":"write","path":full,"data":STANDARD.encode(bytes),"append":false})).await?;
                Ok(())
            }
        }
    }

    pub async fn remove(
        &self,
        path: &str,
        recursive: bool,
    ) -> Result<()> {
        ensure!(valid(path), "invalid tree path");
        match self {
            Self::Local(root) => {
                if recursive {
                    tree::erase(root, path)
                } else {
                    tree::remove(root, path)
                }
            }
            Self::Remote { env, path: root } => {
                env.parents(&format!("{root}/{path}"), false)
                    .await
                    .map_err(file_error)?;
                env.call::<()>(json!({"kind":"remove", "path":format!("{root}/{path}"),"recursive":recursive,"force":false})).await?;
                Ok(())
            }
        }
    }
}

pub(crate) async fn remote_entries(
    env: &Remote,
    path: &str,
    cap: u64,
    count: usize,
) -> Result<Vec<TreeEntry>> {
    env.parents(path, false).await.map_err(file_error)?;
    let entries = env.call::<Vec<TreeEntry>>(json!({"kind":"tree_list", "path":path,"entries":count,"bytes":cap})).await?;
    ensure!(
        entries.len() <= count,
        "The remote tree has too many entries."
    );
    let mut left = cap;
    let mut names = BTreeMap::new();
    for entry in &entries {
        ensure!(
            (entry.path == "." || valid(&entry.path))
                && names
                    .insert(entry.path.as_str(), entry.kind.as_str())
                    .is_none(),
            "Invalid remote tree path."
        );
        ensure!(
            ["file", "directory", "link", "special"]
                .contains(&entry.kind.as_str()),
            "Invalid remote tree kind."
        );
        if entry.kind == "file" {
            left = left.checked_sub(entry.size).ok_or_else(|| {
                anyhow::anyhow!("The remote tree is too large.")
            })?;
            ensure!(
                entry.size <= crate::workspace::MAX_FILE as u64,
                "Remote files are limited to 4 MiB per transfer."
            );
        }
    }
    ensure!(names.contains_key("."), "Missing remote tree root.");
    for entry in entries.iter().filter(|entry| entry.path != ".") {
        let parent = entry
            .path
            .rsplit_once('/')
            .map_or(".", |(parent, _)| parent);
        ensure!(
            names.get(parent) == Some(&"directory"),
            "Invalid remote tree parent."
        );
    }
    Ok(entries)
}
