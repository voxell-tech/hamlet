//! What an agent's file tools reach: this machine under the sandbox,
//! or a remote workspace.

use std::time::Duration;

use hamlet_workspace_wire::{
    Bytes, Canonical, Code, Exec, Info, Kind, LIST_ENTRIES, List,
    Listed, MAX_FILE, Ran, Read, Walk, Walked, Write,
};

use super::walk::{SKIPPED, VISITED};
use crate::papers::remote::{HOUSE, PROJECT, TooLarge};
use crate::sandbox::{
    Env, FileError, FileInfo, FileKind, Sandbox, SetupError, path,
};
use crate::workspace::WorkspaceSession;

/// File operations on the paths an agent's tools see.
pub(super) trait Reach {
    /// The folder commands run in.
    fn cwd(&self) -> &str;
    fn house(&self) -> &str;
    /// The folder the hamlet works on.
    fn folder(&self) -> Option<&str>;

    /// The absolute path `path` names from [`Reach::cwd`].
    fn absolute(&self, path: &str) -> String {
        path::resolve(self.cwd(), path)
    }

    async fn exists(&mut self, path: &str)
    -> Result<bool, FileError>;
    async fn read_file(
        &mut self,
        path: &str,
    ) -> Result<Vec<u8>, FileError>;
    /// The text of a file, refused unless it is UTF-8.
    async fn read_text(
        &mut self,
        path: &str,
    ) -> Result<String, FileError>;
    async fn readable(&mut self, path: &str)
    -> Result<(), FileError>;
    async fn file_info(
        &mut self,
        path: &str,
    ) -> Result<FileInfo, FileError>;
    async fn list_dir(
        &mut self,
        path: &str,
    ) -> Result<Vec<FileInfo>, FileError>;
    async fn canonical_path(
        &mut self,
        path: &str,
    ) -> Result<String, FileError>;
    /// Writes `content` over the file, making its folders.
    async fn write_file(
        &mut self,
        path: &str,
        content: &str,
    ) -> Result<(), FileError>;
    /// The regular files under `root` in the order a search visits
    /// them, at most [`VISITED`].
    async fn walk(&mut self, root: &str) -> Walked;

    /// The full path a tool's `path` names, as [`super::footing`]
    /// tells.
    async fn footing(
        &mut self,
        path: &str,
    ) -> Result<String, FileError> {
        let house = self.house().to_owned();
        let folder = self.folder().map(str::to_owned);
        let own = match super::firsts(path, &house, folder.as_deref())
        {
            Some([folder, house]) => {
                !self.exists(&folder).await?
                    && self.exists(&house).await?
            }
            None => false,
        };
        Ok(super::footing(path, &house, folder.as_deref(), own))
    }
}

/// The failure to open the tools' environment on this machine.
#[derive(Debug, thiserror::Error)]
pub(super) enum Unopened {
    #[error(transparent)]
    Setup(#[from] SetupError),
    #[error(transparent)]
    Lost(#[from] tokio::task::JoinError),
}

/// This machine's files, reached under the sandbox's policy off the
/// async runtime.
pub(super) struct Local {
    env: Option<Env>,
    cwd: String,
    house: String,
    folder: Option<String>,
}

impl Local {
    /// The environment of the tools at `place`, made off the async
    /// runtime.
    pub async fn open(
        sandbox: Sandbox,
        place: &super::Place,
    ) -> Result<Self, Unopened> {
        let at = place.clone();
        let env = tokio::task::spawn_blocking(move || {
            super::environment(&sandbox, &at, false)
        })
        .await??;
        Ok(Self {
            cwd: env.cwd().to_owned(),
            env: Some(env),
            house: path::text(&place.house),
            folder: place.folder.as_deref().map(path::text),
        })
    }

    /// Runs `work` with the environment on a blocking thread.
    async fn on<T: Send + 'static>(
        &mut self,
        work: impl FnOnce(&Env) -> Result<T, FileError> + Send + 'static,
    ) -> Result<T, FileError> {
        let lost = || FileError {
            code: Code::Unknown,
            message: "The tool's environment was lost. Tell the \
                      chief."
                .into(),
        };
        let env = self.env.take().ok_or_else(lost)?;
        let (env, result) = tokio::task::spawn_blocking(move || {
            let result = work(&env);
            (env, result)
        })
        .await
        .map_err(|_| lost())?;
        self.env = Some(env);
        result
    }
}

impl Reach for Local {
    fn cwd(&self) -> &str {
        &self.cwd
    }

