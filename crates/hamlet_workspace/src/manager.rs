//! Authenticated workspace requests routed through manager-owned
//! actor bindings, with idle suspension and restart recovery. One
//! manager serves many homes; each home's token names it, and its
//! actors, worker files and operation records are its own.

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ring::signature::Ed25519KeyPair;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio::time::Instant;
use tonic::Code;

use crate::control::{
    self, ActorView, Actors, Binding, Control, Deletion, Lifecycle,
};
use crate::http::{self, Call};
use crate::wire::{
    self, Envelope, MAX_BODY, Message, Progress, Reply, State,
    valid_id, valid_name,
};

const IDLE: Duration = Duration::from_secs(300);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    listen: String,
    state: PathBuf,
    /// Each home's name and the bearer token its Hamlet sends.
    homes: HashMap<String, String>,
    signing_key: PathBuf,
    control: String,
    control_token: Option<String>,
    control_token_file: Option<PathBuf>,
    control_tls: Option<control::Tls>,
    router: String,
    router_ca: Option<PathBuf>,
    atespace: String,
    template: String,
    /// Seconds without operations before a workspace is suspended.
    #[serde(default)]
    idle_seconds: Option<u64>,
}

pub async fn run(config: Configuration) -> Result<()> {
    if config.idle_seconds == Some(0) {
        bail!("Invalid idle interval");
    }
    control::safe_url(&config.router)?;
    let _lock = lock(&config.state)?;
    let mut client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(160));
    if let Some(ca) = &config.router_ca {
        if !config.router.starts_with("https:") {
            bail!("Router CA requires an HTTPS endpoint");
        }
        for certificate in
            reqwest::Certificate::from_pem_bundle(&fs::read(ca)?)?
        {
            client = client.add_root_certificate(certificate);
        }
    }
    let router = Atenet {
        client: client.build()?,
        url: format!("{}/v1", config.router.trim_end_matches('/')),
        key: crate::signing_key(&config.signing_key)?,
    };
    let actors = Control::new(
        &config.control,
        config.control_token.clone(),
        config.control_token_file.clone(),
        config.atespace.clone(),
        config.template.clone(),
        config.control_tls.as_ref(),
    )?;
    let mut manager = Manager::open(&config, actors, router)?;
    let mut tasks = JoinSet::new();
    manager.reconcile(&mut tasks, None);
    let mut calls = http::listen(&config.listen).await?;
    loop {
        let deadline = manager.deadline();
        tokio::select! {
            Some(done) = tasks.join_next(), if !tasks.is_empty() => {
                if let Ok(done) = done {
                    manager.complete(done, Instant::now());
                }
            }
            Some(call) = calls.recv() => {
                manager.call(call, &mut tasks, Instant::now()).await;
            }
            () = tokio::time::sleep_until(
                deadline.unwrap_or_else(Instant::now),
            ), if deadline.is_some() => {
                manager.idle(&mut tasks, Instant::now()).await;
            }
            else => break,
        }
    }
    Ok(())
}

/// Each home by the digest of the authorization header its Hamlet
/// sends, so a token is never compared byte by byte.
fn tokens(
    homes: &HashMap<String, String>,
) -> Result<HashMap<[u8; 32], String>> {
    if homes.is_empty() {
        bail!("The manager serves no home");
    }
    let mut tokens = HashMap::new();
    for (home, token) in homes {
        if !valid_name(home) || token.len() < 16 {
            bail!("Invalid name or token of home {home:?}");
        }
        if tokens
            .insert(bearer(&format!("Bearer {token}")), home.clone())
            .is_some()
        {
            bail!("Two homes share a token");
        }
    }
    Ok(tokens)
}

fn bearer(authorization: &str) -> [u8; 32] {
    Sha256::digest(authorization).into()
}

