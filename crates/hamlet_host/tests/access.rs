//! What an actor reaches of its host: the realm folder and the values
//! the program that opened the host gave it.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Context as _;
use hamlet_actor::{
    Action, Actor, ActorConfig, Ctx, Handler, Registry,
};
use hamlet_actor_core::ActorFactory;
use hamlet_host::{Address, Cause, Host, HostOptions, HostRef};
use hamlet_testing::Scratch;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// The kind `kind`, an actor of the trait API `A`, with a database.
fn kind<A: Actor>(registry: &mut Registry, kind: &str) {
    registry.register_actor_with::<A>(
        kind,
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
}

/// Calls `action` of the actor at `to` from the embedder, and decodes
/// its answer.
async fn call<M: Action>(
    host: &Host,
    to: &Address,
    action: &M,
) -> anyhow::Result<M::Output>
where
    M::Output: DeserializeOwned,
{
    let reply = host
        .call(
            to,
            M::NAME,
            hamlet_actor::action::encode_positional(action)?,
            Duration::from_secs(10),
            Cause::root("test"),
        )
        .await?;
    Ok(ciborium::from_reader(reply.as_slice())?)
}

/// Keeps the realm folder its host gave it in `create`.
struct Here {
    home: String,
}

#[derive(Serialize, Deserialize)]
struct Home;

impl Action for Home {
    type Output = String;
    const NAME: &'static str = "home";
}

#[async_trait::async_trait]
impl Actor for Here {
    type Input = String;
    type Actions = (Home,);
    type Events = ();
    type ConnParams = ();
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        let host = HostRef::of(ctx.inner())?;
        Ok(Self {
            home: host.folder().display().to_string(),
        })
    }
}

impl Handler<Home> for Here {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Home,
    ) -> anyhow::Result<String> {
        Ok(self.home.clone())
    }
}

/// P4's test of what the workspace needs (G4), its call and its
/// assertion as P4 wrote them.
#[tokio::test(flavor = "multi_thread")]
async fn an_actor_reaches_the_realm_folder_of_its_host() {
    let dir = Scratch::new("gap-home");
    let mut registry = Registry::new();
    registry.register_actor_with::<Here>(
        "here",
        ActorConfig {
            has_database: true,
            ..ActorConfig::default()
        },
    );
    let host = Host::open(&dir, registry.into_factories()).unwrap();
    let reply = host
        .call(
            &Address::new("here", "one"),
            Home::NAME,
            hamlet_actor::action::encode_positional(&Home).unwrap(),
            Duration::from_secs(10),
            Cause::root("test"),
        )
        .await;
    host.shutdown().await.unwrap();
    let home: String =
        ciborium::from_reader(reply.unwrap().as_slice()).unwrap();
    assert_eq!(home, dir.display().to_string());
}

/// A value the embedder gives its host. Each read of it by an actor
/// counts one.
struct Tally(AtomicUsize);

impl Tally {
    fn read(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// Reads the host's [`Tally`] in `create`, and again in its handler.
struct Reader {
    tally: Arc<Tally>,
}

/// Reads the [`Tally`] again. Answers whether it is the one `create`
/// read, and whether the host holds a `String`, which nobody gave.
#[derive(Serialize, Deserialize)]
struct Read;

impl Action for Read {
    type Output = (bool, bool);
    const NAME: &'static str = "read";
}

#[async_trait::async_trait]
impl Actor for Reader {
    type Input = ();
    type Actions = (Read,);
    type Events = ();
    type ConnParams = ();
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        let tally = HostRef::of(ctx.inner())?
            .value::<Tally>()
            .context("the host holds no tally")?;
        tally.read();
        Ok(Self { tally })
    }
}

impl Handler<Read> for Reader {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Read,
    ) -> anyhow::Result<(bool, bool)> {
        let host = HostRef::of(ctx.inner())?;
        let tally = host
            .value::<Tally>()
            .context("the host holds no tally")?;
        tally.read();
        Ok((
            Arc::ptr_eq(&tally, &self.tally),
            host.value::<String>().is_some(),
        ))
    }
}

/// V1: the value an actor reads in `create` and in a handler is the
/// one the embedder gave, and a type nobody gave reads as none.
#[tokio::test(flavor = "multi_thread")]
async fn an_actor_reads_its_hosts_values_in_create_and_in_a_handler()
{
    let dir = Scratch::new("access-values");
    let tally = Arc::new(Tally(AtomicUsize::new(0)));
    let mut registry = Registry::new();
    kind::<Reader>(&mut registry, "reader");
    let host = Host::open_with(
        &dir,
        registry.into_factories(),
        HostOptions::default().with_value(tally.clone()),
    )
    .unwrap();
    let read = call(&host, &Address::new("reader", "a"), &Read).await;
    host.shutdown().await.unwrap();
    assert_eq!(read.unwrap(), (true, false));
    assert_eq!(tally.0.load(Ordering::SeqCst), 2);
}

/// Keeps its [`HostRef`] for as long as it runs.
struct Keeper {
    _host: HostRef,
}

#[derive(Serialize, Deserialize)]
struct Ping;

impl Action for Ping {
    type Output = ();
    const NAME: &'static str = "ping";
}

#[async_trait::async_trait]
impl Actor for Keeper {
    type Input = ();
    type Actions = (Ping,);
    type Events = ();
    type ConnParams = ();
    type Action = ();

    async fn create(ctx: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self {
            _host: HostRef::of(ctx.inner())?,
        })
    }
}

impl Handler<Ping> for Keeper {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Ping,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

fn keepers() -> std::collections::HashMap<String, ActorFactory> {
    let mut registry = Registry::new();
    kind::<Keeper>(&mut registry, "keeper");
    registry.into_factories()
}

/// V2: an actor that keeps its [`HostRef`] does not keep its host:
/// once the last `Host` is dropped, a new host opens the folder.
#[tokio::test(flavor = "multi_thread")]
async fn an_actor_that_keeps_its_host_ref_does_not_keep_the_host() {
    let dir = Scratch::new("access-keeper");
    let host = Host::open(&dir, keepers()).unwrap();
    call(&host, &Address::new("keeper", "a"), &Ping)
        .await
        .unwrap();
    drop(host);
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(2);
    let host = loop {
        match Host::open(&dir, keepers()) {
            Ok(host) => break host,
            Err(error) => {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "the folder is still locked: {error:#}"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    };
    host.shutdown().await.unwrap();
}
