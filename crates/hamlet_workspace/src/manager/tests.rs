use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinSet;
use tokio::time::Instant;
use tonic::Status;

use super::*;
use crate::control::proto::ActorState;
use crate::wire::{Operation, Request};

const ATESPACE: &str = "space";
const TEMPLATE: &str = "template";
const TOKEN: &str = "0123456789abcdef";

/// Control plane fake. `lose` applies the named call and then
/// reports a lost reply.
#[derive(Default)]
struct Fake {
    actors: HashMap<String, ActorView>,
    log: Vec<&'static str>,
    lose: Option<&'static str>,
    uids: u32,
}

impl Fake {
    fn lost(&mut self, verb: &'static str) -> Result<(), Status> {
        self.log.push(verb);
        if self.lose == Some(verb) {
            self.lose = None;
            return Err(Status::unavailable("lost"));
        }
        Ok(())
    }

    fn set(&mut self, name: &str, state: ActorState) -> ActorView {
        let actor = self.actors.get_mut(name).expect("actor");
        actor.state = state as i32;
        actor.clone()
    }

    fn found(&self, name: &str) -> Result<ActorView, Status> {
        self.actors
            .get(name)
            .cloned()
            .ok_or_else(|| Status::not_found(name))
    }
}

impl Actors for Fake {
    async fn get(&mut self, name: &str) -> Result<ActorView, Status> {
        self.log.push("get");
        self.found(name)
    }

    async fn create(
        &mut self,
        name: &str,
    ) -> Result<ActorView, Status> {
        if self.actors.contains_key(name) {
            self.log.push("create");
            return Err(Status::already_exists(name));
        }
        self.uids += 1;
        let actor = ActorView {
            atespace: ATESPACE.into(),
            name: name.into(),
            uid: format!("uid-{}", self.uids),
            template_atespace: ATESPACE.into(),
            template: TEMPLATE.into(),
            state: ActorState::Suspended as i32,
        };
        self.actors.insert(name.into(), actor.clone());
        self.lost("create")?;
        Ok(actor)
    }

    async fn suspend(
        &mut self,
        name: &str,
    ) -> Result<ActorView, Status> {
        self.found(name)?;
        let actor = self.set(name, ActorState::Suspended);
        self.lost("suspend")?;
        Ok(actor)
    }

    async fn resume(
        &mut self,
        name: &str,
    ) -> Result<ActorView, Status> {
        self.found(name)?;
        let actor = self.set(name, ActorState::Running);
        self.lost("resume")?;
        Ok(actor)
    }

    async fn delete(
        &mut self,
        name: &str,
        uid: &str,
    ) -> Result<ActorView, Status> {
        let actor = self.found(name).inspect_err(|_| {
            self.log.push("delete");
        })?;
        if actor.uid != uid {
            self.log.push("delete");
            return Err(Status::failed_precondition("uid"));
        }
        self.actors.remove(name);
        self.lost("delete")?;
        Ok(actor)
    }
}

type Forwarded = (String, Message, oneshot::Sender<Result<Reply>>);

struct Fabric(mpsc::UnboundedSender<Forwarded>);

impl Router for Fabric {
    fn forward(
        &self,
        route: String,
        _actor_name: String,
        message: Message,
    ) -> impl Future<Output = Result<Reply>> + Send + 'static {
        let sender = self.0.clone();
        async move {
            let (reply, answer) = oneshot::channel();
            sender
                .send((route, message, reply))
                .map_err(|_| anyhow!("router closed"))?;
            answer.await.map_err(|_| anyhow!("dropped"))?
        }
    }
}

