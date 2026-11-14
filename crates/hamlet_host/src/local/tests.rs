use hamlet_actor::action::encode_positional;
use hamlet_actor::{Action, Actor, Ctx, Handler, Registry};
use hamlet_actor_core::{ActorConfig, ActorStart};
use hamlet_testing::Scratch;
use serde::{Deserialize, Serialize};

use super::*;
use crate::Caller;
use crate::link::{Role, Side, Vetted};
use crate::route::{CallerKind, Operation};
use crate::testing::code;

const DEADLINE: Duration = Duration::from_secs(10);

/// The kind `waker`, whose run handler sets the run wake deadline
/// `wake_at` and then answers only lifecycle events.
fn waker(wake_at: i64) -> HashMap<String, ActorFactory> {
    let config = ActorConfig {
        name: Some("waker".to_owned()),
        has_database: true,
        ..ActorConfig::default()
    };
    HashMap::from([(
        "waker".to_owned(),
        ActorFactory::new(config, move |start: ActorStart| {
            Box::pin(async move {
                let ActorStart {
                    ctx, mut events, ..
                } = start;
                ctx.set_run_wake_at(Some(wake_at)).await?;
                while let Some(event) = events.recv().await {
                    counter::answer(event);
                }
                Ok(())
            })
        }),
    )])
}

/// The deadline a stop stores comes from the file as well as from
/// the generation's last alarm: none from a file whose core made
/// no tables, and the run wake deadline with no alarm pushed.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_reads_its_deadline_from_the_file() {
    let dir = Scratch::new("host-stop-deadline");
    let mut bare =
        plain_sqlite::open(&dir.join("bare.sqlite"), true).unwrap();
    bump_generation(
        &mut bare,
        "bare",
        "a",
        &Open::OrCreate,
        &OpenClaim::default(),
    )
    .unwrap();
    assert_eq!(mark_stopped(&bare, None).unwrap(), None);
    assert_eq!(mark_stopped(&bare, Some(5)).unwrap(), Some(5));

    let wake_at = now_ms() + 600_000;
    let host = Host::open(&dir, waker(wake_at)).unwrap();
    let a = Address::new("waker", "a").unwrap();
    host.get_or_start(&a).await.unwrap();
    host.stop(&a, ShutdownKind::Sleep).await.unwrap();
    assert!(matches!(
        host.inner().actors.lock().lives.get(&a),
        Some(Life::Stopped { wake_at: stopped, .. })
            if *stopped == wake_at
    ));
    drop(host);
    let file =
        plain_sqlite::open(&actor_path(&dir, &a), false).unwrap();
    assert_eq!(mark_stopped(&file, None).unwrap(), Some(wake_at));
}

