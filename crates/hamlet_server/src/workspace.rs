//! A remote workspace (C18): another machine whose worker runs the
//! agents' tools, each agent in its own `/lodge` with its hamlet's
//! `/project` and the library at `/library`. Requests carry a bearer
//! token; virtual paths never reach the host.

pub(crate) mod lodges;
mod serving;

use std::path::PathBuf;
use std::time::Duration;

use hamlet_store::ids::AgentId;
use hamlet_workspace_wire::{
    Call, Code, LIBRARY_FILES, Library, LibraryFile, MAX_FILE,
    Message, Operation, Reply, Request, VERSION,
};

use crate::credentials::WorkspaceAddress;
use crate::knowledge::folder::Folder;

/// The most bytes of a reply.
const MAX_REPLY: usize = 6 * 1024 * 1024;
const UNKNOWN: &str = "The workspace result is unknown. Inspect its \
                       files before repeating the operation.";

/// What a request may reach in the workspace.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Policy {
    /// The hamlet's shared `/project`.
    pub project: bool,
    /// The public HTTP proxy.
    pub network: bool,
    pub read_only: bool,
}

/// The authenticated connection to a workspace worker.
#[derive(Clone)]
pub(crate) struct Transport {
    client: reqwest::Client,
    connection: WorkspaceAddress,
}

impl Transport {
    pub fn new(connection: WorkspaceAddress) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(165))
                .build()?,
            connection,
        })
    }

    pub fn url(&self) -> &str {
        &self.connection.url
    }

    /// The requests of `agent`, which works in `workspace`, under
    /// `policy`.
    pub fn session(
        &self,
        agent: AgentId,
        workspace: &str,
        policy: Policy,
    ) -> anyhow::Result<Session> {
        let agent = u32::try_from(agent.0).map_err(|_| {
            anyhow::anyhow!("an agent id past the workspace's")
        })?;
        Ok(Session {
            transport: self.clone(),
            workspace: workspace.to_owned(),
            agent,
            policy,
            library: None,
        })
    }

    async fn post(
        &self,
        message: &Message,
    ) -> Result<Reply, Failure> {
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
            .map_err(|_| Failure::unknown())?;
        if !response.status().is_success() {
            return Err(Failure::unknown());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) =
            response.chunk().await.map_err(|_| Failure::unknown())?
        {
            if chunk.len() > MAX_REPLY.saturating_sub(bytes.len()) {
                return Err(Failure::unknown());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| Failure::unknown())
    }
}

/// A failed workspace request.
#[derive(Debug)]
pub(crate) struct Failure {
    pub code: Code,
    pub message: String,
    /// The request may have taken effect.
    pub uncertain: bool,
}

impl Failure {
    fn unknown() -> Self {
        Self {
            code: Code::Unknown,
            message: UNKNOWN.into(),
            uncertain: true,
        }
    }
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

impl From<Failure> for hamlet_sandbox::FileError {
    fn from(failure: Failure) -> Self {
        Self {
            code: failure.code,
            message: failure.message,
        }
    }
}

/// A request whose future can be dropped while the worker still runs
/// it; dropping it cancels the request.
struct Pending<'a> {
    transport: &'a Transport,
    id: String,
    done: bool,
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if !self.done {
            let transport = self.transport.clone();
            let id = std::mem::take(&mut self.id);
            tokio::spawn(async move {
                let _ = transport.post(&Message::Cancel { id }).await;
            });
        }
    }
}

/// Requests of one agent under one policy.
#[derive(Clone)]
pub(crate) struct Session {
    transport: Transport,
    workspace: String,
    agent: u32,
    policy: Policy,
    library: Option<Library>,
}

impl Session {
    /// The same requests with the library at `root` mounted
    /// read-only at `/library`, as it is now.
    pub async fn with_library(
        mut self,
        root: PathBuf,
    ) -> Result<Self, Failure> {
        self.library = Some(snapshot(root).await?);
        Ok(self)
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    /// Runs `operation`. The library goes by its digest, and its
    /// files only when the worker does not hold them.
    pub async fn call<C: Call>(
        &self,
        operation: C,
    ) -> Result<C::Reply, Failure> {
        let operation = operation.into();
        let named = self.library.as_ref().map(Library::named);
        let value = match self.execute(operation.clone(), named).await
        {
            Err(failure) if failure.code == Code::LibraryMissing => {
                self.execute(operation, self.library.clone()).await?
            }
            result => result?,
        };
        serde_json::from_value(value).map_err(|_| Failure::unknown())
    }

    fn request(
        &self,
        operation: Operation,
        library: Option<Library>,
    ) -> Request {
        Request {
            version: VERSION,
            id: uuid::Uuid::new_v4().to_string(),
            workspace: self.workspace.clone(),
            agent: self.agent,
            project: self.policy.project,
            network: self.policy.network,
            read_only: self.policy.read_only,
            library,
            operation,
        }
    }

    async fn execute(
        &self,
        operation: Operation,
        library: Option<Library>,
    ) -> Result<serde_json::Value, Failure> {
        let request = self.request(operation, library);
        let mut pending = Pending {
            transport: &self.transport,
            id: request.id.clone(),
            done: false,
        };
        let reply =
            self.transport.post(&Message::Execute { request }).await;
        pending.done = true;
        let failure = match reply {
            Ok(Reply::Ok { value }) => return Ok(value),
            Ok(Reply::Error {
                code,
                message,
                uncertain,
            }) => Failure {
                code,
                message: message.replace(
                    &self.transport.connection.token,
                    "[REDACTED]",
                ),
                uncertain: uncertain
                    || matches!(
                        code,
                        Code::Unknown
                            | Code::Aborted
                            | Code::Uncertain
                            | Code::Confinement
                    ),
            },
            Err(failure) => failure,
        };
        let mut message = failure.message;
        if failure.uncertain && message != UNKNOWN {
            message.push_str(&format!(" {UNKNOWN}"));
        }
        message.push_str(&format!(" Request: {}.", pending.id));
        Err(Failure { message, ..failure })
    }
}

/// The workspace an agent works in: its hamlet's, or the realm's for
/// the Hand and the workers of no hamlet.
pub(crate) fn workspace_of(
    hamlet: Option<&hamlet_store::ids::HamletId>,
) -> String {
    hamlet.map_or_else(|| "realm".to_owned(), ToString::to_string)
}

/// The library at `root`, read without following links.
async fn snapshot(root: PathBuf) -> Result<Library, Failure> {
    let too_much = || Failure {
        code: Code::Invalid,
        uncertain: false,
        message: format!(
            "The library could not be read safely within \
             {LIBRARY_FILES} files and 4 MiB; no workspace \
             operation was sent."
        ),
    };
    tokio::task::spawn_blocking(move || {
        let folder = Folder::open(&root).ok()?;
        let listing = folder.list(&|_| false).ok()?;
        if listing.len() > LIBRARY_FILES {
            return None;
        }
        let mut total = 0;
        let mut files = Vec::new();
        for entry in listing {
            let bytes = folder.read(&entry.path).ok()?;
            total += bytes.len();
            if total > MAX_FILE {
                return None;
            }
            files.push(LibraryFile {
                path: entry.path,
                data: hamlet_workspace_wire::Bytes(bytes),
                mode: if entry.executable { 0o755 } else { 0o644 },
            });
        }
        Some(Library::of(files))
    })
    .await
    .map_err(|_| Failure::unknown())?
    .ok_or_else(too_much)
}
