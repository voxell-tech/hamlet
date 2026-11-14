//! The route table: a call or make it does not allow fails with
//! `actor.refused`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use call::call;
use code::code;
use files::any_file_of;
use hamlet_actor::{
    Action, Actor, ActorConfig, Ctx, Handler, Registry,
};
use hamlet_actor_error::ActorError;
use hamlet_host::route::{CallerKind, Operation, Routes};
use hamlet_host::{
    Address, Caller, Host, HostEvent, HostOptions, HostRef,
};
use hamlet_testing::Scratch;
use logs::Logs;
use serde::{Deserialize, Serialize};
use tracing_subscriber::filter::LevelFilter;

#[path = "support/call.rs"]
mod call;
#[path = "support/code.rs"]
mod code;
#[path = "support/files.rs"]
mod files;
#[path = "support/logs.rs"]
mod logs;

const KINDS: [&str; 4] = ["realm", "workspace", "profile", "agent"];

const DEADLINE: Duration = Duration::from_secs(10);

fn routes() -> Routes {
    Routes::only(&[
        (CallerKind::Actor("realm".to_owned()), &KINDS),
        (CallerKind::Actor("workspace".to_owned()), &KINDS),
        (CallerKind::Actor("profile".to_owned()), &KINDS),
        (
            CallerKind::Actor("agent".to_owned()),
            &["realm", "workspace", "profile"],
        ),
        (CallerKind::Client, &["realm", "agent"]),
    ])
}

/// Ids of the actors whose ping ran, in order.
#[derive(Default)]
struct Pinged(Mutex<Vec<String>>);

struct Peer;

#[derive(Serialize, Deserialize)]
struct Ping;

impl Action for Ping {
    type Output = ();
    const NAME: &'static str = "ping";
}

#[derive(Serialize, Deserialize)]
struct Ask {
    to: Address,
}

impl Action for Ask {
    type Output = Option<Failure>;
    const NAME: &'static str = "ask";
}

#[derive(Serialize, Deserialize)]
struct Make {
    kind: String,
    key: String,
}

impl Action for Make {
    type Output = Option<Failure>;
    const NAME: &'static str = "make";
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Failure {
    code: String,
    metadata: String,
}

fn failure<T>(result: anyhow::Result<T>) -> Option<Failure> {
    result.err().map(|error| Failure {
        code: code(&error),
        metadata: ActorError::extract(&error)
            .metadata()
            .unwrap_or_default()
            .to_string(),
    })
}

#[async_trait::async_trait]
impl Actor for Peer {
    type Input = ();
    type Actions = (Ping, Ask, Make);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(_: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self)
    }
}

impl Handler<Ping> for Peer {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Ping,
    ) -> anyhow::Result<()> {
        if let Some(pinged) =
            HostRef::of(ctx.inner())?.value::<Pinged>()
        {
            pinged
                .0
                .lock()
                .unwrap()
                .push(ctx.inner().actor_id().to_owned());
        }
        Ok(())
    }
}

impl Handler<Ask> for Peer {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        ask: Ask,
    ) -> anyhow::Result<Option<Failure>> {
        Ok(failure(
            Host::call_from(
                ctx.inner(),
                &ask.to,
                Ping::NAME,
                hamlet_actor::action::encode_positional(&Ping)?,
                DEADLINE,
            )
            .await,
        ))
    }
}

impl Handler<Make> for Peer {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        make: Make,
    ) -> anyhow::Result<Option<Failure>> {
        Ok(failure(
            Host::create_from(
                ctx.inner(),
                &make.kind,
                &make.key,
                Vec::new(),
            )
            .await,
        ))
    }
}

fn open(dir: &Scratch, pinged: Arc<Pinged>) -> Host {
    let mut registry = Registry::new();
    for kind in KINDS {
        registry.register_actor_with::<Peer>(
            kind,
            ActorConfig {
                has_database: true,
                ..ActorConfig::default()
            },
        );
    }
    let options = HostOptions {
        routes: routes(),
        ..HostOptions::default()
    };
    Host::open_with(
        dir,
        registry.into_factories(),
        options.with_value(pinged),
    )
    .unwrap()
}

