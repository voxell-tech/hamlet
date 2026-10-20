//! Authenticated workspace v1 requests. Virtual paths never reach the
//! host.

use std::path::PathBuf;
use std::time::Duration;

use hamlet_workspace_wire::{
    Call, Code, LIBRARY_FILES, Library, LibraryFile, MAX_FILE,
    Message, Operation, Reply, Request, VERSION,
};

use crate::credentials::Workspace;
use crate::record::AgentRef;
use crate::sandbox::FileError;

const MAX_REPLY: usize = 6 * 1024 * 1024;
const UNKNOWN: &str = "The workspace result is unknown. Inspect its \
                       files before repeating the operation.";

/// What a request may reach in the workspace.
#[derive(Clone, Copy, Default)]
pub(crate) struct Policy {
    /// The hamlet's shared `/project`.
    pub project: bool,
    /// The public HTTP proxy.
    pub network: bool,
    pub read_only: bool,
}

/// The authenticated connection to a workspace manager.
#[derive(Clone)]
pub(crate) struct Transport {
    client: reqwest::Client,
    connection: Workspace,
}

impl Transport {
    pub fn new(connection: Workspace) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(165))
                .build()?,
            connection,
        })
    }

    pub fn connection(&self) -> &Workspace {
        &self.connection
    }

    /// Requests of `agent` under `policy`.
    pub fn session(
        &self,
        agent: AgentRef,
        policy: Policy,
    ) -> anyhow::Result<Session> {
        anyhow::ensure!(
            agent.number <= u32::MAX as u64,
            "invalid workspace agent"
        );
        Ok(Session {
            transport: self.clone(),
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

/// A failed house operation: a workspace request, or a local one
/// refused with a protocol code.
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

    /// A refusal with `code` that changed nothing.
    pub fn refused(code: Code, message: &str) -> Self {
        Self {
            code,
            message: message.into(),
            uncertain: false,
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

impl From<Failure> for FileError {
    fn from(failure: Failure) -> Self {
        Self {
            code: failure.code.as_str(),
            message: failure.message,
        }
    }
}

/// Whether `error` leaves its request's effect unknown. Only a
/// workspace request can.
pub(crate) fn uncertain(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<Failure>()
        .is_some_and(|failure| failure.uncertain)
}

/// Whether `error` is a workspace refusal with `code`.
pub(crate) fn is(error: &anyhow::Error, code: Code) -> bool {
    error
        .downcast_ref::<Failure>()
        .is_some_and(|failure| failure.code == code)
}

/// A request whose future can be dropped while the worker is still
/// running; dropping it cancels the request.
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
    agent: AgentRef,
    policy: Policy,
    library: Option<Library>,
}

impl Session {
    /// The same requests with the shared library at `root` mounted
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

    /// A new request to run `operation` with `library`.
    fn request(
        &self,
        operation: Operation,
        library: Option<Library>,
    ) -> Request {
        Request {
            version: VERSION,
            id: uuid::Uuid::new_v4().to_string(),
            workspace: self.agent.hamlet.clone(),
            agent: self.agent.number as u32,
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

/// The shared library at `root`, read without following links.
async fn snapshot(root: PathBuf) -> Result<Library, Failure> {
    tokio::task::spawn_blocking(move || {
        use rustix::fs::{Mode, OFlags};
        let root = std::fs::File::from(rustix::fs::open(
            root,
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::NOFOLLOW
                | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        crate::papers::tree::library(
            &root,
            MAX_FILE as u64,
            LIBRARY_FILES,
        )
    })
    .await
    .map_err(|_| Failure::unknown())?
    .map(|files| {
        Library::of(
            files
                .into_iter()
                .map(|(path, entry)| LibraryFile {
                    path,
                    data: hamlet_workspace_wire::Bytes(entry.bytes),
                    mode: entry.mode,
                })
                .collect(),
        )
    })
    .map_err(|_| Failure {
        code: Code::Invalid,
        uncertain: false,
        message: format!(
            "The shared library could not be read safely within \
             {LIBRARY_FILES} entries and 4 MiB. Ask the chief to \
             inspect the shelf; no workspace operation was sent."
        ),
    })
}

pub(crate) fn unavailable(operation: &str) -> String {
    format!(
        "Cannot {operation}: this operation is not supported by \
         remote workspaces. No local copy was opened; keep the \
         remote files in place and ask the chief before changing \
         workspace backends."
    )
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    /// The shared protocol fixture, whose requests this client must
    /// build alike.
    const FIXTURE: &str = include_str!(
        "../../hamlet_workspace_wire/fixtures/protocol-v1.json"
    );

    #[test]
    fn requests_are_the_shared_fixture_s() {
        let fixture = serde_json::from_str::<Value>(FIXTURE).unwrap();
        let transport = Transport::new(Workspace {
            url: "http://127.0.0.1:9".into(),
            token: "fixture-token".into(),
        })
        .unwrap();
        let mut checked = 0;
        for case in fixture["cases"].as_array().unwrap() {
            let Ok(Message::Execute { request: expected }) =
                serde_json::from_value(case["message"].clone())
            else {
                continue;
            };
            // Other versions are the worker's to refuse.
            if expected.version != VERSION {
                continue;
            }
            let session = transport
                .session(
                    AgentRef {
                        hamlet: expected.workspace.clone(),
                        number: expected.agent.into(),
                    },
                    Policy {
                        project: expected.project,
                        network: expected.network,
                        read_only: expected.read_only,
                    },
                )
                .unwrap();
            let mut built = session.request(
                expected.operation.clone(),
                expected.library.clone(),
            );
            built.id.clone_from(&expected.id);
            assert_eq!(
                serde_json::to_value(Message::Execute {
                    request: built
                })
                .unwrap(),
                case["message"],
                "{}",
                case["name"]
            );
            checked += 1;
        }
        assert!(checked > 0);
    }
}
