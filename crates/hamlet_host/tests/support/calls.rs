//! A Rust host whose kinds call the stand-in's `agent` and answer
//! it, and the stand-in run by the test itself as its Rust host.

use std::collections::HashMap;
use std::io::{PipeReader, PipeWriter, Read};
use std::path::Path;
use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Context;
use hamlet_actor::action::encode_positional;
use hamlet_actor::{
    Action, Actor, ActorConfig, Ctx, Handler, Registry,
};
use hamlet_actor_core::ActorFactory;
use hamlet_host::child::{LinkState, Supervisor};
use hamlet_host::link::{self, Frame, PROTOCOL, WireError};
use hamlet_host::route::Routes;
use hamlet_host::{Address, Caller, Host, HostOptions, HostRef};
use hamlet_os::process::{Pipes, Tree};
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

pub use crate::actions::*;
use crate::linked::{child, own_stand_in, wait_for};

/// How long a test waits for what it set going.
pub const WAIT: Duration = Duration::from_secs(10);

/// The deadline of the tests' calls.
pub const DEADLINE: Duration = Duration::from_secs(10);

/// The Rust host's kinds.
pub const KINDS: [&str; 2] = ["realm", "workspace"];

/// The kinds `kinds`, each made by [`Peer`].
pub fn factories(kinds: &[&str]) -> HashMap<String, ActorFactory> {
    let mut registry = Registry::new();
    for kind in kinds {
        registry.register_actor_with::<Peer>(
            kind,
            ActorConfig {
                has_database: true,
                ..ActorConfig::default()
            },
        );
    }
    registry.into_factories()
}

/// A Rust host of [`KINDS`] on `home` with `options`, with the
/// stand-in run with `args` as its child.
pub fn open(
    home: &Path,
    options: HostOptions,
    args: &[&str],
) -> (Host, Supervisor) {
    hamlet_host::child::open(
        home,
        factories(&KINDS),
        options,
        child(args),
    )
    .unwrap()
}

/// [`open`] under `routes`, once the link is ready.
pub async fn ready(
    home: &Path,
    routes: Routes,
    args: &[&str],
) -> (Host, Supervisor) {
    let (host, node) = open(
        home,
        HostOptions {
            routes,
            ..HostOptions::default()
        },
        args,
    );
    wait_for(&mut node.state(), LinkState::Ready).await;
    (host, node)
}

/// Shuts the stand-in down, which exits cleanly, then the host.
pub async fn close(host: Host, node: Supervisor) {
    let status = node.shutdown().await.expect("a child ran");
    assert!(status.success(), "{status}");
    host.shutdown().await.unwrap();
}

/// Makes `actors` from a Rust host with no child, which lets the
/// folder go.
pub async fn made(home: &Path, actors: &[&Address]) {
    let host = Host::open(home, factories(&KINDS)).unwrap();
    for at in actors {
        host.create(at, Vec::new()).await.unwrap();
    }
    host.shutdown().await.unwrap();
}