/// The core of an ended generation keeps its handle. Through it,
/// that generation can neither stop nor destroy the actor's
/// running generation, give it an alarm, nor reach its file. If
/// `made_again`, the actor is destroyed and its key made again
/// first: the new file counts its generations from the start
/// again.
async fn an_ended_generations_handle_reaches_nothing(
    made_again: bool,
) {
    let dir = Scratch::new("host-ended-handle");
    let host = Host::open(
        &dir,
        counter::factories(&["counter"], counter::Sleep::WhenIdle),
    )
    .unwrap();
    let counter_a = Address::new("counter", "a").unwrap();
    host.get_or_start(&counter_a).await.unwrap();
    let id = actor_id(&counter_a);
    let running = || {
        host.inner().actors.lock().lives[&counter_a]
            .loaded()
            .unwrap()
            .clone()
    };
    let ended = {
        let generation = running();
        LocalHandle {
            host: Arc::downgrade(host.inner()),
            at: counter_a.clone(),
            generation: generation.id,
            file: generation.file.clone(),
            alarm: generation.alarm.clone(),
        }
    };
    host.stop(&counter_a, ShutdownKind::Sleep).await.unwrap();
    host.get_or_start(&counter_a).await.unwrap();
    if made_again {
        host.stop(&counter_a, ShutdownKind::Destroy).await.unwrap();
        host.get_or_start(&counter_a).await.unwrap();
    }
    let next = running();
    let call = |name: &'static str| {
        host.call(
            &counter_a,
            name,
            Vec::new(),
            Duration::from_secs(600),
        )
    };
    call("increment").await.unwrap();

    ended.sleep_actor(id.clone(), Some(1), None);
    ended.sleep_actor(id.clone(), Some(1), Some("crash".into()));
    ended.stop_actor(id.clone(), Some(1));
    let (ack, acked) = oneshot::channel();
    ended.set_alarm_with_ack(
        id.clone(),
        Some(now_ms() + 60_000),
        Some(1),
        Some(ack),
    );
    acked
        .await
        .expect("the ended generation's alarm was not acked");
    assert!(
        ended
            .sqlite_execute(
                id.clone(),
                "UPDATE counter SET value = 100".to_owned(),
                None,
            )
            .await
            .is_err()
    );

    assert!(matches!(
        host.inner().actors.lock().lives.get(&counter_a),
        Some(Life::Running(generation))
            if Arc::ptr_eq(generation, &next)
                && generation.alarm.lock().is_none()
    ));
    let reply = call("get").await.unwrap();
    assert_eq!(counter::count(reply).unwrap(), 1);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_generations_handle_reaches_nothing_of_the_next() {
    an_ended_generations_handle_reaches_nothing(false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_generations_handle_reaches_nothing_of_its_key_made_again()
 {
    an_ended_generations_handle_reaches_nothing(true).await;
}

struct Callee;

#[derive(Serialize, Deserialize)]
struct Who;

impl Action for Who {
    type Output = Option<Caller>;
    const NAME: &'static str = "who";
}

#[derive(Serialize, Deserialize)]
struct GetOrStart {
    at: Address,
}

impl Action for GetOrStart {
    type Output = ();
    const NAME: &'static str = "get_or_start";
}

#[async_trait::async_trait]
impl Actor for Callee {
    type Input = ();
    type Actions = (Who, GetOrStart);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(_: &Ctx<Self>) -> Result<Self> {
        Ok(Self)
    }
}

impl Handler<Who> for Callee {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Who,
    ) -> Result<Option<Caller>> {
        ctx.conn().context("the call has no connection")?.params()
    }
}

impl Handler<GetOrStart> for Callee {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        GetOrStart { at }: GetOrStart,
    ) -> Result<()> {
        HostRef::of(ctx.inner())?.get_or_start(&at).await
    }
}

fn at(kind: &str, key: &str) -> Address {
    Address::new(kind, key).unwrap()
}

fn linked(
    dir: &Path,
    kinds: &[&str],
    routes: Routes,
) -> Result<Host> {
    let mut registry = Registry::new();
    for kind in kinds {
        registry.register_actor_with::<Callee>(
            kind,
            ActorConfig {
                has_database: true,
                ..ActorConfig::default()
            },
        );
    }
    Host::open_linked(
        dir,
        registry.into_factories(),
        HostOptions {
            routes,
            ..HostOptions::default()
        },
        Some(Port::new(Role::Rust, ["agent".to_owned()])),
    )
}

fn vetted(from: &Address) -> Vetted {
    Side::new(Role::Node, ["agent".to_owned()])
        .vet(Some(Caller::Actor(from.clone())))
        .unwrap()
}

async fn who(
    inbound: &Inbound,
    from: &Address,
    to: &Address,
) -> Result<Vec<u8>> {
    let args = encode_positional(&Who)?;
    inbound
        .call(vetted(from), to, Who::NAME, args, DEADLINE)
        .await
}

