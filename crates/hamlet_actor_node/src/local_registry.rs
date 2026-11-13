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
        host.boot().await.map_err(napi_anyhow_error)
    }

    /// Calls `action` on the actor of `kind` and `key`, starting it
    /// first if it is not running.
    #[napi]
    pub async fn action(
        &self,
        kind: String,
        key: String,
        action: String,
        args: Buffer,
    ) -> napi::Result<Buffer> {
        let host = self.host()?;
        let actor_id = host
            .get_or_start(&kind, &key)
            .await
            .map_err(napi_anyhow_error)?;
        let output = host
            .action(&actor_id, &action, args.to_vec())
            .await
            .map_err(napi_anyhow_error)?;
        Ok(output.into())
    }

    #[napi]
    pub async fn shutdown(&self) -> napi::Result<()> {
        let host = self.host.lock().take();
        if let Some(host) = host {
            host.stop_all().await.map_err(napi_anyhow_error)?;
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