/// What `check` gives once it gives something, within [`WAIT`].
pub async fn eventually<T>(
    what: &str,
    mut check: impl FnMut() -> Option<T>,
) -> T {
    let checking = async {
        loop {
            if let Some(found) = check() {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(WAIT, checking)
        .await
        .unwrap_or_else(|_| panic!("{what} never happened"))
}

/// How many starts the stand-in recorded in `path`.
pub fn starts(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .count()
}

/// The names of the frames the stand-in recorded reading in
/// `path`. Once it has read the shutdown's goodbye, which follows
/// every frame written before it, they are all it was sent.
pub fn frames_read(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The input of an agent whose start calls the `note` of `note`.
pub fn agent_input(note: Option<&Address>) -> Vec<u8> {
    let mut input = Vec::new();
    ciborium::into_writer(&note, &mut input).unwrap();
    input
}

/// The call of `action` that a [`Relay`] makes of `to`.
pub fn relay<M: Action>(to: &Address, action: &M) -> Relay {
    Relay {
        to: to.clone(),
        name: M::NAME.to_owned(),
        args: encode_positional(action).unwrap(),
        deadline_ms: u64::try_from(DEADLINE.as_millis()).unwrap(),
    }
}

impl Seen {
    pub fn output(output: impl Serialize) -> Self {
        Self::Output(serde_json::to_value(output).unwrap())
    }

    /// The error's `group.code`, as `actor.refused`.
    pub fn code(&self) -> Option<String> {
        match self {
            Self::Output(_) => None,
            Self::Error { group, code, .. } => {
                Some(format!("{group}.{code}"))
            }
        }
    }
}

/// How many notes the actor took, this one included.
#[derive(Serialize, Deserialize)]
pub struct Note;

impl Action for Note {
    type Output = u64;
    const NAME: &'static str = "note";
}

/// Makes the agent `at` with [`agent_input`] of `input`.
#[derive(Serialize, Deserialize)]
pub struct Make {
    pub at: Address,
    pub input: Option<Address>,
}

impl Action for Make {
    type Output = Seen;
    const NAME: &'static str = "make";
}

/// The name of the thread the action runs on.
#[derive(Serialize, Deserialize)]
pub struct Thread;

impl Action for Thread {
    type Output = Option<String>;
    const NAME: &'static str = "thread";
}

/// A text of `bytes` bytes.
#[derive(Serialize, Deserialize)]
pub struct Large {
    pub bytes: usize,
}

impl Action for Large {
    type Output = String;
    const NAME: &'static str = "large";
}

/// An actor of the Rust host's kinds.
pub struct Peer {
    notes: AtomicU64,
}

#[async_trait::async_trait]
impl Actor for Peer {
    type Input = ();
    type Actions = (Who, Fail, Note, Relay, Make, Thread, Large);
    type Events = ();
    type ConnParams = Option<Caller>;
    type Action = ();

    async fn create(_: &Ctx<Self>) -> anyhow::Result<Self> {
        Ok(Self {
            notes: AtomicU64::new(0),
        })
    }
}

impl Handler<Who> for Peer {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Who,
    ) -> anyhow::Result<Option<Caller>> {
        ctx.conn().context("the call has no connection")?.params()
    }
}

impl Handler<Fail> for Peer {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        fail: Fail,
    ) -> anyhow::Result<()> {
        Err(fail.into_error())
    }
}

impl Handler<Note> for Peer {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Note,
    ) -> anyhow::Result<u64> {
        Ok(self.notes.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

impl Handler<Relay> for Peer {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        relay: Relay,
    ) -> anyhow::Result<Seen> {
        relay.run(ctx.inner()).await
    }
}

impl Handler<Make> for Peer {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        make: Make,
    ) -> anyhow::Result<Seen> {
        let made = HostRef::of(ctx.inner())?
            .create(&make.at, agent_input(make.input.as_ref()))
            .await;
        Ok(Seen::of(made.map(|()| serde_json::Value::Null)))
    }
}

impl Handler<Thread> for Peer {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: Thread,
    ) -> anyhow::Result<Option<String>> {
        Ok(std::thread::current().name().map(str::to_owned))
    }
}

impl Handler<Large> for Peer {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        large: Large,
    ) -> anyhow::Result<String> {
        Ok("x".repeat(large.bytes))
    }
}

/// The stand-in, run by the test itself as its Rust host.
pub struct OwnStandIn {
    tree: Tree,
    input: PipeWriter,
    /// `None` once the test closed it.
    output: Option<PipeReader>,
    errors: PipeReader,
}

impl OwnStandIn {
    /// The stand-in run with `args` on `home`.
    pub fn start(home: &Path, args: &[&str]) -> Self {
        let (
            tree,
            Pipes {
                input,
                output,
                errors,
            },
        ) = own_stand_in(home, args);
        Self {
            tree,
            input,
            output: Some(output),
            errors,
        }
    }

    pub fn write(&mut self, frame: &Frame) {
        link::write(&mut self.input, frame).unwrap();
    }

    /// The next frame it writes, within [`WAIT`].
    pub async fn read(&mut self) -> Frame {
        let mut output =
            self.output.take().expect("its output is closed");
        let reading = tokio::task::spawn_blocking(move || {
            let read = link::read(&mut output);
            (output, read)
        });
        let (output, read) = tokio::time::timeout(WAIT, reading)
            .await
            .expect("the stand-in wrote nothing")
            .unwrap();
        self.output = Some(output);
        read.unwrap().expect("the stand-in's output ended")
    }

    /// Takes its hello, welcomes it as the Rust host of `kinds`
    /// under `routes`, and takes its `Ready`.
    pub async fn welcome(&mut self, kinds: &[&str], routes: Routes) {
        let hello = self.read().await;
        assert!(matches!(hello, Frame::Hello { .. }), "{hello:?}");
        self.write(&Frame::Welcome {
            protocol: PROTOCOL,
            fingerprint: link::fingerprint().to_owned(),
            kinds: kinds
                .iter()
                .map(|kind| (*kind).to_owned())
                .collect(),
            routes,
        });
        assert_eq!(self.read().await, Frame::Ready);
    }

    /// The frames it writes before its reply to `id`, and the
    /// reply.
    pub async fn until_reply(
        &mut self,
        id: u64,
    ) -> (Vec<Frame>, Result<ByteBuf, WireError>) {
        let mut before = Vec::new();
        loop {
            match self.read().await {
                Frame::Reply { id: to, result } if to == id => {
                    return (before, result);
                }
                frame => before.push(frame),
            }
        }
    }

    pub fn close_output(&mut self) {
        self.output = None;
    }

    /// How it ended, failing past `within`, and what it wrote to
    /// its standard error.
    pub async fn ended(
        mut self,
        within: Duration,
    ) -> (ExitStatus, String) {
        let status = tokio::time::timeout(within, self.tree.ended())
            .await
            .expect("the stand-in outlived the wait")
            .unwrap();
        let mut errors = String::new();
        self.errors.read_to_string(&mut errors).unwrap();
        (status, errors)
    }
}