#[test]
fn a_kind_both_hosts_hold_is_refused_at_open() {
    let dir = Scratch::new("host-linked-both");
    let error = linked(&dir, &["realm", "agent"], Routes::Any)
        .err()
        .expect("a kind both hosts hold was let in");
    assert!(error.to_string().contains("`agent`"), "{error:#}");
    assert!(!dir.join("host.lock").exists());
    linked(&dir, &["realm"], Routes::Any).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_linked_kind_is_never_locked_or_booted() {
    let dir = Scratch::new("host-linked-boot");
    let host = linked(&dir, &["realm"], Routes::Any).unwrap();
    assert_eq!(host.boot().await.unwrap(), []);
    assert!(!dir.join("actors").join("agent").exists());

    for kind in ["realm", "agent"] {
        let folder = dir.join("actors").join(kind);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("x.sqlite"), "not a database")
            .unwrap();
    }
    let mut events = host.events();
    assert_eq!(host.boot().await.unwrap(), []);
    let mut unreadable = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let HostEvent::UnreadableFile { actor, .. } = event {
            unreadable.push(actor);
        }
    }
    assert_eq!(unreadable, [at("realm", "x")]);
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_peers_call_runs_here_stamped_with_its_vetted_caller() {
    let dir = Scratch::new("host-linked-peer-call");
    let host = linked(&dir, &["realm"], Routes::Any).unwrap();
    let inbound = Inbound::of(&host).unwrap();
    let (agent, realm) = (at("agent", "a"), at("realm", "r"));
    host.create(&realm, Vec::new()).await.unwrap();

    let reply = who(&inbound, &agent, &realm).await.unwrap();
    assert_eq!(
        ciborium::from_reader::<Option<Caller>, _>(reply.as_slice())
            .unwrap(),
        Some(Caller::Actor(agent.clone()))
    );

    let made = at("realm", "m");
    inbound
        .create(vetted(&agent), &made, Vec::new())
        .await
        .unwrap();
    let again = inbound
        .create(vetted(&agent), &made, Vec::new())
        .await
        .expect_err("a second create made the actor again");
    assert_eq!(code(&again), "actor.duplicate_key");
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_peers_operation_meets_the_route_table_again() {
    let dir = Scratch::new("host-linked-peer-route");
    let routes = Routes::only(&[(
        CallerKind::Actor("agent".to_owned()),
        &["workspace"],
    )]);
    let host = linked(&dir, &["realm"], routes).unwrap();
    let inbound = Inbound::of(&host).unwrap();
    let (agent, realm) = (at("agent", "a"), at("realm", "r"));
    host.create(&realm, Vec::new()).await.unwrap();
    let mut events = host.events();

    let called = who(&inbound, &agent, &realm).await.map(drop);
    let made = at("realm", "m");
    let created =
        inbound.create(vetted(&agent), &made, Vec::new()).await;
    for refused in [called, created] {
        let refused = refused
            .expect_err("the table let an agent reach the realm");
        assert_eq!(code(&refused), "actor.refused");
    }
    assert!(!actor_path(&dir, &made).exists());
    let mut reported = Vec::new();
    while let Ok(event) = events.try_recv() {
        reported.push(event);
    }
    assert_eq!(
        reported,
        [
            HostEvent::Refused {
                from: Caller::Actor(agent.clone()),
                to: realm,
                operation: Operation::Call {
                    action: Who::NAME.to_owned(),
                },
            },
            HostEvent::Refused {
                from: Caller::Actor(agent),
                to: made,
                operation: Operation::Create,
            },
        ]
    );
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_call_and_a_create_cross_the_link() {
    let dir = Scratch::new("host-linked-operations");
    let host = linked(&dir, &["realm"], Routes::Any).unwrap();
    let (agent, realm) = (at("agent", "a"), at("realm", "r"));
    host.create(&realm, Vec::new()).await.unwrap();
    let get_or_start =
        encode_positional(&GetOrStart { at: agent.clone() }).unwrap();

    let refused = [
        ("get_or_start", host.get_or_start(&agent).await.err()),
        (
            "get_or_start",
            host.call(
                &realm,
                GetOrStart::NAME,
                get_or_start,
                DEADLINE,
            )
            .await
            .err(),
        ),
        ("stop", host.stop(&agent, ShutdownKind::Sleep).await.err()),
        (
            "stop",
            host.stop(&agent, ShutdownKind::Destroy).await.err(),
        ),
        (
            "set_aside_timers",
            host.set_aside_timers(&agent).await.err(),
        ),
        ("rearm_timer", host.rearm_timer(&agent, "e", 0).await.err()),
    ];
    for (operation, error) in refused {
        let error = error.expect(operation);
        assert_eq!(
            code(&error),
            "actor.invalid_operation",
            "{error:#}"
        );
        assert_eq!(
            ActorError::extract(&error).metadata().unwrap()
                ["operation"],
            operation
        );
    }
    host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_peers_request_for_a_kind_of_its_own_is_not_sent_back() {
    let dir = Scratch::new("host-linked-peer-own-kind");
    let host = linked(&dir, &["realm"], Routes::Any).unwrap();
    let inbound = Inbound::of(&host).unwrap();
    let (a, b) = (at("agent", "a"), at("agent", "b"));

    let called = who(&inbound, &a, &b).await.err();
    let made = inbound.create(vetted(&a), &b, Vec::new()).await.err();
    for error in [called, made] {
        let error = error.expect("a peer's request was sent back");
        assert_eq!(code(&error), "actor.not_registered", "{error:#}");
    }
    host.shutdown().await.unwrap();
}