/// Deletes a home's workspace actor only while it still has
/// `expected_uid`, leaving a tombstone that prevents recreating it.
/// Fails while a manager holds the same state.
pub async fn delete(
    config: &Configuration,
    home: &str,
    workspace: &str,
    expected_uid: &str,
) -> Result<()> {
    if !valid_name(home) || !valid_name(workspace) {
        bail!("Invalid home or workspace name");
    }
    let _lock = lock(&config.state)?;
    let mut actors = Control::new(
        &config.control,
        config.control_token.clone(),
        config.control_token_file.clone(),
        config.atespace.clone(),
        config.template.clone(),
        config.control_tls.as_ref(),
    )?;
    remove(
        &mut actors,
        &config.state,
        &control::key(home, workspace),
        expected_uid,
    )
    .await
}

async fn remove(
    actors: &mut impl Actors,
    state: &Path,
    key: &str,
    expected_uid: &str,
) -> Result<()> {
    let path = control::binding_path(state, key);
    let mut binding = control::load(&path)?
        .context("This workspace has no actor binding")?;
    if binding.uid.as_deref() != Some(expected_uid) {
        bail!(
            "The workspace binding does not hold actor UID \
             {expected_uid}; nothing was deleted"
        );
    }
    if binding.deletion == Some(Deletion::Completed) {
        return Ok(());
    }
    binding.deletion = Some(Deletion::Requested);
    control::save(&path, &binding)?;
    match actors.delete(&binding.name, expected_uid).await {
        Ok(actor) if actor.uid == expected_uid => {}
        Ok(_) => bail!(
            "Deletion reported a different actor UID; the tombstone \
             is kept"
        ),
        Err(status) if status.code() == Code::NotFound => {}
        Err(status) => bail!(
            "Actor deletion failed or is uncertain ({}); the \
             tombstone is kept and the deletion can be repeated",
            status.message()
        ),
    }
    binding.deletion = Some(Deletion::Completed);
    control::save(&path, &binding)
}

fn lock(state: &Path) -> Result<fs::File> {
    fs::create_dir_all(state)?;
    fs::set_permissions(state, fs::Permissions::from_mode(0o700))?;
    let lock = fs::File::create(state.join("lock"))?;
    // SAFETY: flock only reads the descriptor of the owned file.
    if unsafe {
        libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB)
    } != 0
    {
        bail!("Workspace state already has a manager");
    }
    Ok(lock)
}

/// Durable record of one operation id.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum Record {
    /// Cancellation received before any actor was involved.
    Cancelled { cancelled: bool },
    Operation {
        route: String,
        /// The settled reply; unset while the outcome is unknown.
        reply: Option<Reply>,
    },
}

/// Signs and forwards a home's messages to an actor's worker.
pub trait Router {
    fn forward(
        &self,
        route: String,
        actor_name: String,
        home: String,
        message: Message,
    ) -> impl Future<Output = Result<Reply>> + Send + 'static;
}

/// The atenet router.
struct Atenet {
    client: reqwest::Client,
    url: String,
    key: Ed25519KeyPair,
}

impl Router for Atenet {
    fn forward(
        &self,
        route: String,
        actor_name: String,
        home: String,
        message: Message,
    ) -> impl Future<Output = Result<Reply>> + Send + 'static {
        let signed = route
            .split_once('/')
            .context("Invalid actor route")
            .and_then(|(atespace, uid)| {
                self.sign(home, uid, message).map(|signed| {
                    (format!("{atespace}/{actor_name}"), signed)
                })
            });
        let client = self.client.clone();
        let url = self.url.clone();
        async move {
            let (target, (timestamp, signature, body)) = signed?;
            let mut response = client
                .post(url)
                .header("ate-target-actor", target)
                .header("x-workspace-time", timestamp)
                .header("x-workspace-signature", signature)
                .header("content-type", "application/json")
                .body(body)
                .send()
                .await?
                .error_for_status()?;
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if bytes.len() + chunk.len() > MAX_BODY {
                    bail!("Oversize worker reply");
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(serde_json::from_slice(&bytes)?)
        }
    }
}

