// This file was changed for Hamlet. See NOTICE in this crate.
//! The registry class, on Hamlet's local host: actors run in this
//! process, each in its own SQLite file under one folder.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use hamlet_actor_core::ActorFactory;
use hamlet_host::{Address, Host};
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use parking_lot::Mutex;

use crate::actor_factory::NapiActorFactory;
use crate::{NapiInvalidState, napi_anyhow_error};

#[napi]
pub struct CoreRegistry {
    factories: Mutex<HashMap<String, Arc<ActorFactory>>>,
    host: Mutex<Option<Host>>,
}

#[napi]
impl CoreRegistry {
    #[napi(constructor)]
    pub fn new() -> Self {
        crate::init_tracing(None);
        Self {
            factories: Mutex::new(HashMap::new()),
            host: Mutex::new(None),
        }
    }

    #[napi]
    pub fn register(
        &self,
        name: String,
        factory: &NapiActorFactory,
    ) -> napi::Result<()> {
        self.factories.lock().insert(name, factory.actor_factory());
        Ok(())
    }

    /// Opens the host in `dir` with the registered kinds and starts
    /// every actor whose stored alarm is due. Returns their ids.
    #[napi]
    pub async fn start(
        &self,
        dir: String,
    ) -> napi::Result<Vec<String>> {
        let factories = std::mem::take(&mut *self.factories.lock())
            .into_iter()
            .map(|(kind, factory)| (kind, owned(factory)))
            .collect();
        let host = Host::open(Path::new(&dir), factories)
            .map_err(napi_anyhow_error)?;
        *self.host.lock() = Some(host.clone());
        let began = host.boot().await.map_err(napi_anyhow_error)?;
        Ok(began.iter().map(ToString::to_string).collect())
    }

    /// Makes the actor of `kind` and `key` with `input`, the CBOR
    /// its kind's state is made from, and starts it. Fails with
    /// `actor.duplicate_key` when the key has an actor. Returns its
    /// id.
    #[napi]
    pub async fn create(
        &self,
        kind: String,
        key: String,
        input: Buffer,
    ) -> napi::Result<String> {
        let at = address(kind, key)?;
        self.host()?
            .create(&at, input.to_vec())
            .await
            .map_err(napi_anyhow_error)?;
        Ok(at.to_string())
    }

    /// The actor of `kind` and `key`, made with no input when the key
    /// has none, and started. Returns its id. For tests: a host's own
    /// code makes an actor with `create`.
    #[napi]
    pub async fn get_or_start(
        &self,
        kind: String,
        key: String,
    ) -> napi::Result<String> {
        let at = address(kind, key)?;
        self.host()?
            .get_or_start(&at)
            .await
            .map_err(napi_anyhow_error)?;
        Ok(at.to_string())
    }

    /// Calls the action `name` of the actor of `kind` and `key`,
    /// started from its file first if it is not running, within
    /// the deadline, in milliseconds. A call never makes its actor:
    /// for a key with no actor it fails with `actor.not_found`.
    #[napi]
    pub async fn call(
        &self,
        kind: String,
        key: String,
        name: String,
        args: Buffer,
        deadline_ms: u32,
    ) -> napi::Result<Buffer> {
        let at = address(kind, key)?;
        let output = self
            .host()?
            .call(
                &at,
                &name,
                args.to_vec(),
                Duration::from_millis(deadline_ms.into()),
            )
            .await
            .map_err(napi_anyhow_error)?;
        Ok(output.into())
    }

    /// Shuts the host down: stops every actor and lets the folder go.
    #[napi]
    pub async fn shutdown(&self) -> napi::Result<()> {
        let host = self.host.lock().take();
        if let Some(host) = host {
            host.shutdown().await.map_err(napi_anyhow_error)?;
        }
        Ok(())
    }

    fn host(&self) -> napi::Result<Host> {
        self.host.lock().clone().ok_or_else(|| {
            napi_anyhow_error(
                NapiInvalidState {
                    state: "registry".to_owned(),
                    reason: "not started".to_owned(),
                }
                .build(),
            )
        })
    }
}

impl Default for CoreRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// The address of `kind` and `key`. A name that is not plain fails
/// with its text.
fn address(kind: String, key: String) -> napi::Result<Address> {
    Address::new(kind, key)
        .map_err(|error| napi_anyhow_error(error.into()))
}

/// The host owns its factories, while the N-API factory object keeps
/// its own handle, so each kind gets a factory that starts through
/// the shared one.
fn owned(factory: Arc<ActorFactory>) -> ActorFactory {
    ActorFactory::new_with_manual_startup_ready(
        factory.config().clone(),
        move |start| {
            let factory = Arc::clone(&factory);
            Box::pin(async move { factory.start(start).await })
        },
    )
    .with_streaming_request_body()
}
