//! Pinned Substrate control client and durable actor identity
//! binding.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tonic::Status;
use tonic::transport::{
    Certificate, Channel, ClientTlsConfig, Endpoint,
};

#[allow(
    clippy::doc_lazy_continuation,
    reason = "generated from the pinned proto"
)]
pub mod proto {
    tonic::include_proto!("ateapi");
}

use proto::control_client::ControlClient;
use proto::{
    Actor, ActorState, CreateActorRequest, DeleteActorRequest,
    DeleteOptions, GetActorRequest, ObjectRef, ResourceMetadata,
    ResumeActorRequest, SuspendActorRequest,
};

/// Durable record of one workspace's actor.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Binding {
    pub name: String,
    /// Immutable actor UID, unset until an actor has been observed.
    pub uid: Option<String>,
    #[serde(default)]
    pub lifecycle: Lifecycle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion: Option<Deletion>,
}

/// Last lifecycle intent the manager persisted for a [`Binding`].
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq,
)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    #[default]
    Active,
    Suspending,
    Suspended,
}

/// Administrative deletion tombstone of a [`Binding`].
#[derive(
    Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq,
)]
#[serde(rename_all = "snake_case")]
pub enum Deletion {
    Requested,
    Completed,
}

impl Binding {
    pub fn new(key: &str) -> Self {
        Self {
            name: format!("hamlet-{}", &key[..48]),
            uid: None,
            lifecycle: Lifecycle::Active,
            deletion: None,
        }
    }
}

/// The identity fields of an [`Actor`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorView {
    pub atespace: String,
    pub name: String,
    pub uid: String,
    pub template_atespace: String,
    pub template: String,
    pub state: i32,
}

impl ActorView {
    pub fn running(&self) -> bool {
        self.state == ActorState::Running as i32
    }

    pub fn suspended(&self) -> bool {
        self.state == ActorState::Suspended as i32
            || self.state == ActorState::Suspending as i32
    }
}

/// Actor lifecycle calls by name within one atespace.
pub trait Actors {
    async fn get(&mut self, name: &str) -> Result<ActorView, Status>;
    async fn create(
        &mut self,
        name: &str,
    ) -> Result<ActorView, Status>;
    async fn suspend(
        &mut self,
        name: &str,
    ) -> Result<ActorView, Status>;
    async fn resume(
        &mut self,
        name: &str,
    ) -> Result<ActorView, Status>;
    /// Deletes only while the actor still has `uid`.
    async fn delete(
        &mut self,
        name: &str,
        uid: &str,
    ) -> Result<ActorView, Status>;
}

/// Substrate's `Control` gRPC service.
pub struct Control {
    client: ControlClient<Channel>,
    token: Option<String>,
    token_file: Option<PathBuf>,
    atespace: String,
    template: String,
}

/// Private control-plane trust and certificate identity.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tls {
    ca: PathBuf,
    server_name: String,
}

impl Control {
    pub fn new(
        url: &str,
        token: Option<String>,
        token_file: Option<PathBuf>,
        atespace: String,
        template: String,
        tls: Option<&Tls>,
    ) -> Result<Self> {
        if token.is_some() && token_file.is_some() {
            bail!("Choose a control token or token file, not both");
        }
        safe_url(url)?;
        let mut endpoint = Endpoint::from_shared(url.to_owned())?
            .timeout(std::time::Duration::from_secs(20));
        if url.starts_with("https:") {
            let trust = match tls {
                Some(tls) => ClientTlsConfig::new()
                    .ca_certificate(Certificate::from_pem(fs::read(
                        &tls.ca,
                    )?))
                    .domain_name(&tls.server_name),
                None => ClientTlsConfig::new().with_native_roots(),
            };
            endpoint = endpoint.tls_config(trust)?;
        } else if tls.is_some() {
            bail!("Control TLS requires an HTTPS endpoint");
        }
        Ok(Self {
            client: ControlClient::new(endpoint.connect_lazy()),
            token,
            token_file,
            atespace,
            template,
        })
    }

    fn reference(&self, name: &str) -> Option<ObjectRef> {
        Some(ObjectRef {
            atespace: self.atespace.clone(),
            name: name.to_owned(),
        })
    }