impl Atenet {
    fn sign(
        &self,
        home: String,
        actor_uid: &str,
        message: Message,
    ) -> Result<(String, String, Vec<u8>)> {
        let body = serde_json::to_vec(&Envelope {
            home,
            actor_uid: actor_uid.to_owned(),
            message,
        })?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs()
            .to_string();
        let signed = [timestamp.as_bytes(), b"\n", &body].concat();
        let signature =
            STANDARD.encode(self.key.sign(&signed).as_ref());
        Ok((timestamp, signature, body))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Execute,
    Status,
    Cancel,
}

/// An operation id, which is unique only within its home.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Ticket {
    home: String,
    id: String,
}

/// A finished forward, handed back to the event loop.
struct Completion {
    ticket: Ticket,
    route: String,
    purpose: Purpose,
    result: Result<Reply>,
    reply: Option<oneshot::Sender<Reply>>,
}

struct Workspace {
    binding: Binding,
    last: Instant,
}

fn cancelled_before_admission() -> Reply {
    Reply::error(
        wire::Code::Aborted,
        "Workspace operation cancelled before admission. Nothing \
         was run.",
        false,
    )
}

/// Why a workspace actor cannot take an operation. Nothing was
/// forwarded.
#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Deleted,
    Missing,
    Changed,
    Unavailable,
}

impl Refusal {
    fn reply(&self) -> Reply {
        match self {
            Self::Deleted => Reply::error(
                wire::Code::NotFound,
                "This workspace was deleted by an administrator. \
                 Ask the chief before using it again.",
                false,
            ),
            Self::Missing => Reply::error(
                wire::Code::NotFound,
                "The workspace actor is missing and was not \
                 recreated, so its files are not available. Ask the \
                 chief to recover or delete it.",
                false,
            ),
            Self::Changed => Reply::error(
                wire::Code::Unavailable,
                "The workspace actor's identity or template \
                 changed. Ask the chief to inspect its binding \
                 before retrying.",
                false,
            ),
            Self::Unavailable => Reply::error(
                wire::Code::Unavailable,
                "The workspace actor could not be verified or \
                 resumed. Nothing was run; retry later or ask the \
                 chief to inspect it.",
                false,
            ),
        }
    }
}

/// State owned by the manager's single event loop.
struct Manager<A, R> {
    state: PathBuf,
    /// Each home by [`bearer`] of its authorization.
    homes: HashMap<[u8; 32], String>,
    atespace: String,
    template: String,
    idle: Duration,
    actors: A,
    router: R,
    workspaces: HashMap<String, Workspace>,
    /// Routes of operations with unknown outcomes.
    pending: HashMap<Ticket, String>,
    /// Forwards in flight, by route.
    inflight: HashMap<String, usize>,
}

