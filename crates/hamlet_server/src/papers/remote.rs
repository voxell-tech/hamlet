//! House trees in a remote workspace, reached without following
//! links.

use std::collections::BTreeMap;

use anyhow::{Result, bail, ensure};
use hamlet_workspace_wire::{
    Bytes, Code, Info, InitHouse, Kind, List, MAX_FILE, Mkdir, Read,
    Remove, SetMode, TreeEntry, TreeKind, TreeList, TreeWrite, Write,
};

use super::tree::Entry;
use crate::prompt::shelf::{self, Skill};
use crate::workspace::{self, WorkspaceSession};

/// Where tools see the house, the hamlet's folder and the library.
pub(crate) const HOUSE: &str = "/house";
pub(crate) const PROJECT: &str = "/project";
pub(crate) const LIBRARY: &str = "/library";

/// The machine a remote workspace's tools run on.
pub(crate) const MACHINE: &str =
    "# Environment\nSandbox: remote workspace. System: Linux. \
     Network is denied unless granted. Granted commands use \
     HTTP_PROXY and HTTPS_PROXY for public HTTP(S) on ports 80 and \
     443; direct sockets, private destinations and UDP remain \
     blocked. Commands write only this agent's /house and the \
     enabled shared /project. Output is buffered until the process \
     tree exits, at most 1 MiB; file transfers are limited to 4 MiB.";

/// The refusal of a file too large for one request.
#[derive(Debug, thiserror::Error)]
#[error("Remote files are limited to 4 MiB per transfer.")]
pub(crate) struct TooLarge;

/// A folder of a remote house.
#[derive(Clone)]
pub(crate) struct Folder {
    session: WorkspaceSession,
    path: String,
}

impl Folder {
    /// The house of `session`'s agent.
    pub fn house(session: WorkspaceSession) -> Self {
        Self {
            session,
            path: HOUSE.into(),
        }
    }

    fn at(&self, path: &str) -> String {
        if path == "." {
            self.path.clone()
        } else {
            format!("{}/{path}", self.path)
        }
    }

    pub async fn initialize(&self) -> Result<()> {
        Ok(self.session.call(InitHouse {}).await?)
    }

    pub async fn directory(
        &self,
        name: &str,
        create: bool,
    ) -> Result<Self> {
        let path = self.at(name);
        if create {
            let made = self
                .session
                .call(Mkdir {
                    path: path.clone(),
                    recursive: false,
                    no_follow: true,
                })
                .await;
            match made {
                Err(failure)
                    if failure.code == Code::AlreadyExists => {}
                made => made?,
            }
        }
        let info = self
            .session
            .call(Info {
                path: path.clone(),
                no_follow: true,
            })
            .await?;
        ensure!(
            info.kind == Kind::Directory,
            "The house directory is not safe to open."
        );
        Ok(Self {
            session: self.session.clone(),
            path,
        })
    }

    pub async fn kind(&self, path: &str) -> Result<Option<TreeKind>> {
        let info = Info {
            path: self.at(path),
            no_follow: true,
        };
        match self.session.call(info).await {
            Ok(info) => Ok(Some(match info.kind {
                Kind::File => TreeKind::File,
                Kind::Directory => TreeKind::Directory,
                Kind::Symlink => TreeKind::Link,
            })),
            Err(failure) if failure.code == Code::NotFound => {
                Ok(None)
            }
            Err(failure) if failure.code == Code::NotSupported => {
                Ok(Some(TreeKind::Special))
            }
            Err(failure) => Err(failure.into()),
        }
    }

    pub async fn entries(
        &self,
        cap: u64,
        count: usize,
    ) -> Result<Vec<TreeEntry>> {
        let entries = self
            .session
            .call(TreeList {
                path: self.path.clone(),
                entries: count as u64,
                bytes: cap,
            })
            .await?;
        if entries.len() > count {
            bail!(workspace::Failure::refused(
                Code::TooMany,
                "The tree has too many entries."
            ));
        }
        let mut left = cap;
        let mut kinds = BTreeMap::new();
        for entry in &entries {
            ensure!(
                (entry.path == "."
                    || super::files::is_valid(&entry.path))
                    && kinds
                        .insert(entry.path.as_str(), entry.kind)
                        .is_none(),
                "Invalid remote tree path."
            );
            if entry.kind == TreeKind::File {
                left = left.checked_sub(entry.size).ok_or_else(
                    || {
                        workspace::Failure::refused(
                            Code::TooLarge,
                            "The tree is too large.",
                        )
                    },
                )?;
                ensure!(entry.size <= MAX_FILE as u64, TooLarge);
            }
        }
        ensure!(
            kinds.get(".") == Some(&TreeKind::Directory),
            "The tree is not a directory."
        );
        for entry in entries.iter().filter(|entry| entry.path != ".")
        {
            let parent = entry
                .path
                .rsplit_once('/')
                .map_or(".", |(parent, _)| parent);
            ensure!(
                kinds.get(parent) == Some(&TreeKind::Directory),
                "Invalid remote tree parent."
            );
        }
        Ok(entries)
    }

