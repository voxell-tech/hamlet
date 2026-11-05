//! What an agent's file tools reach: this machine under the sandbox.

use std::time::Duration;

use hamlet_os::fs::path;
use hamlet_sandbox::{
    Env, ExecError, FileError, FileInfo, FileKind, Launch, Leftovers,
    Ran, Sandbox, SetupError,
};
use hamlet_workspace_wire::{Code, Listed, Walked};

use super::walk::{SKIPPED, VISITED};

/// Said where a call lost the environment it works in.
const LOST: &str = "The tool's environment was lost. Tell the chief.";

/// File operations on the paths an agent's tools see.
pub(super) trait Reach {
    /// The folder commands run in.
    fn cwd(&self) -> &str;
    fn lodge(&self) -> &str;
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

    /// Runs `argv` in [`Reach::cwd`] for at most `timeout`. Its
    /// output, with `secrets` taken out, goes to `output`, and past
    /// the spill limits all of it is kept in a file.
    async fn exec(
        &mut self,
        argv: &[String],
        timeout: Duration,
        secrets: &[String],
        output: &mut (impl FnMut(&str) + Send),
    ) -> Result<Ran, ExecError>;

    /// The private temp folder commands get here, where there is
    /// one.
    fn tmp(&self) -> Option<&str>;

    /// The policy commands run under, as a result names it.
    fn policy(&self) -> String;

    /// The limits commands run without, as a result names them.
    fn degraded(&self) -> serde_json::Value;

    /// The full path a tool's `path` names, as [`super::footing`]
    /// tells.
    async fn footing(
        &mut self,
        path: &str,
    ) -> Result<String, FileError> {
        let lodge = self.lodge().to_owned();
        let folder = self.folder().map(str::to_owned);
        let own = match super::firsts(path, &lodge, folder.as_deref())
        {
            Some([folder, lodge]) => {
                !self.exists(&folder).await?
                    && self.exists(&lodge).await?
            }
            None => false,
        };
        Ok(super::footing(path, &lodge, folder.as_deref(), own))
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
    leftovers: Leftovers,
    policy: String,
    degraded: serde_json::Value,
    cwd: String,
    lodge: String,
    folder: Option<String>,
}

impl Local {
    /// The environment of the tools at `place`, made off the async
    /// runtime, whose commands reach the network only with `net` and
    /// leave what they started to `leftovers`.
    pub async fn open(
        sandbox: Sandbox,
        place: &super::Place,
        net: bool,
        leftovers: Leftovers,
    ) -> Result<Self, Unopened> {
        let at = place.clone();
        let env = tokio::task::spawn_blocking(move || {
            super::environment(&sandbox, &at, net)
        })
        .await??;
        Ok(Self {
            policy: env.policy_hash().to_owned(),
            degraded: env.degraded(),
            cwd: env.cwd().to_owned(),
            env: Some(env),
            leftovers,
            lodge: path::text(&place.lodge),
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
            message: LOST.into(),
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

    fn lodge(&self) -> &str {
        &self.lodge
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

    async fn exec(
        &mut self,
        argv: &[String],
        timeout: Duration,
        secrets: &[String],
        output: &mut (impl FnMut(&str) + Send),
    ) -> Result<Ran, ExecError> {
        let Some(env) = &self.env else {
            return Err(ExecError::Spawn(LOST.into()));
        };
        env.exec(
            Launch::Shell(argv),
            timeout,
            secrets,
            &self.leftovers,
            output,
        )
        .await
    }

    fn tmp(&self) -> Option<&str> {
        self.env.as_ref().map(Env::tmp)
    }

    fn policy(&self) -> String {
        self.policy.clone()
    }

    fn degraded(&self) -> serde_json::Value {
        self.degraded.clone()
    }
}