struct Harness {
    manager: Manager<Fake, Fabric>,
    router: mpsc::UnboundedReceiver<Forwarded>,
    tasks: JoinSet<Completion>,
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scratch() -> Scratch {
    let path = std::env::temp_dir()
        .join(format!("hamlet-manager-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&path).expect("scratch");
    Scratch(path)
}

fn config(state: &Path) -> Configuration {
    Configuration {
        listen: "127.0.0.1:0".into(),
        state: state.into(),
        home: "home".into(),
        token: TOKEN.into(),
        signing_key: PathBuf::new(),
        control: "http://127.0.0.1:1".into(),
        control_token: None,
        control_token_file: None,
        control_tls: None,
        router: "http://127.0.0.1:1".into(),
        router_ca: None,
        atespace: ATESPACE.into(),
        template: TEMPLATE.into(),
        idle_seconds: Some(10),
    }
}

fn open(state: &Path, fake: Fake) -> Harness {
    let (send, router) = mpsc::unbounded_channel();
    Harness {
        manager: Manager::open(&config(state), fake, Fabric(send))
            .expect("open"),
        router,
        tasks: JoinSet::new(),
    }
}

fn execute(id: &str) -> Message {
    Message::Execute {
        request: Request {
            version: 1,
            id: id.into(),
            workspace: "work".into(),
            agent: 1,
            project: false,
            network: false,
            read_only: false,
            library: None,
            operation: Operation::InitHouse,
        },
    }
}

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn later(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}

fn key() -> String {
    control::key("home", "work")
}

fn name() -> String {
    Binding::new(&key()).name
}

fn code(reply: &Reply) -> &str {
    match reply {
        Reply::Ok { .. } => "ok",
        Reply::Error { code, .. } => code,
    }
}

impl Harness {
    /// Admits `message` and returns its pending answer.
    async fn send(
        &mut self,
        message: Message,
    ) -> oneshot::Receiver<Reply> {
        let (reply, answer) = oneshot::channel();
        let call = Call {
            authorization: format!("Bearer {TOKEN}"),
            signature: String::new(),
            timestamp: String::new(),
            body: serde_json::to_vec(&message).expect("json"),
            reply,
        };
        self.manager
            .call(call, &mut self.tasks, Instant::now())
            .await;
        answer
    }

    async fn worker(
        &mut self,
    ) -> (String, Message, oneshot::Sender<Result<Reply>>) {
        tokio::time::timeout(
            Duration::from_secs(5),
            self.router.recv(),
        )
        .await
        .expect("forwarded in time")
        .expect("forwarded")
    }

    async fn settle(&mut self) {
        let done = tokio::time::timeout(
            Duration::from_secs(5),
            self.tasks.join_next(),
        )
        .await
        .expect("settled in time")
        .expect("task")
        .expect("join");
        self.manager.complete(done, Instant::now());
    }

    async fn idle(&mut self, at: Instant) {
        self.manager.idle(&mut self.tasks, at).await;
    }

    fn binding(&self) -> Binding {
        control::load(&control::binding_path(
            &self.manager.state,
            &key(),
        ))
        .expect("load")
        .expect("binding")
    }

    /// Runs one Execute to a settled `reply`.
    async fn run(&mut self, reply: Reply) -> Reply {
        let answer = self.send(execute(&id())).await;
        let (_, message, worker) = self.worker().await;
        assert!(matches!(message, Message::Execute { .. }));
        assert!(worker.send(Ok(reply)).is_ok());
        self.settle().await;
        answer.await.expect("answer")
    }
}

#[tokio::test]
async fn cancellation_before_admission_survives_restart() {
    let scratch = scratch();
    let mut harness = open(&scratch.0, Fake::default());
    let cancelled = id();
    let answer = harness
        .send(Message::Cancel {
            id: cancelled.clone(),
        })
        .await;
    assert_eq!(code(&answer.await.expect("cancel")), "ok");
    let answer = harness.send(execute(&cancelled)).await;
    assert_eq!(code(&answer.await.expect("execute")), "aborted");
    drop(harness);
    let mut harness = open(&scratch.0, Fake::default());
    let answer = harness.send(execute(&cancelled)).await;
    assert_eq!(
        code(&answer.await.expect("execute after restart")),
        "aborted"
    );
    let answer =
        harness.send(Message::Status { id: cancelled }).await;
    assert_eq!(code(&answer.await.expect("status")), "aborted");
    assert!(harness.router.try_recv().is_err());
    assert_eq!(
        code(&harness.run(Reply::ok(json!("normal"))).await),
        "ok"
    );

    let active = id();
    let running = harness.send(execute(&active)).await;
    let (_, _, execution) = harness.worker().await;
    let cancel =
        harness.send(Message::Cancel { id: active.clone() }).await;
    let (_, message, cancellation) = harness.worker().await;
    assert!(matches!(message, Message::Cancel { .. }));
    assert!(
        cancellation
            .send(Ok(Reply::ok(json!({"state": "cancelling"}))))
            .is_ok()
    );
    harness.settle().await;
    assert_eq!(code(&cancel.await.expect("cancel")), "ok");
    assert!(
        execution
            .send(Ok(Reply::error("aborted", "Cancelled", false)))
            .is_ok()
    );
    harness.settle().await;
    assert_eq!(code(&running.await.expect("execution")), "aborted");
    let duplicate = harness.send(execute(&active)).await;
    assert_eq!(
        code(&duplicate.await.expect("duplicate")),
        "uncertain"
    );
}

#[tokio::test]
async fn lost_create_reply_is_adopted_without_a_second_actor() {
    let dir = scratch();
    let mut h = open(
        &dir.0,
        Fake {
            lose: Some("create"),
            ..Fake::default()
        },
    );
    let answer = h.send(execute(&id())).await;
    assert_eq!(code(&answer.await.expect("answer")), "unavailable");
    assert!(h.manager.pending.is_empty());

    assert_eq!(code(&h.run(Reply::ok(json!({}))).await), "ok");
    assert_eq!(
        h.manager.actors.log,
        ["get", "create", "get", "resume"]
    );
    assert_eq!(h.manager.actors.actors.len(), 1);
    assert_eq!(h.binding().uid.as_deref(), Some("uid-1"));
}

#[tokio::test]
async fn missing_or_replaced_actor_is_reported_not_recreated() {
    let dir = scratch();
    let mut h = open(&dir.0, Fake::default());
    h.run(Reply::ok(json!({}))).await;
    let replaced =
        h.manager.actors.actors.remove(&name()).expect("actor");

    let answer = h.send(execute(&id())).await;
    assert_eq!(code(&answer.await.expect("answer")), "not_found");
    h.manager.actors.actors.insert(
        name(),
        ActorView {
            uid: "other".into(),
            ..replaced
        },
    );
    let answer = h.send(execute(&id())).await;
    assert_eq!(code(&answer.await.expect("answer")), "unavailable");
    assert_eq!(
        h.manager
            .actors
            .log
            .iter()
            .filter(|v| **v == "create")
            .count(),
        1
    );
    assert!(h.router.try_recv().is_err());
}

#[tokio::test]
async fn idle_suspension_waits_for_operations_and_requests_resume() {
    let dir = scratch();
    let mut h = open(&dir.0, Fake::default());
    h.run(Reply::ok(json!({}))).await;

    let answer = h.send(execute(&id())).await;
    let (_, _, worker) = h.worker().await;
    h.idle(later(60)).await;
    assert!(!h.manager.actors.log.contains(&"suspend"));

    assert!(worker.send(Ok(Reply::ok(json!({})))).is_ok());
    h.settle().await;
    answer.await.expect("answer");
    h.idle(later(60)).await;
    assert!(h.manager.actors.log.contains(&"suspend"));
    assert_eq!(h.binding().lifecycle, Lifecycle::Suspended);

    h.manager.actors.log.clear();
    assert_eq!(code(&h.run(Reply::ok(json!({}))).await), "ok");
    assert_eq!(h.manager.actors.log, ["get", "resume"]);
    assert_eq!(h.binding().lifecycle, Lifecycle::Active);
}

#[tokio::test]
async fn uncertain_suspension_is_reconciled_on_next_request() {
    let dir = scratch();
    let mut h = open(&dir.0, Fake::default());
    h.run(Reply::ok(json!({}))).await;
    h.manager.actors.lose = Some("suspend");
    h.idle(later(60)).await;
    assert_eq!(h.binding().lifecycle, Lifecycle::Suspending);

    h.manager.actors.log.clear();
    assert_eq!(code(&h.run(Reply::ok(json!({}))).await), "ok");
    assert_eq!(h.manager.actors.log, ["get", "resume"]);
    assert_eq!(h.binding().lifecycle, Lifecycle::Active);
}

#[tokio::test]
async fn resume_of_a_replaced_actor_is_refused() {
    let dir = scratch();
    let mut h = open(&dir.0, Fake::default());
    h.run(Reply::ok(json!({}))).await;
    h.idle(later(60)).await;
    h.manager
        .actors
        .actors
        .get_mut(&name())
        .expect("actor")
        .template = "other".into();
    let answer = h.send(execute(&id())).await;
    assert_eq!(code(&answer.await.expect("answer")), "unavailable");
    assert!(h.router.try_recv().is_err());
    assert_eq!(h.binding().lifecycle, Lifecycle::Suspended);
}

#[tokio::test]
async fn restart_reconciles_by_status_and_never_replays() {
    let dir = scratch();
    let operation = id();
    {
        let mut h = open(&dir.0, Fake::default());
        h.run(Reply::ok(json!({}))).await;
        let _answer = h.send(execute(&operation)).await;
        let _lost = h.worker().await;
    }
    let mut fake = Fake::default();
    fake.create(&name()).await.expect("actor");
    fake.set(&name(), ActorState::Running);
    let mut h = open(&dir.0, fake);
    assert!(h.manager.pending.contains_key(&operation));
    h.manager.reconcile(&mut h.tasks, None);

    let (_, message, worker) = h.worker().await;
    assert!(matches!(message, Message::Status { .. }));
    assert!(
        worker
            .send(Ok(Reply::ok(json!({ "state": "running" }))))
            .is_ok()
    );
    h.settle().await;
    h.idle(later(60)).await;
    assert!(!h.manager.actors.log.contains(&"suspend"));

    let (_, message, worker) = h.worker().await;
    assert!(matches!(message, Message::Status { .. }));
    let result = Reply::error("io", "failed", false);
    assert!(worker.send(Ok(result)).is_ok());
    h.settle().await;
    assert!(h.manager.pending.is_empty());

    let answer = h
        .send(Message::Status {
            id: operation.clone(),
        })
        .await;
    assert_eq!(code(&answer.await.expect("answer")), "io");
    let answer = h.send(execute(&operation)).await;
    assert_eq!(code(&answer.await.expect("answer")), "uncertain");
    assert!(h.router.try_recv().is_err());

    h.idle(later(120)).await;
    assert!(h.manager.actors.log.contains(&"suspend"));
}

#[tokio::test]
async fn untrusted_status_answers_keep_the_operation_unknown() {
    let dir = scratch();
    let mut h = open(&dir.0, Fake::default());
    h.run(Reply::ok(json!({}))).await;
    let operation = id();
    let answer = h.send(execute(&operation)).await;
    let (_, _, worker) = h.worker().await;
    drop(worker);
    h.settle().await;
    assert_eq!(code(&answer.await.expect("answer")), "uncertain");

    for reply in [
        Reply::uncertain(),
        Reply::error(
            "permission_denied",
            "Invalid workspace signature.",
            false,
        ),
        Reply::ok(json!({ "state": "cancelling" })),
    ] {
        let answer = h
            .send(Message::Status {
                id: operation.clone(),
            })
            .await;
        let (_, _, worker) = h.worker().await;
        assert!(worker.send(Ok(reply)).is_ok());
        h.settle().await;
        answer.await.expect("answer");
        assert!(h.manager.pending.contains_key(&operation));
    }
    h.idle(later(60)).await;
    assert!(!h.manager.actors.log.contains(&"suspend"));
}

#[tokio::test]
async fn staged_route_records_are_kept_and_foreign_ones_refused() {
    let dir = scratch();
    let operation = id();
    let binding = json!({ "name": name(), "uid": "uid-1" });
    fs::write(
        control::binding_path(&dir.0, &key()),
        binding.to_string(),
    )
    .expect("binding");
    let path = dir.0.join(format!("request-{operation}.json"));
    fs::write(&path, json!("space/uid-1").to_string())
        .expect("record");

    let mut h = open(&dir.0, Fake::default());
    assert_eq!(h.manager.pending[&operation], "space/uid-1");
    h.idle(later(60)).await;
    let (route, message, _) = h.worker().await;
    assert_eq!(route, "space/uid-1");
    assert!(matches!(message, Message::Status { .. }));
    assert!(h.manager.actors.log.is_empty());
    drop(h);

    fs::write(&path, json!("elsewhere/uid-1").to_string())
        .expect("record");
    let (send, _router) = mpsc::unbounded_channel();
    assert!(
        Manager::open(&config(&dir.0), Fake::default(), Fabric(send))
            .is_err()
    );
}

#[tokio::test]
async fn deletion_checks_uid_recovers_and_keeps_its_tombstone() {
    let dir = scratch();
    let mut h = open(&dir.0, Fake::default());
    h.run(Reply::ok(json!({}))).await;
    let Harness { manager, .. } = h;
    let mut fake = manager.actors;

    assert!(
        remove(&mut fake, &dir.0, &key(), "uid-2").await.is_err()
    );
    assert!(!fake.log.contains(&"delete"));

    fake.lose = Some("delete");
    assert!(
        remove(&mut fake, &dir.0, &key(), "uid-1").await.is_err()
    );
    let path = control::binding_path(&dir.0, &key());
    let binding =
        control::load(&path).expect("load").expect("binding");
    assert_eq!(binding.deletion, Some(Deletion::Requested));

    remove(&mut fake, &dir.0, &key(), "uid-1")
        .await
        .expect("recovered");
    let binding =
        control::load(&path).expect("load").expect("binding");
    assert_eq!(binding.deletion, Some(Deletion::Completed));

    let mut h = open(&dir.0, fake);
    h.manager.actors.log.clear();
    let answer = h.send(execute(&id())).await;
    assert_eq!(code(&answer.await.expect("answer")), "not_found");
    assert!(h.manager.actors.log.is_empty());
    assert!(h.manager.actors.actors.is_empty());
}
