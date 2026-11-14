//! The route table, with Hamlet's: an agent reaches the realm, the
//! workspace and the profile, and never another agent; a client
//! reaches the realm and an agent. The host refuses any other call or
//! make from a caller with `actor.refused`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use call::call;
use code::code;
use files::any_file_of;
use hamlet_actor::{
    Action, Actor, ActorConfig, Ctx, Handler, Registry,
};
use hamlet_actor_error::ActorError;
use hamlet_host::route::{CallerKind, Routes};
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

/// Hamlet's route table.
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

/// The ids of the actors whose ping ran, in order, in a host that
/// holds it.
#[derive(Default)]
struct Pinged(Mutex<Vec<String>>);

/// An actor of any kind: it answers a ping, and pings and makes other
/// actors as itself.
struct Peer;

#[derive(Serialize, Deserialize)]
struct Ping;

impl Action for Ping {
    type Output = ();
    const NAME: &'static str = "ping";
}

/// Pings the actor at `to`, and answers how that went.
#[derive(Serialize, Deserialize)]
struct Ask {
    to: Address,
}

impl Action for Ask {
    type Output = Outcome;
    const NAME: &'static str = "ask";
}

/// Makes the actor of `kind` and `key`, and answers how that went.
#[derive(Serialize, Deserialize)]
struct Make {
    kind: String,
    key: String,
}

impl Action for Make {
    type Output = Outcome;
    const NAME: &'static str = "make";
}

/// How an operation went: `None` when it was answered, else its
/// error's `group.code` and the JSON of its metadata.
type Outcome = Option<(String, String)>;

fn outcome<T>(result: anyhow::Result<T>) -> Outcome {
    result.err().map(|error| {
        (
            code(&error),
            ActorError::extract(&error)
                .metadata()
                .unwrap_or_default()
                .to_string(),
        )
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
    ) -> anyhow::Result<Outcome> {
        Ok(outcome(
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
    ) -> anyhow::Result<Outcome> {
        Ok(outcome(
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

/// A host of Hamlet's kinds and table on `dir`, which records each
/// ping in `pinged`.
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

/// Makes each actor of `at`, with no caller.
async fn make(host: &Host, at: &[&Address]) {
    for at in at {
        host.create(&at.kind, &at.key, Vec::new()).await.unwrap();
    }
}

/// Has the actor at `from` ping the actor at `to`, and answers how
/// that went.
async fn ask(host: &Host, from: &Address, to: &Address) -> Outcome {
    call(host, from, &Ask { to: to.clone() }).await.unwrap()
}

/// An agent's call to another agent fails with `actor.refused`,
/// naming both kinds, and the callee's action does not run. The host
/// reports the refusal on its events and in its log. The agent's
/// calls to the realm, the workspace and the profile are answered.
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

    let (code, metadata) = refused.expect("another agent answered");
    assert_eq!(code, "actor.refused");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&metadata).unwrap(),
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
            action: Some(Ping::NAME.to_owned()),
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

/// A client's call is answered at the realm and at an agent, and
/// refused at the workspace and at the profile.
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
            outcome(called).map(|(code, _)| code),
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

/// An agent's create of another agent fails with `actor.refused`, and
/// leaves no file for its key.
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
        made.map(|(code, _)| code).as_deref(),
        Some("actor.refused")
    );
    assert!(!any_file_of(&dir, "agent", "b"));
}