impl<A: Actors, R: Router> Manager<A, R> {
    /// Loads bindings and refuses unreadable or foreign operation
    /// records.
    fn open(
        config: &Configuration,
        actors: A,
        router: R,
    ) -> Result<Self> {
        let now = Instant::now();
        let mut manager = Self {
            state: config.state.clone(),
            homes: tokens(&config.homes)?,
            atespace: config.atespace.clone(),
            template: config.template.clone(),
            idle: config
                .idle_seconds
                .map_or(IDLE, Duration::from_secs),
            actors,
            router,
            workspaces: HashMap::new(),
            pending: HashMap::new(),
            inflight: HashMap::new(),
        };
        for entry in fs::read_dir(&config.state)? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if let Some(key) = name
                .strip_prefix("actor-")
                .and_then(|name| name.strip_suffix(".json"))
            {
                let binding = control::load(&path)?
                    .with_context(|| format!("Unreadable {name}"))?;
                manager.workspaces.insert(
                    key.to_owned(),
                    Workspace { binding, last: now },
                );
            } else if name.starts_with("request-") {
                bail!(
                    "Operation record {name} of a single-home \
                     manager belongs in homes/<its home>/; refusing \
                     to start"
                );
            }
        }
        for home in config.homes.keys() {
            fs::create_dir_all(manager.records(home))?;
        }
        for folder in fs::read_dir(config.state.join("homes"))? {
            let folder = folder?.path();
            let home = folder
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|home| valid_name(home))
                .with_context(|| {
                    format!("Unexpected {}", folder.display())
                })?
                .to_owned();
            manager.load(&home, &folder)?;
        }
        Ok(manager)
    }

    /// Takes the unknown outcomes of one home's operation records.
    fn load(&mut self, home: &str, folder: &Path) -> Result<()> {
        for entry in fs::read_dir(folder)? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if let Some(id) = name
                .strip_prefix("request-")
                .and_then(|name| name.strip_suffix(".json"))
            {
                let record = serde_json::from_slice::<Record>(
                    &fs::read(&path)?,
                )
                .ok()
                .filter(|_| valid_id(id));
                let route = match record {
                    Some(Record::Operation {
                        route,
                        reply: None,
                    }) => route,
                    Some(Record::Operation {
                        reply: Some(_),
                        ..
                    })
                    | Some(Record::Cancelled { .. }) => {
                        continue;
                    }
                    None => bail!(
                        "Unreadable operation record {name}; \
                         refusing to start"
                    ),
                };
                if !self.routable(&route) {
                    bail!(
                        "Operation record {name} has a foreign \
                         route; refusing to start"
                    );
                }
                self.pending.insert(
                    Ticket {
                        home: home.to_owned(),
                        id: id.to_owned(),
                    },
                    route,
                );
            }
        }
        Ok(())
    }

    fn routable(&self, route: &str) -> bool {
        route
            .strip_prefix(&self.atespace)
            .and_then(|route| route.strip_prefix('/'))
            .is_some_and(|uid| {
                !uid.is_empty()
                    && uid.bytes().all(|c| {
                        c.is_ascii_alphanumeric() || c == b'-'
                    })
            })
    }

    fn route(&self, binding: &Binding) -> Option<String> {
        binding
            .uid
            .as_ref()
            .map(|uid| format!("{}/{uid}", self.atespace))
    }

    fn records(&self, home: &str) -> PathBuf {
        self.state.join("homes").join(home)
    }

    fn record(&self, ticket: &Ticket) -> PathBuf {
        self.records(&ticket.home)
            .join(format!("request-{}.json", ticket.id))
    }

    async fn call(
        &mut self,
        call: Call,
        tasks: &mut JoinSet<Completion>,
        now: Instant,
    ) {
        let Some(home) =
            self.homes.get(&bearer(&call.authorization)).cloned()
        else {
            let _ = call.reply.send(Reply::error(
                wire::Code::PermissionDenied,
                "Workspace authentication failed. Ask the chief to \
                 check the connection.",
                false,
            ));
            return;
        };
        let message =
            match serde_json::from_slice::<Message>(&call.body) {
                Ok(message) if valid_id(message.id()) => message,
                _ => {
                    let _ = call.reply.send(Reply::error(
                        wire::Code::Invalid,
                        "Invalid workspace request.",
                        false,
                    ));
                    return;
                }
            };
        let ticket = Ticket {
            home,
            id: message.id().to_owned(),
        };
        let path = self.record(&ticket);
        let purpose = match &message {
            Message::Execute { request } => {
                if request.version != 1
                    || !valid_name(&request.workspace)
                {
                    let _ = call.reply.send(Reply::error(
                        wire::Code::NotSupported,
                        "This workspace service supports protocol 1 \
                         only.",
                        false,
                    ));
                    return;
                }
                if self.pending.contains_key(&ticket) || path.exists()
                {
                    let cancelled =
                        fs::read(&path).ok().is_some_and(|bytes| {
                            matches!(
                                serde_json::from_slice::<Record>(
                                    &bytes
                                ),
                                Ok(Record::Cancelled { .. })
                            )
                        });
                    let _ = call.reply.send(if cancelled {
                        cancelled_before_admission()
                    } else {
                        Reply::uncertain()
                    });
                    return;
                }
                let key =
                    control::key(&ticket.home, &request.workspace);
                let route = match self.resolve(&key, now).await {
                    Ok(route) => route,
                    Err(refusal) => {
                        let _ = call.reply.send(refusal.reply());
                        return;
                    }
                };
                let record = Record::Operation {
                    route: route.clone(),
                    reply: None,
                };
                if control::save(&path, &record).is_err() {
                    let _ = call.reply.send(Reply::uncertain());
                    return;
                }
                self.pending.insert(ticket.clone(), route);
                Purpose::Execute
            }
            Message::Status { .. } => Purpose::Status,
            Message::Cancel { .. } => Purpose::Cancel,
        };
        let Some(route) = self.pending.get(&ticket).cloned() else {
            let reply = match fs::read(&path) {
                Ok(bytes) => {
                    match serde_json::from_slice::<Record>(&bytes) {
                        Ok(Record::Cancelled { .. }) => {
                            cancelled_before_admission()
                        }
                        Ok(Record::Operation {
                            reply: Some(reply),
                            ..
                        }) if purpose == Purpose::Status => reply,
                        Ok(Record::Operation {
                            reply: Some(_),
                            ..
                        }) => Reply::ok(Progress {
                            state: State::Completed,
                        }),
                        _ => Reply::uncertain(),
                    }
                }
                Err(error)
                    if error.kind()
                        == std::io::ErrorKind::NotFound =>
                {
                    if purpose == Purpose::Cancel {
                        if control::save(
                            &path,
                            &Record::Cancelled { cancelled: true },
                        )
                        .is_ok()
                        {
                            Reply::ok(Progress {
                                state: State::Cancelled,
                            })
                        } else {
                            Reply::uncertain()
                        }
                    } else {
                        Reply::error(
                            wire::Code::NotFound,
                            "Unknown workspace operation.",
                            false,
                        )
                    }
                }
                Err(_) => Reply::uncertain(),
            };
            let _ = call.reply.send(reply);
            return;
        };
        self.touch(&route, now);
        self.forward(
            tasks,
            ticket,
            route,
            message,
            purpose,
            Some(call.reply),
        );
    }

    /// Binds, verifies and resumes the workspace's actor, returning
    /// its route. Creation is retried only while no actor UID has
    /// been observed; actor names are unique, so retries cannot
    /// create a second actor.
    async fn resolve(
        &mut self,
        key: &str,
        now: Instant,
    ) -> Result<String, Refusal> {
        if !self.workspaces.contains_key(key) {
            self.persist(key, Binding::new(key), now)?;
        }
        let mut binding = self.workspaces[key].binding.clone();
        if binding.deletion.is_some() {
            return Err(Refusal::Deleted);
        }
        let actor = match self.actors.get(&binding.name).await {
            Ok(actor) => actor,
            Err(status)
                if status.code() == Code::NotFound
                    && binding.uid.is_none() =>
            {
                match self.actors.create(&binding.name).await {
                    Ok(actor) => actor,
                    Err(status)
                        if status.code() == Code::AlreadyExists =>
                    {
                        self.actors
                            .get(&binding.name)
                            .await
                            .map_err(|_| Refusal::Unavailable)?
                    }
                    Err(_) => return Err(Refusal::Unavailable),
                }
            }
            Err(status) if status.code() == Code::NotFound => {
                return Err(Refusal::Missing);
            }
            Err(_) => return Err(Refusal::Unavailable),
        };
        self.verify(&binding, &actor)?;
        if binding.uid.is_none() {
            binding.uid = Some(actor.uid.clone());
            self.persist(key, binding.clone(), now)?;
        }
        if !actor.running() {
            // ResumeActor addresses the actor by name only; the
            // reply's UID is checked so a replaced actor
            // is refused.
            let resumed = self
                .actors
                .resume(&binding.name)
                .await
                .map_err(|_| Refusal::Unavailable)?;
            self.verify(&binding, &resumed)?;
        }
        if binding.lifecycle != Lifecycle::Active {
            binding.lifecycle = Lifecycle::Active;
            self.persist(key, binding.clone(), now)?;
        }
        self.touch_key(key, now);
        self.route(&binding).ok_or(Refusal::Unavailable)
    }

    fn verify(
        &self,
        binding: &Binding,
        actor: &ActorView,
    ) -> Result<(), Refusal> {
        control::verify(
            binding,
            actor,
            &self.atespace,
            &self.template,
        )
        .map_err(|_| Refusal::Changed)
    }

    fn persist(
        &mut self,
        key: &str,
        binding: Binding,
        now: Instant,
    ) -> Result<(), Refusal> {
        control::save(
            &control::binding_path(&self.state, key),
            &binding,
        )
        .map_err(|_| Refusal::Unavailable)?;
        self.workspaces
            .entry(key.to_owned())
            .and_modify(|workspace| {
                workspace.binding = binding.clone();
            })
            .or_insert(Workspace { binding, last: now });
        Ok(())
    }

    fn touch(&mut self, route: &str, now: Instant) {
        let atespace = &self.atespace;
        for workspace in self.workspaces.values_mut() {
            if workspace.binding.uid.as_ref().is_some_and(|uid| {
                route.strip_prefix(atespace.as_str())
                    == Some(&format!("/{uid}"))
            }) {
                workspace.last = now;
            }
        }
    }

    fn touch_key(&mut self, key: &str, now: Instant) {
        if let Some(workspace) = self.workspaces.get_mut(key) {
            workspace.last = now;
        }
    }

    fn forward(
        &mut self,
        tasks: &mut JoinSet<Completion>,
        ticket: Ticket,
        route: String,
        message: Message,
        purpose: Purpose,
        reply: Option<oneshot::Sender<Reply>>,
    ) {
        let Some(workspace) =
            self.workspaces.values().find(|workspace| {
                self.route(&workspace.binding).as_deref()
                    == Some(&route)
                    && workspace.binding.deletion.is_none()
            })
        else {
            if let Some(reply) = reply {
                let _ = reply.send(Reply::uncertain());
            }
            return;
        };
        let actor_name = workspace.binding.name.clone();
        *self.inflight.entry(route.clone()).or_default() += 1;
        let result = self.router.forward(
            route.clone(),
            actor_name,
            ticket.home.clone(),
            message,
        );
        tasks.spawn(async move {
            Completion {
                ticket,
                route,
                purpose,
                result: result.await,
                reply,
            }
        });
    }

    /// Records a settled outcome before answering the caller.
    fn complete(&mut self, done: Completion, now: Instant) {
        if let Some(count) = self.inflight.get_mut(&done.route) {
            *count -= 1;
            if *count == 0 {
                self.inflight.remove(&done.route);
            }
        }
        let reply =
            done.result.unwrap_or_else(|_| Reply::uncertain());
        if settles(done.purpose, &reply)
            && self.pending.contains_key(&done.ticket)
        {
            let record = Record::Operation {
                route: done.route.clone(),
                reply: Some(reply.clone()),
            };
            if control::save(&self.record(&done.ticket), &record)
                .is_ok()
            {
                self.pending.remove(&done.ticket);
            }
        }
        if let Some(sender) = done.reply {
            self.touch(&done.route, now);
            let _ = sender.send(reply);
        }
    }

    /// Sends signed Status, never Execute, for unknown outcomes not
    /// already being forwarded. `only` limits it to one route and to
    /// records whose actor has no binding.
    fn reconcile(
        &mut self,
        tasks: &mut JoinSet<Completion>,
        only: Option<&str>,
    ) {
        let targets = self
            .pending
            .iter()
            .filter(|(_, route)| {
                !self.inflight.contains_key(*route)
                    && only.is_none_or(|only| {
                        only == route.as_str() || !self.bound(route)
                    })
            })
            .map(|(ticket, route)| (ticket.clone(), route.clone()))
            .collect::<Vec<_>>();
        for (ticket, route) in targets {
            let message = Message::Status {
                id: ticket.id.clone(),
            };
            self.forward(
                tasks,
                ticket,
                route,
                message,
                Purpose::Status,
                None,
            );
        }
    }

    fn bound(&self, route: &str) -> bool {
        self.workspaces.values().any(|workspace| {
            self.route(&workspace.binding).as_deref() == Some(route)
        })
    }

    /// An unknown outcome on an unbound route blocks every workspace.
    fn blocked(&self, route: &str) -> bool {
        self.inflight.contains_key(route)
            || self.pending.values().any(|pending| {
                pending == route || !self.bound(pending)
            })
    }

    fn deadline(&self) -> Option<Instant> {
        self.workspaces
            .values()
            .filter(|workspace| eligible(&workspace.binding))
            .map(|workspace| workspace.last + self.idle)
            .min()
    }

    /// Suspends idle workspaces inline, so new requests wait for the
    /// suspension to resolve.
    async fn idle(
        &mut self,
        tasks: &mut JoinSet<Completion>,
        now: Instant,
    ) {
        let due = self
            .workspaces
            .iter()
            .filter(|(_, workspace)| {
                eligible(&workspace.binding)
                    && workspace.last + self.idle <= now
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in due {
            let binding = self.workspaces[&key].binding.clone();
            let Some(route) = self.route(&binding) else {
                continue;
            };
            if self.blocked(&route) {
                self.reconcile(tasks, Some(&route));
            } else {
                self.suspend(&key, binding, now).await;
            }
            self.touch_key(&key, now);
        }
    }

    #[expect(
        clippy::print_stderr,
        reason = "stderr is the manager's log"
    )]
    async fn suspend(
        &mut self,
        key: &str,
        mut binding: Binding,
        now: Instant,
    ) {
        binding.lifecycle = Lifecycle::Suspending;
        if self.persist(key, binding.clone(), now).is_err() {
            return;
        }
        binding.lifecycle = match self.try_suspend(&binding).await {
            Ok(true) => Lifecycle::Suspended,
            Ok(false) => Lifecycle::Active,
            Err(refusal) => {
                eprintln!(
                    "Suspension of {} is unresolved ({refusal:?})",
                    binding.name
                );
                return;
            }
        };
        let _ = self.persist(key, binding, now);
    }

    /// SuspendActor addresses the actor by name only, so a replaced
    /// actor can be suspended between the check and the call; its
    /// reply is checked and identity drift fails closed.
    async fn try_suspend(
        &mut self,
        binding: &Binding,
    ) -> Result<bool, Refusal> {
        let actor = self
            .actors
            .get(&binding.name)
            .await
            .map_err(|_| Refusal::Unavailable)?;
        self.verify(binding, &actor)?;
        if actor.suspended() {
            return Ok(true);
        }
        if !actor.running() {
            return Ok(false);
        }
        let actor = self
            .actors
            .suspend(&binding.name)
            .await
            .map_err(|_| Refusal::Unavailable)?;
        self.verify(binding, &actor)?;
        Ok(true)
    }
}

fn eligible(binding: &Binding) -> bool {
    binding.uid.is_some()
        && binding.deletion.is_none()
        && binding.lifecycle != Lifecycle::Suspended
}

/// Whether a worker reply settles the operation. Status answers with
/// the worker's front-door rejections are not trusted as results.
fn settles(purpose: Purpose, reply: &Reply) -> bool {
    match reply {
        _ if purpose == Purpose::Cancel => false,
        Reply::Error {
            uncertain, code, ..
        } => {
            let rejected = purpose == Purpose::Status
                && matches!(
                    code,
                    wire::Code::PermissionDenied
                        | wire::Code::Invalid
                );
            !uncertain && !rejected
        }
        Reply::Ok { value } => !in_progress(value),
    }
}

fn in_progress(value: &Value) -> bool {
    serde_json::from_value::<Progress>(value.clone()).is_ok_and(
        |progress| {
            matches!(
                progress.state,
                State::Running | State::Cancelling
            )
        },
    )
}
