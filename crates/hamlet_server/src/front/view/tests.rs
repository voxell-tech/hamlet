use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use hamlet_actor::{
    Action, Actor, ActorConfig, Ctx, Handler, Registry,
};
use hamlet_host::Host;
use hamlet_messages::Message;
use hamlet_messages::sender::Client;
use hamlet_protocol::client::State;
use hamlet_testing::Scratch;
use serde::{Deserialize, Serialize};

use super::*;
use crate::credentials::Credentials;
use crate::front::Keys;

/// A realm whose `read_state` reads its revision, then waits for the
/// test to let it go when the test holds the reads.
struct HeldRealm {
    revision: AtomicU64,
}

/// What the test holds a read with: told when the read began, and
/// released when the test sends.
type Hold = (oneshot::Sender<()>, oneshot::Receiver<()>);

static HOLD: Mutex<Option<Hold>> = Mutex::new(None);

/// Commits a change: the next revision, and its `World`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Commit;

impl Action for Commit {
    type Output = ();
    const NAME: &'static str = "commit";
}

impl Message for Commit {
    type To = Realm;
    type From = Client;
}

impl Actor for HeldRealm {
    type Input = ();
    type Actions = (ReadState, Commit);
    type Events = (World,);
    type ConnParams = Option<hamlet_host::Caller>;
    type Action = ();

    fn create<'ctx, 'made>(
        _: &'ctx Ctx<Self>,
    ) -> Pin<Box<dyn Future<Output = Result<Self>> + Send + 'made>>
    where
        'ctx: 'made,
    {
        Box::pin(async {
            Ok(Self {
                revision: AtomicU64::new(0),
            })
        })
    }
}

impl Handler<ReadState> for HeldRealm {
    async fn handle(
        self: Arc<Self>,
        _: Ctx<Self>,
        _: ReadState,
    ) -> Result<State> {
        let revision = self.revision.load(Ordering::SeqCst);
        let held = HOLD.lock().unwrap().take();
        if let Some((began, release)) = held {
            let _ = began.send(());
            let _ = release.await;
        }
        let mut state = hamlet_testing::realm::state();
        state.hamlets[0].name = format!("v{revision}");
        Ok(state)
    }
}

impl Handler<Commit> for HeldRealm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        _: Commit,
    ) -> Result<()> {
        let revision =
            self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        ctx.emit(World {
            revision: Revision(revision),
        })
    }
}

/// The name a client holding `state` is shown for the hamlet.
fn shown(state: &State) -> &str {
    &state.hamlets[0].name
}

/// Reads the frames to the client until it holds `name`.
async fn until_shown(
    frames: &mut mpsc::UnboundedReceiver<(u64, ServerMessage)>,
    state: &mut Option<State>,
    name: &str,
) {
    while state.as_ref().is_none_or(|state| shown(state) != name) {
        let (_, message) =
            frames.recv().await.expect("the view serves");
        match message {
            ServerMessage::State(whole) => *state = Some(whole),
            ServerMessage::Change(changed) => {
                let held = state.as_mut().expect("a State first");
                for update in &changed.changes {
                    held.apply(update);
                }
            }
            _ => {}
        }
    }
}

/// A `World` that comes while a read is out, for a commit after the
/// read's own, is read again once that read ends: the client is shown
/// the later commit, not left on the read's.
#[tokio::test(flavor = "multi_thread")]
async fn a_change_told_while_a_read_is_out_is_read_again_after_it() {
    let dir = Scratch::new("view-held-read");
    let mut registry = Registry::new();
    registry.register_actor_with::<HeldRealm>(
        Realm::KIND,
        ActorConfig::default(),
    );
    let host = Host::open(&dir, registry.into_factories()).unwrap();
    host.create(&Realm.address().unwrap(), Vec::new())
        .await
        .unwrap();
    let calls = Calls::new(host.clone());
    let (frames, mut sent) = mpsc::unbounded_channel();
    let (shows, jobs) = Shows::new();
    let keys =
        Keys(Arc::new(RwLock::new(Credentials::open(&dir).unwrap())));
    let hub =
        Hub::new(calls.clone(), frames.clone(), keys, shows.clone());
    let own = Arc::new(Own {
        chief: "Ada".into(),
        confinement: Confinement::Insecure,
        realm: "/realm".into(),
        epoch: 0,
        link: watch::Sender::new(crate::settings::untested()),
        faux: false,
    });
    let served = tokio::spawn(serve(
        jobs,
        shows.clone(),
        calls.clone(),
        own,
        hub,
        frames,
    ));
    shows.welcome(1).await;
    let mut state = None;
    until_shown(&mut sent, &mut state, "v0").await;

    let (began, beginning) = oneshot::channel();
    let (release, released) = oneshot::channel();
    *HOLD.lock().unwrap() = Some((began, released));
    calls.call(&Realm, &Commit).await.unwrap();
    beginning.await.unwrap();
    calls.call(&Realm, &Commit).await.unwrap();
    release.send(()).unwrap();

    tokio::time::timeout(
        Duration::from_secs(30),
        until_shown(&mut sent, &mut state, "v2"),
    )
    .await
    .expect("the second commit is shown");
    served.abort();
    host.shutdown().await.unwrap();
}
