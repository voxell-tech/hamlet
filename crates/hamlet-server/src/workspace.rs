//! Authenticated workspace v1 requests. Virtual paths never reach the
//! host.

use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::credentials::Workspace;
use crate::record::AgentRef;
use crate::sandbox::{FileError, FileInfo, FileKind};

pub(crate) const MAX_FILE: usize = 4 * 1024 * 1024;
const MAX_REPLY: usize = 6 * 1024 * 1024;
const UNKNOWN: &str = "The workspace result is unknown. Inspect its \
                       files before repeating the operation.";

#[derive(Clone)]
pub(crate) struct Remote {
    client: reqwest::Client,
    connection: Workspace,
    agent: AgentRef,
    project: bool,
    network: bool,
    read_only: bool,
    library: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Reply {
    Ok {
        value: Value,
    },
    Error {
        code: String,
        message: String,
        uncertain: bool,
    },
}

#[derive(Debug)]
pub(crate) struct Failure {
    pub code: &'static str,
    pub message: String,
    pub uncertain: bool,
}

impl std::fmt::Display for Failure {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

impl From<Failure> for FileError {
    fn from(failure: Failure) -> Self {
        Self {
            code: failure.code,
            message: failure.message,
        }
    }
}

fn unknown() -> Failure {
    Failure {
        code: "unknown",
        message: UNKNOWN.into(),
        uncertain: true,
    }
}

fn invalid(message: &str) -> Failure {
    Failure {
        code: "invalid",
        message: message.into(),
        uncertain: false,
    }
}

/// A request whose future can be dropped while the worker is still
/// running.
struct Pending {
    remote: Remote,
    id: String,
    done: bool,
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.done {
            let remote = self.remote.clone();
            let id = self.id.clone();
            tokio::spawn(async move {
                let _ = remote
                    .post(&json!({"kind":"cancel", "id":id}))
                    .await;
            });
        }
    }
}

impl Remote {
    pub fn new(
        connection: Workspace,
        agent: AgentRef,
        project: bool,
        network: bool,
        read_only: bool,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            agent.number <= u32::MAX as u64,
            "invalid workspace agent"
        );
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(165))
                .build()?,
            connection,
            agent,
            project,
            network,
            read_only,
            library: None,
        })
    }

    pub fn with_library(mut self, root: PathBuf) -> Self {
        self.library = Some(root);
        self
    }

    pub fn cwd(&self) -> &str {
        if self.project { "/project" } else { "/house" }
    }

    pub fn policy(&self) -> String {
        format!(
            "workspace-v1:{}:{}:{}",
            if self.project { "project" } else { "house" },
            if self.network {
                "public-http-proxy"
            } else {
                "netless"
            },
            if self.read_only {
                "read-only"
            } else {
                "writable"
            }
        )
    }

    pub fn absolute(&self, path: &str) -> String {
        // POSIX resolution, including on a server whose host uses
        // other paths.
        let joined = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("{}/{path}", self.cwd())
        };
        let mut parts = Vec::new();
        for part in joined.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                _ => parts.push(part),
            }
        }
        format!("/{}", parts.join("/"))
    }

    async fn post(&self, message: &Value) -> Result<Value, Failure> {
        let mut response = self
            .client
            .post(format!(
                "{}/v1",
                self.connection.url.trim_end_matches('/')
            ))
            .bearer_auth(&self.connection.token)
            .json(message)
            .send()
            .await
            .map_err(|_| unknown())?;
        if !response.status().is_success() {
            return Err(unknown());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) =
            response.chunk().await.map_err(|_| unknown())?
        {
            if chunk.len() > MAX_REPLY.saturating_sub(bytes.len()) {
                return Err(unknown());
            }
            bytes.extend_from_slice(&chunk);
        }
        match serde_json::from_slice::<Reply>(&bytes)
            .map_err(|_| unknown())?
        {
            Reply::Ok { value } => Ok(value),
            Reply::Error {
                code,
                message,
                uncertain,
            } => {
                let code = match code.as_str() {
                    "aborted" => "aborted",
                    "not_found" => "not_found",
                    "permission_denied" => "permission_denied",
                    "not_directory" => "not_directory",
                    "is_directory" => "is_directory",
                    "invalid" => "invalid",
                    "not_supported" => "not_supported",
                    "timeout" => "timeout",
                    _ => "unknown",
                };
                Err(Failure {
                    code,
                    message: message.replace(
                        &self.connection.token,
                        "[REDACTED]",
                    ),
                    uncertain: uncertain
                        || matches!(code, "unknown" | "aborted"),
                })
            }
        }
    }

    pub async fn call<T: serde::de::DeserializeOwned>(
        &self,
        operation: Value,
    ) -> Result<T, Failure> {
        let mut request = json!({"version":1, "id":uuid::Uuid::new_v4().to_string(),
            "workspace":self.agent.hamlet, "agent":self.agent.number,
            "project":self.project, "network":self.network, "read_only":self.read_only,
            "operation":operation});
        if let Some(root) = &self.library {
            let root = root.clone();
            let files = tokio::task::spawn_blocking(move || {
                use rustix::fs::{Mode, OFlags};
                let root = std::fs::File::from(rustix::fs::open(
                    root,
                    OFlags::RDONLY
                        | OFlags::DIRECTORY
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::empty(),
                )?);
                crate::papers::tree::library(&root)
            })
            .await
            .map_err(|_| unknown())?
            .map_err(|_| Failure {
                code: "unknown",
                uncertain: false,
                message: "The shared library could not be read \
                          safely within 1024 entries and 4 MiB. Ask \
                          the chief to inspect the shelf; no \
                          workspace operation was sent."
                    .into(),
            })?;
            request["library"] = files.into_iter().map(|(path, entry)| json!({"path":path,"data":STANDARD.encode(entry.bytes),"mode":entry.mode})).collect();
        }
        let mut pending = Pending {
            remote: self.clone(),
            id: request["id"].as_str().expect("request id").into(),
            done: false,
        };
        let response = self
            .post(&json!({"kind":"execute", "request":request}))
            .await;
        pending.done = true;
        let value = response.map_err(|mut failure| {
            if failure.uncertain
                && !failure.message.contains("Inspect")
            {
                failure.message.push_str(&format!(" {UNKNOWN}"));
            }
            failure
                .message
                .push_str(&format!(" Request: {}.", pending.id));
            failure
        })?;
        serde_json::from_value(value).map_err(|_| unknown())
    }

    pub async fn initialize(&self) -> Result<(), Failure> {
        self.call(json!({"kind":"init_house"})).await
    }

    pub async fn read(
        &self,
        path: &str,
        no_follow: bool,
    ) -> Result<Vec<u8>, FileError> {
        #[derive(Deserialize)]
        struct Read {
            data: String,
            info: Information,
        }
        if no_follow {
            self.parents(path, false).await?;
        }
        let read = self.call::<Read>(json!({"kind":"read", "path":self.absolute(path), "no_follow":no_follow})).await?;
        let bytes =
            STANDARD.decode(read.data).map_err(|_| unknown())?;
        if bytes.len() > MAX_FILE
            || read.info.kind != "file"
            || read.info.size != bytes.len() as u64
        {
            return Err(unknown().into());
        }
        Ok(bytes)
    }

    pub async fn info(
        &self,
        path: &str,
    ) -> Result<FileInfo, FileError> {
        self.call::<Information>(
            json!({"kind":"info", "path":self.absolute(path)}),
        )
        .await?
        .checked()
    }

    pub async fn parents(
        &self,
        path: &str,
        missing: bool,
    ) -> Result<(), FileError> {
        let path = self.absolute(path);
        let mut current = String::new();
        let parts = path
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            current.push('/');
            current.push_str(part);
            match self.info(&current).await {
                Ok(info) if info.kind == FileKind::Directory => {}
                Ok(_) => {
                    return Err(invalid(
                        "The house path contains a link or a \
                         non-directory.",
                    )
                    .into());
                }
                Err(error)
                    if missing && error.code == "not_found" =>
                {
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub async fn list(
        &self,
        path: &str,
    ) -> Result<Vec<FileInfo>, FileError> {
        let entries = self
            .call::<Vec<Information>>(
                json!({"kind":"list", "path":self.absolute(path)}),
            )
            .await?;
        if entries.len() > 10_000 {
            return Err(unknown().into());
        }
        entries.into_iter().map(Information::checked).collect()
    }

    pub async fn exists(
        &self,
        path: &str,
    ) -> Result<bool, FileError> {
        match self.info(path).await {
            Ok(_) => Ok(true),
            Err(error) if error.code == "not_found" => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub async fn write(
        &self,
        path: &str,
        bytes: &[u8],
    ) -> Result<(), FileError> {
        if bytes.len() > MAX_FILE {
            return Err(invalid(
                "Remote files are limited to 4 MiB per transfer.",
            )
            .into());
        }
        self.call(json!({"kind":"write","path":self.absolute(path),"data":STANDARD.encode(bytes),"append":false})).await.map_err(Into::into)
    }

    pub async fn footing(
        &self,
        path: &str,
    ) -> Result<String, FileError> {
        if path.starts_with('/') || !self.project {
            return Ok(self.absolute(path));
        }
        let first = path.split('/').next().unwrap_or_default();
        if !self.exists(&format!("/project/{first}")).await?
            && self.exists(&format!("/house/{first}")).await?
        {
            return Ok(self.absolute(&format!("/house/{path}")));
        }
        Ok(self.absolute(path))
    }
}

#[derive(Deserialize)]
struct Information {
    name: String,
    kind: String,
    size: u64,
}

impl Information {
    fn checked(self) -> Result<FileInfo, FileError> {
        let kind = match self.kind.as_str() {
            "file" => FileKind::File,
            "directory" => FileKind::Directory,
            "symlink" => FileKind::Symlink,
            _ => return Err(unknown().into()),
        };
        if self.name.contains(['/', '\0'])
            || [".", ".."].contains(&self.name.as_str())
        {
            return Err(unknown().into());
        }
        Ok(FileInfo {
            name: self.name,
            kind,
            size: self.size,
        })
    }
}

pub(crate) fn unavailable(operation: &str) -> String {
    format!(
        "Cannot {operation}: this operation is not supported by \
         remote workspaces. No local copy was opened; keep the \
         remote files in place and ask the chief before changing \
         workspace backends."
    )
}
