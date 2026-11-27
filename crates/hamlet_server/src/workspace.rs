//! A remote workspace (C18): another machine that keeps a copy of
//! each agent's filesystem at `/lodge`. Requests carry a bearer
//! token; virtual paths never reach the host.

pub(crate) mod lodges;

use std::time::Duration;

use hamlet_messages::realm::HamletId;
use hamlet_messages::workspace::AgentNumber;
use hamlet_workspace_wire::{
    Call, Code, Library, Message, Operation, Reply, Request, VERSION,
};

use crate::credentials::WorkspaceAddress;

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

    /// The requests of `agent`, which works in `workspace`, under
    /// `policy`.
    pub fn session(
        &self,
        agent: AgentNumber,
        workspace: &str,
        policy: Policy,
    ) -> Session {
        Session {
            transport: self.clone(),
            workspace: workspace.to_owned(),
            agent: agent.0,
            policy,
            library: None,
        }
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
pub(crate) fn workspace_of(hamlet: Option<&HamletId>) -> String {
    hamlet
        .map_or_else(|| "realm".to_owned(), |hamlet| hamlet.0.clone())
}