    fn request<T>(
        &self,
        body: T,
    ) -> Result<tonic::Request<T>, Status> {
        let mut request = tonic::Request::new(body);
        let token = match &self.token_file {
            Some(path) => {
                Some(fs::read_to_string(path).map_err(|_| {
                    Status::unauthenticated(
                        "Control token file unavailable",
                    )
                })?)
            }
            None => self.token.clone(),
        };
        if let Some(token) = token {
            let token = token.trim();
            if token.is_empty() {
                return Err(Status::unauthenticated(
                    "Empty control token",
                ));
            }
            request.metadata_mut().insert(
                "authorization",
                format!("Bearer {token}").parse().map_err(|_| {
                    Status::invalid_argument("Invalid control token")
                })?,
            );
        }
        Ok(request)
    }
}

impl Actors for Control {
    async fn get(&mut self, name: &str) -> Result<ActorView, Status> {
        let request = self.request(GetActorRequest {
            actor: self.reference(name),
        })?;
        view(Some(self.client.get_actor(request).await?.into_inner()))
    }

    async fn create(
        &mut self,
        name: &str,
    ) -> Result<ActorView, Status> {
        let request = self.request(CreateActorRequest {
            actor: Some(Actor {
                metadata: Some(ResourceMetadata {
                    atespace: self.atespace.clone(),
                    name: name.to_owned(),
                    ..Default::default()
                }),
                actor_template: self
                    .reference(&self.template.clone()),
                ..Default::default()
            }),
        })?;
        view(Some(
            self.client.create_actor(request).await?.into_inner(),
        ))
    }

    async fn suspend(
        &mut self,
        name: &str,
    ) -> Result<ActorView, Status> {
        let request = self.request(SuspendActorRequest {
            actor: self.reference(name),
        })?;
        view(
            self.client
                .suspend_actor(request)
                .await?
                .into_inner()
                .actor,
        )
    }

    async fn resume(
        &mut self,
        name: &str,
    ) -> Result<ActorView, Status> {
        let request = self.request(ResumeActorRequest {
            actor: self.reference(name),
        })?;
        view(
            self.client
                .resume_actor(request)
                .await?
                .into_inner()
                .actor,
        )
    }

    async fn delete(
        &mut self,
        name: &str,
        uid: &str,
    ) -> Result<ActorView, Status> {
        let request = self.request(DeleteActorRequest {
            actor: self.reference(name),
            any_state: true,
            options: Some(DeleteOptions {
                version: 0,
                uid: uid.to_owned(),
            }),
        })?;
        view(Some(
            self.client.delete_actor(request).await?.into_inner(),
        ))
    }
}

/// A reply without identity is treated as an unknown outcome.
fn view(actor: Option<Actor>) -> Result<ActorView, Status> {
    let unknown = || Status::unknown("Actor reply has no identity");
    let actor = actor.ok_or_else(unknown)?;
    let metadata = actor.metadata.ok_or_else(unknown)?;
    let template = actor.actor_template.ok_or_else(unknown)?;
    Ok(ActorView {
        atespace: metadata.atespace,
        name: metadata.name,
        uid: metadata.uid,
        template_atespace: template.atespace,
        template: template.name,
        state: actor.status.map_or(0, |status| status.state),
    })
}

/// Fails unless `actor` is the bound actor built from `template`.
pub fn verify(
    binding: &Binding,
    actor: &ActorView,
    atespace: &str,
    template: &str,
) -> Result<()> {
    if actor.name != binding.name
        || actor.atespace != atespace
        || actor.uid.is_empty()
        || actor.template != template
        || actor.template_atespace != atespace
        || binding.uid.as_ref().is_some_and(|uid| uid != &actor.uid)
    {
        bail!(
            "Actor identity or template changed; explicit recovery \
             is required"
        );
    }
    Ok(())
}

/// Stable binding key of a workspace within a home.
pub fn key(home: &str, workspace: &str) -> String {
    format!("{:x}", Sha256::digest(format!("{home}\0{workspace}")))
}

pub fn binding_path(state: &Path, key: &str) -> PathBuf {
    state.join(format!("actor-{key}.json"))
}

pub fn load(path: &Path) -> Result<Option<Binding>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

pub fn save(path: &Path, value: &impl Serialize) -> Result<()> {
    let temporary = path.with_extension("pending");
    let mut file = fs::File::create(&temporary)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    fs::File::open(path.parent().context("State has no parent")?)?
        .sync_all()?;
    Ok(())
}

pub fn safe_url(url: &str) -> Result<()> {
    let url = reqwest::Url::parse(url)?;
    let loopback = matches!(
        url.host_str(),
        Some("127.0.0.1" | "[::1]" | "localhost")
    );
    if !(url.scheme() == "https"
        || (url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!(
            "Workspace endpoints require HTTPS or loopback HTTP \
             without URL credentials"
        );
    }
    Ok(())
}
