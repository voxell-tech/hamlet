// This file was changed for Hamlet. See NOTICE in this crate.
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use anyhow::Result;
use hamlet_actor_core::metrics::{
    RenderedMetrics, render_prometheus_metrics,
};
use hamlet_actor_core::{
    ActorConfig, ActorFactory as CoreActorFactory, ActorStart,
};
use hamlet_actor_error::ActorError;

use crate::actor::Actor;
use crate::start::{Start, run_actor, wrap_start};

pub struct Registry {
    inner: HashMap<String, CoreActorFactory>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    #[deprecated(note = "use register_actor/register_actor_with \
                         and implement Actor + Handler instead")]
    pub fn register<A, F, Fut>(
        &mut self,
        name: &str,
        entry: F,
    ) -> &mut Self
    where
        A: Actor,
        F: Fn(Start<A>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        #[allow(deprecated)]
        self.register_with::<A, F, Fut>(
            name,
            ActorConfig::default(),
            entry,
        )
    }

    pub fn register_actor<A>(&mut self, name: &str) -> &mut Self
    where
        A: Actor,
    {
        self.register_actor_with::<A>(name, ActorConfig::default())
    }

    pub fn register_actor_with<A>(
        &mut self,
        name: &str,
        config: ActorConfig,
    ) -> &mut Self
    where
        A: Actor,
    {
        #[allow(deprecated)]
        self.register_with::<A, _, _>(name, config, run_actor::<A>)
    }

    #[deprecated(note = "use register_actor/register_actor_with \
                         and implement Actor + Handler instead")]
    pub fn register_with<A, F, Fut>(
        &mut self,
        name: &str,
        config: ActorConfig,
        entry: F,
    ) -> &mut Self
    where
        A: Actor,
        F: Fn(Start<A>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.inner.insert(
            name.to_owned(),
            build_factory::<A, F, Fut>(config, entry),
        );
        self
    }

    /// Renders the process-global Prometheus metrics. Equivalent to
    /// the TypeScript `registry.routes.prometheusMetrics()`. Metrics
    /// are process-global, so this does not depend on registry
    /// instance state.
    /// The registered actors' factories, by name, for a host to run.
    pub fn into_factories(self) -> HashMap<String, CoreActorFactory> {
        self.inner
    }

    pub fn prometheus_metrics(&self) -> Result<RenderedMetrics> {
        render_prometheus_metrics()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

fn build_factory<A, F, Fut>(
    config: ActorConfig,
    entry: F,
) -> CoreActorFactory
where
    A: Actor,
    F: Fn(Start<A>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    let config = actor_config::<A>(config);
    let entry = Arc::new(entry);
    CoreActorFactory::new_with_manual_startup_ready(
        config,
        move |core_start: ActorStart| {
            let entry = Arc::clone(&entry);
            Box::pin(async move {
                let mut core_start = core_start;
                let startup_ready = core_start.startup_ready.take();
                match wrap_start::<A>(core_start) {
                    Ok(mut start) => {
                        start.startup_ready = startup_ready;
                        entry(start).await
                    }
                    Err(error) => {
                        if let Some(reply) = startup_ready {
                            let startup_error = anyhow::Error::new(
                                ActorError::extract(&error),
                            );
                            let _ = reply.send(Err(startup_error));
                        }
                        Err(error)
                    }
                }
            })
        },
    )
}

fn actor_config<A: Actor>(mut config: ActorConfig) -> ActorConfig {
    // Every Rust actor gets a database; `ctx.sql()` is the only
    // durable storage API.
    config.has_database = true;
    // Internal storage (state, KV, queue) always uses SQLite, so
    // without local SQLite compiled in every actor must route it
    // through the engine.
    if !cfg!(feature = "sqlite-local") {
        config.remote_sqlite = true;
    }
    config
}