    fn house(&self) -> &str {
        &self.house
    }

    fn folder(&self) -> Option<&str> {
        self.folder.as_deref()
    }

    async fn exists(
        &mut self,
        path: &str,
    ) -> Result<bool, FileError> {
        let path = path.to_owned();
        self.on(move |env| env.exists(&path)).await
    }

    async fn read_file(
        &mut self,
        path: &str,
    ) -> Result<Vec<u8>, FileError> {
        let path = path.to_owned();
        self.on(move |env| env.read_file(&path)).await
    }

    async fn read_text(
        &mut self,
        path: &str,
    ) -> Result<String, FileError> {
        let path = path.to_owned();
        self.on(move |env| env.read_text(&path)).await
    }

    async fn readable(
        &mut self,
        path: &str,
    ) -> Result<(), FileError> {
        let path = path.to_owned();
        self.on(move |env| env.open_file(&path).map(drop)).await
    }

    async fn file_info(
        &mut self,
        path: &str,
    ) -> Result<FileInfo, FileError> {
        let path = path.to_owned();
        self.on(move |env| env.file_info(&path)).await
    }

    async fn list_dir(
        &mut self,
        path: &str,
    ) -> Result<Vec<FileInfo>, FileError> {
        let path = path.to_owned();
        self.on(move |env| env.list_dir(&path)).await
    }

    async fn canonical_path(
        &mut self,
        path: &str,
    ) -> Result<String, FileError> {
        let path = path.to_owned();
        self.on(move |env| env.canonical_path(&path)).await
    }

    async fn write_file(
        &mut self,
        path: &str,
        content: &str,
    ) -> Result<(), FileError> {
        let (path, content) = (path.to_owned(), content.to_owned());
        self.on(move |env| env.write_file(&path, &content)).await
    }

    async fn walk(&mut self, root: &str) -> Walked {
        let root = root.to_owned();
        self.on(move |env| {
            let skip = SKIPPED.map(String::from);
            Ok(hamlet_workspace_wire::walk(
                &root,
                &skip,
                VISITED as u64,
                |dir| {
                    env.list_dir(dir).map(|listed| {
                        listed
                            .into_iter()
                            .map(|entry| {
                                let kind = match entry.kind {
                                    FileKind::File => {
                                        Listed::File(entry.size)
                                    }
                                    FileKind::Directory => {
                                        Listed::Directory
                                    }
                                    FileKind::Symlink => {
                                        Listed::Other
                                    }
                                };
                                (entry.name, kind)
                            })
                            .collect()
                    })
                },
            ))
        })
        .await
        .unwrap_or_default()
    }
}

/// A remote workspace's `/house` and `/project`.
pub(super) struct Remote {
    session: WorkspaceSession,
}

impl Remote {
    pub fn new(session: WorkspaceSession) -> Self {
        Self { session }
    }

    /// The policy commands run under, as a result names it.
    pub fn policy(&self) -> String {
        let policy = self.session.policy();
        format!(
            "workspace-v1:{}:{}:{}",
            if policy.project { "project" } else { "house" },
            if policy.network {
                "public-http-proxy"
            } else {
                "netless"
            },
            if policy.read_only {
                "read-only"
            } else {
                "writable"
            }
        )
    }

    /// Runs `command` with bash in [`Reach::cwd`] for at most
    /// `timeout`, in whole milliseconds rounded up.
    pub async fn exec(
        &self,
        command: &str,
        timeout: Duration,
    ) -> Result<Ran, crate::workspace::Failure> {
        let timeout_ms =
            u64::try_from(timeout.as_nanos().div_ceil(1_000_000))
                .unwrap_or(u64::MAX);
        self.session
            .call(Exec {
                argv: vec![
                    "/bin/bash".into(),
                    "-c".into(),
                    command.into(),
                ],
                cwd: self.cwd().into(),
                timeout_ms,
            })
            .await
    }