    pub async fn read_file(
        &self,
        path: &str,
        cap: u64,
    ) -> Result<Vec<u8>> {
        let read = self
            .session
            .call(Read {
                path: self.at(path),
                no_follow: true,
            })
            .await?;
        let bytes = read.data.0;
        ensure!(
            read.info.kind == Kind::File
                && read.info.size == bytes.len() as u64,
            workspace::Failure::refused(
                Code::Invalid,
                "The remote file changed while it was read.",
            )
        );
        ensure!(bytes.len() as u64 <= cap, "The file is too large.");
        Ok(bytes)
    }

    pub async fn put(&self, path: &str, entry: &Entry) -> Result<()> {
        ensure!(entry.bytes.len() <= MAX_FILE, TooLarge);
        let written = self
            .session
            .call(TreeWrite {
                path: self.at(path),
                data: Bytes(entry.bytes.clone()),
                mode: entry.mode & 0o777,
            })
            .await;
        match written {
            Err(failure) if failure.code == Code::AlreadyExists => {
                let present = self
                    .read_file(path, entry.bytes.len() as u64)
                    .await?;
                ensure!(
                    present == entry.bytes,
                    "The merge target changed; inspect the houses \
                     before retrying."
                );
                Ok(())
            }
            written => Ok(written?),
        }
    }

    pub async fn mkdir(&self, path: &str) -> Result<()> {
        Ok(self
            .session
            .call(Mkdir {
                path: self.at(path),
                recursive: false,
                no_follow: true,
            })
            .await?)
    }

    pub async fn set_mode(
        &self,
        path: &str,
        mode: u32,
    ) -> Result<()> {
        Ok(self
            .session
            .call(SetMode {
                path: self.at(path),
                mode: mode & 0o777,
            })
            .await?)
    }

    pub async fn replace(
        &self,
        path: &str,
        bytes: &[u8],
    ) -> Result<()> {
        ensure!(bytes.len() <= MAX_FILE, TooLarge);
        let info = Info {
            path: self.at(path),
            no_follow: true,
        };
        match self.session.call(info).await {
            Ok(info) if info.kind != Kind::File => {
                bail!("The file to replace is not a regular file.")
            }
            Err(failure) if failure.code != Code::NotFound => {
                return Err(failure.into());
            }
            _ => {}
        }
        Ok(self
            .session
            .call(Write {
                path: self.at(path),
                data: Bytes(bytes.to_vec()),
                append: false,
                parents: false,
                no_follow: true,
            })
            .await?)
    }

    pub async fn remove(
        &self,
        path: &str,
        recursive: bool,
    ) -> Result<()> {
        Ok(self
            .session
            .call(Remove {
                path: self.at(path),
                recursive,
                force: false,
                no_follow: true,
            })
            .await?)
    }
}

/// The agent's notes.
pub(super) async fn notes(
    session: &WorkspaceSession,
) -> Result<String> {
    let read = Read {
        path: format!("{HOUSE}/{}", super::NOTES),
        no_follow: true,
    };
    Ok(String::from_utf8(session.call(read).await?.data.0)?)
}

/// The skills on the house's shelf, in the folders' order.
pub(super) async fn skills(session: &WorkspaceSession) -> Vec<Skill> {
    let list = List {
        path: format!("{HOUSE}/skills"),
        no_follow: true,
    };
    let Ok(entries) = session.call(list).await else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries {
        if entry.kind != Kind::Directory
            || !super::files::is_valid(&entry.name)
        {
            continue;
        }
        let read = Read {
            path: format!("{HOUSE}/skills/{}/SKILL.md", entry.name),
            no_follow: true,
        };
        if let Ok(read) = session.call(read).await {
            let bytes =
                &read.data.0[..read.data.0.len().min(64 * 1024)];
            found.push(shelf::skill(
                entry.name,
                &String::from_utf8_lossy(bytes),
            ));
        }
    }
    found.sort_by(|a, b| a.folder.cmp(&b.folder));
    found
}

/// The file `name` of the hamlet's folder.
pub(super) async fn project(
    session: &WorkspaceSession,
    name: &str,
) -> Option<Vec<u8>> {
    let read = Read {
        path: format!("{PROJECT}/{name}"),
        no_follow: false,
    };
    session.call(read).await.ok().map(|read| read.data.0)
}

/// Whether the hamlet's folder has `name`.
pub(super) async fn has_project_file(
    session: &WorkspaceSession,
    name: &str,
) -> bool {
    let info = Info {
        path: format!("{PROJECT}/{name}"),
        no_follow: false,
    };
    session.call(info).await.is_ok()
}