async fn make(host: &Host, at: &[&Address]) {
    for at in at {
        host.create(&at.kind, &at.key, Vec::new()).await.unwrap();
    }
}

async fn ask(
    host: &Host,
    from: &Address,
    to: &Address,
) -> Option<Failure> {
    call(host, from, &Ask { to: to.clone() }).await.unwrap()
}

/// An agent's call to another agent fails with `actor.refused`,
/// naming both kinds, and the callee's action does not run. The host
/// reports the refusal on its events and in its log.
#[tokio::test]
async fn an_agent_reaches_the_realm_workspace_and_profile_but_not_another_agent()
 {
    let dir = Scratch::new("route-agent");
    let pinged = Arc::<Pinged>::default();
    let host = open(&dir, pinged.clone());
    let a = Address::new("agent", "a");
    let b = Address::new("agent", "b");
    let others = [
        Address::new("realm", "r"),
        Address::new("workspace", "w"),
        Address::new("profile", "p"),
    ];
    make(&host, &[&a, &b]).await;
    make(&host, &others.each_ref()).await;
    let mut events = host.events();
    let logs = Logs::default();
    let logging = logs.record(LevelFilter::WARN);
    let refused = ask(&host, &a, &b).await;
    let mut answered = Vec::new();
    for to in &others {
        answered.push(ask(&host, &a, to).await);
    }
    drop(logging);
    host.shutdown().await.unwrap();

    let refused = refused.expect("another agent answered");
    assert_eq!(refused.code, "actor.refused");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&refused.metadata)
            .unwrap(),
        serde_json::json!({
            "operation": "call",
            "from": {"actor": "agent"},
            "to": "agent",
        })
    );
    assert_eq!(answered, [None, None, None]);
    assert_eq!(
        *pinged.0.lock().unwrap(),
        ["realm/r", "workspace/w", "profile/p"]
    );
    let mut reported = Vec::new();
    while let Ok(event) = events.try_recv() {
        reported.push(event);
    }
    assert_eq!(
        reported,
        [HostEvent::Refused {
            from: Caller::Actor(a),
            to: b,
            operation: Operation::Call {
                action: Ping::NAME.to_owned(),
            },
        }]
    );
    let logged = logs.text();
    assert!(
        logged.lines().any(|line| {
            ["WARN", "agent/a", "agent/b", Ping::NAME]
                .iter()
                .all(|word| line.contains(word))
        }),
        "{logged}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_reaches_the_realm_and_an_agent_but_not_the_workspace_or_profile()
 {
    let dir = Scratch::new("route-client");
    let host = open(&dir, Arc::default());
    let at = KINDS.map(|kind| Address::new(kind, "x"));
    make(&host, &at.each_ref()).await;
    let mut outcomes = Vec::new();
    for to in &at {
        let called = host
            .call_for_client(
                to,
                Ping::NAME,
                hamlet_actor::action::encode_positional(&Ping)
                    .unwrap(),
                DEADLINE,
            )
            .await;
        outcomes.push((
            to.kind.clone(),
            failure(called).map(|failure| failure.code),
        ));
    }
    host.shutdown().await.unwrap();
    let refused = Some("actor.refused".to_owned());
    assert_eq!(
        outcomes,
        [
            ("realm".to_owned(), None),
            ("workspace".to_owned(), refused.clone()),
            ("profile".to_owned(), refused),
            ("agent".to_owned(), None),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agents_create_of_another_agent_is_refused_and_leaves_no_file()
 {
    let dir = Scratch::new("route-create");
    let host = open(&dir, Arc::default());
    let a = Address::new("agent", "a");
    make(&host, &[&a]).await;
    let made = call(
        &host,
        &a,
        &Make {
            kind: "agent".to_owned(),
            key: "b".to_owned(),
        },
    )
    .await
    .unwrap();
    host.shutdown().await.unwrap();
    assert_eq!(
        made.map(|failure| failure.code).as_deref(),
        Some("actor.refused")
    );
    assert!(!any_file_of(&dir, "agent", "b"));
}