    async fn contents(
        &self,
        path: &str,
    ) -> Result<Vec<u8>, FileError> {
        let read = self
            .session
            .call(Read {
                path: self.absolute(path),
                no_follow: false,
            })
            .await?;
        let bytes = read.data.0;
        if bytes.len() > MAX_FILE
            || read.info.kind != Kind::File
            || read.info.size != bytes.len() as u64
        {
            return Err(unknown());
        }
        Ok(bytes)
    }
}

fn unknown() -> FileError {
    FileError {
        code: Code::Unknown,
        message: "The workspace answered with a malformed result. \
                  Inspect its files before repeating the operation."
            .into(),
    }
}

/// What `stat` saw of a remote path, refused unless its name is one.
fn checked(
    stat: hamlet_workspace_wire::Stat,
) -> Result<FileInfo, FileError> {
    if stat.name.contains(['/', '\0'])
        || [".", ".."].contains(&stat.name.as_str())
    {
        return Err(unknown());
    }
    Ok(FileInfo {
        name: stat.name,
        kind: match stat.kind {
            Kind::File => FileKind::File,
            Kind::Directory => FileKind::Directory,
            Kind::Symlink => FileKind::Symlink,
        },
        size: stat.size,
    })
}

impl Reach for Remote {
    fn cwd(&self) -> &str {
        if self.session.policy().project {
            PROJECT
        } else {
            HOUSE
        }
    }

    fn house(&self) -> &str {
        HOUSE
    }

    fn folder(&self) -> Option<&str> {
        self.session.policy().project.then_some(PROJECT)
    }

    async fn exists(
        &mut self,
        path: &str,
    ) -> Result<bool, FileError> {
        match self.file_info(path).await {
            Ok(_) => Ok(true),
            Err(error) if error.code == Code::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn read_file(
        &mut self,
        path: &str,
    ) -> Result<Vec<u8>, FileError> {
        self.contents(path).await
    }

    async fn read_text(
        &mut self,
        path: &str,
    ) -> Result<String, FileError> {
        let bytes = self.contents(path).await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn readable(
        &mut self,
        path: &str,
    ) -> Result<(), FileError> {
        self.contents(path).await.map(drop)
    }

    async fn file_info(
        &mut self,
        path: &str,
    ) -> Result<FileInfo, FileError> {
        let info = Info {
            path: self.absolute(path),
            no_follow: false,
        };
        checked(self.session.call(info).await?)
    }

    async fn list_dir(
        &mut self,
        path: &str,
    ) -> Result<Vec<FileInfo>, FileError> {
        let list = List {
            path: self.absolute(path),
            no_follow: false,
        };
        let entries = self.session.call(list).await?;
        if entries.len() > LIST_ENTRIES {
            return Err(unknown());
        }
        entries.into_iter().map(checked).collect()
    }

    async fn canonical_path(
        &mut self,
        path: &str,
    ) -> Result<String, FileError> {
        let canonical = Canonical {
            path: self.absolute(path),
        };
        Ok(self.session.call(canonical).await?)
    }

    async fn write_file(
        &mut self,
        path: &str,
        content: &str,
    ) -> Result<(), FileError> {
        if content.len() > MAX_FILE {
            return Err(FileError {
                code: Code::Invalid,
                message: TooLarge.to_string(),
            });
        }
        let write = Write {
            path: self.absolute(path),
            data: Bytes(content.as_bytes().to_vec()),
            append: false,
            parents: true,
            no_follow: false,
        };
        Ok(self.session.call(write).await?)
    }

    async fn walk(&mut self, root: &str) -> Walked {
        let walk = Walk {
            path: root.into(),
            skip: SKIPPED.map(String::from).into(),
            files: VISITED as u64,
        };
        self.session.call(walk).await.unwrap_or_default()
    }
}
