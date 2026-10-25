//! A folder of house files on the startup-selected workspace, or one
//! the server owns. Local work runs off the async runtime.

use std::collections::BTreeMap;

use anyhow::{Result, ensure};
use hamlet_os::fs::{Directory, Missing, Mode, Name, TreePath};
use hamlet_workspace_wire::{TreeEntry, TreeKind};
use sha2::{Digest, Sha256};

use super::remote::Folder;
use super::tree::{self, Entry};

pub(crate) enum Files {
    Local(Directory),
    Remote(Folder),
}

async fn blocking<T: Send + 'static>(
    dir: &Directory,
    work: impl FnOnce(&Directory) -> Result<T> + Send + 'static,
) -> Result<T> {
    let dir = dir.try_clone()?;
    tokio::task::spawn_blocking(move || work(&dir)).await?
}

impl Files {
    pub async fn directory(
        &self,
        name: &str,
        create: bool,
    ) -> Result<Self> {
        ensure!(
            Name::in_tree(name).is_ok(),
            "invalid tree directory"
        );
        let name = name.to_owned();
        let missing = if create {
            Missing::Make(Mode::FOLDER)
        } else {
            Missing::Fail
        };
        match self {
            Self::Local(root) => blocking(root, move |root| {
                Ok(tree::directory(root, &name, missing)?)
            })
            .await
            .map(Self::Local),
            Self::Remote(folder) => folder
                .directory(&name, create)
                .await
                .map(Self::Remote),
        }
    }

    /// What `path` names, a link itself rather than what it names,
    /// or `None` where nothing is.
    pub async fn kind(&self, path: &str) -> Result<Option<TreeKind>> {
        let tree_path = TreePath::parse(path)?;
        match self {
            Self::Local(root) => {
                blocking(root, move |root| {
                    tree::kind(root, &tree_path)
                })
                .await
            }
            Self::Remote(folder) => folder.kind(path).await,
        }
    }

    /// Every entry of the tree, `.` first, within `cap` bytes and
    /// `count` entries.
    pub async fn entries(
        &self,
        cap: u64,
        count: usize,
    ) -> Result<Vec<TreeEntry>> {
        match self {
            Self::Local(root) => {
                blocking(root, move |root| {
                    tree::entries(root, cap, count)
                })
                .await
            }
            Self::Remote(folder) => folder.entries(cap, count).await,
        }
    }

    /// The regular file `path`, at most `cap` bytes.
    pub async fn read_file(
        &self,
        path: &str,
        cap: u64,
    ) -> Result<Vec<u8>> {
        let tree_path = TreePath::parse(path)?;
        match self {
            Self::Local(root) => {
                blocking(root, move |root| {
                    tree::read_file(root, &tree_path, cap)
                })
                .await
            }
            Self::Remote(folder) => folder.read_file(path, cap).await,
        }
    }

    /// Writes a new file with its mode, making missing folders. A
    /// file already there must hold the same bytes.
    pub async fn put(&self, path: &str, entry: &Entry) -> Result<()> {
        let tree_path = TreePath::parse(path)?;
        match self {
            Self::Local(root) => {
                let entry = entry.clone();
                blocking(root, move |root| {
                    tree::put(root, &tree_path, &entry)
                })
                .await
            }
            Self::Remote(folder) => folder.put(path, entry).await,
        }
    }

    /// Makes the new folder `path` in an existing one.
    pub async fn mkdir(&self, path: &str) -> Result<()> {
        let tree_path = TreePath::parse(path)?;
        match self {
            Self::Local(root) => {
                blocking(root, move |root| {
                    tree::mkdir(root, &tree_path)
                })
                .await
            }
            Self::Remote(folder) => folder.mkdir(path).await,
        }
    }

    /// Sets the mode of `path`, or of this folder for `.`.
    pub async fn set_mode(
        &self,
        path: &str,
        mode: u32,
    ) -> Result<()> {
        let tree_path = match path {
            "." => None,
            path => Some(TreePath::parse(path)?),
        };
        match self {
            Self::Local(root) => {
                blocking(root, move |root| match &tree_path {
                    Some(path) => tree::set_mode(root, path, mode),
                    None => Ok(root.set_mode(Mode::new(mode))?),
                })
                .await
            }
            Self::Remote(folder) => folder.set_mode(path, mode).await,
        }
    }

    /// Writes `bytes` over the regular file `path`, or makes it where
    /// it is missing.
    pub async fn replace(
        &self,
        path: &str,
        bytes: &[u8],
    ) -> Result<()> {
        let tree_path = TreePath::parse(path)?;
        match self {
            Self::Local(root) => {
                let bytes = bytes.to_vec();
                blocking(root, move |root| {
                    tree::replace(root, &tree_path, &bytes)
                })
                .await
            }
            Self::Remote(folder) => folder.replace(path, bytes).await,
        }
    }

    /// Removes the file or empty folder `path`, or with `recursive`
    /// the folder and all in it.
    pub async fn remove(
        &self,
        path: &str,
        recursive: bool,
    ) -> Result<()> {
        let tree_path = TreePath::parse(path)?;
        match self {
            Self::Local(root) => {
                blocking(root, move |root| {
                    if recursive {
                        tree::erase_path(root, &tree_path)
                    } else {
                        tree::remove(root, &tree_path)
                    }
                })
                .await
            }
            Self::Remote(folder) => {
                folder.remove(path, recursive).await
            }
        }
    }

    /// The regular files of the tree, refusing links and special
    /// files, within `cap` bytes and `count` entries.
    pub async fn read(
        &self,
        cap: u64,
        count: usize,
    ) -> Result<BTreeMap<String, Entry>> {
        let mut files = BTreeMap::new();
        for entry in self.entries(cap, count).await? {
            match entry.kind {
                TreeKind::Directory => {}
                TreeKind::File => {
                    let bytes =
                        self.read_file(&entry.path, cap).await?;
                    ensure!(
                        has_bytes(&entry, &bytes),
                        "The tree changed while it was read. \
                         Inspect it before retrying."
                    );
                    files.insert(
                        entry.path,
                        Entry {
                            bytes,
                            mode: entry.mode & 0o777,
                        },
                    );
                }
                _ => anyhow::bail!(
                    "Links and special files cannot be merged."
                ),
            }
        }
        Ok(files)
    }
}

/// Whether `bytes` are the file `entry` listed.
pub(super) fn has_bytes(entry: &TreeEntry, bytes: &[u8]) -> bool {
    entry.size == bytes.len() as u64
        && entry.sha256.as_deref()
            == Some(format!("{:x}", Sha256::digest(bytes)).as_str())
}
