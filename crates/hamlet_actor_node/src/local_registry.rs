// This file was changed for Hamlet. See NOTICE in this crate.
//! The registry class, on Hamlet's local host: actors run in this
//! process, each in its own SQLite file under one folder.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use hamlet_actor_core::ActorFactory;
use hamlet_host::Host;
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use parking_lot::Mutex;

use crate::actor_factory::NapiActorFactory;
use crate::address::{JsAddress, JsCallOptions};
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

    /// Serves the link to the Rust host that started this process,
    /// with the registered kinds, until its goodbye. Call `takeLink`
    /// first. The registry's own `create`, `getOrStart` and `call`
    /// stay unstarted: the Rust host makes and calls this host's
    /// actors.
    #[napi]
    pub async fn serve_link(&self) -> napi::Result<()> {
        let factories = std::mem::take(&mut *self.factories.lock())
            .into_iter()
            .map(|(kind, factory)| (kind, owned(factory)))
            .collect();
        hamlet_host::link::serve(factories)
            .await
            .map_err(napi_anyhow_error)
    }

    /// Makes the actor at `to` with `input`, the CBOR its kind's
    /// state is made from, and starts it. Fails with
    /// `actor.duplicate_key` when the key has an actor. Returns its
    /// id.
    #[napi]
    pub async fn create(
        &self,
        to: JsAddress,
        input: Buffer,
    ) -> napi::Result<String> {
        let at = to.checked()?;
        self.host()?
            .create(&at, input.to_vec())
            .await
            .map_err(napi_anyhow_error)?;
        Ok(at.to_string())
    }

    /// The actor at `to`, made with no input when the key has none,
    /// and started. Returns its id. For tests: a host's own code
    /// makes an actor with `create`.
    #[napi]
    pub async fn get_or_start(
        &self,
        to: JsAddress,
    ) -> napi::Result<String> {
        let at = to.checked()?;
        self.host()?
            .get_or_start(&at)
            .await
            .map_err(napi_anyhow_error)?;
        Ok(at.to_string())
    }

    /// Calls the action `name` of the actor at `to` with no caller,
    /// started from its file first if it is not running. A call never
    /// makes its actor: for a key with no actor it fails with
    /// `actor.not_found`.
    #[napi]
    pub async fn call(
        &self,
        to: JsAddress,
        name: String,
        args: Buffer,
        options: JsCallOptions,
    ) -> napi::Result<Buffer> {
        let at = to.checked()?;
        let deadline = options.deadline()?;
        let output = self
            .host()?
            .call(&at, &name, args.to_vec(), deadline)
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
