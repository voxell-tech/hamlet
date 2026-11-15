//! The runner's side of the server. The realm's live state is the
//! world's resources; the runner sleeps until an input comes, writes
//! it as a message and updates the app once, then sleeps again.
//! Nothing runs on a frame.
//!
//! Every input is one update in one transaction: one whose system
//! fails or panics is rolled back and logged, and the server goes on.
//! Only a [`Fatal`] error, or a database that is no longer a
//! realm's, stops it.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use bevy_app::{App, First, Update};
use bevy_ecs::entity::Entity;
use bevy_ecs::error::{BevyError, ErrorContext};
use bevy_ecs::message::{Message, Messages};
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::common_conditions::on_message;
use bevy_ecs::schedule::{IntoScheduleConfigs, ScheduleConfigs};
use bevy_ecs::system::{
    ResMut, ScheduleSystem, SystemParam, SystemState,
};
use bevy_ecs::world::World;
use hamlet_protocol::client::RequestId;
use hamlet_sandbox::Sandbox;
use notify::RecommendedWatcher;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};

use crate::background::{BackgroundFinished, BackgroundResult};
use crate::calls::{Ran, ToolRun, ToolRunEnded};
use crate::clients::Clients;
use crate::clients::asked::{ClientConnected, ClientEvent};
use crate::clients::connection::Event;
use crate::clock;
use crate::core::{Confined, Core, Leftovers, Workspace};
use crate::credentials::Credentials;
use crate::diagnostic::{self, Secrets, tell};
use crate::follow_up::discard;
use crate::jobs::Job;
use crate::knowledge::runner::{
    JobsDone, RepositoryWriteLock, Runner,
};
use crate::providers::Providers;
use crate::realm::Realm;
use crate::society::agents;
use crate::talk::Env;
use crate::talk::heard::RuntimeLineReceived;
use crate::talk::process::{Pi, Received};
use crate::talk::runtimes::{self, Harness, Runtimes};
use crate::talk::sync::{FittedImages, ImagesFitted};
use crate::timers::{self, TimerFired, TimerSystem};
use crate::work::added::{self, AddedToolsChanged};
use crate::work::tools::shell::Shells;
use crate::workspace::Transport;

/// The error handler, where each input begins, and who is told when
/// one fails.
pub(crate) fn plugin(app: &mut App) {
    app.set_error_handler(record)
        .init_resource::<InputOrigin>()
        .add_systems(First, begin_input);
}

/// An input that wakes the server.
pub(crate) enum Input {
    /// A line from a runtime, or the end of its output.
    RuntimeLine(Received),
    /// A pass of the jobs runner that did jobs.
    JobsDone(Vec<Job>),
    /// An event of a client's connection.
    ConnectionEvent(Event),
    /// A client connected.
    ClientConnected(TcpStream),
    /// A tool run ended.
    ToolRunEnded(Ran),
    /// The images of an input were fitted to its model.
    ImagesFitted(FittedImages),
    /// A task finished, with what it awaited off the loop.
    BackgroundWorkFinished(Box<BackgroundResult>),
    /// The folder of the tools added by hand changed.
    AddedToolsChanged,
    /// A timer's deadline came.
    TimerDue,
    /// A timer that was due runs.
    TimerFired(TimerSystem),
}

impl Input {
    /// Writes the input as the message its area's systems read.
    fn write(self, world: &mut World) {
        match self {
            Input::RuntimeLine(received) => {
                world.write_message(RuntimeLineReceived(received));
            }
            Input::JobsDone(done) => {
                world.write_message(JobsDone(done));
            }
            Input::ConnectionEvent(event) => {
                world.write_message(ClientEvent(event));
            }
            Input::ClientConnected(stream) => {
                world.write_message(ClientConnected(stream));
            }
            Input::ToolRunEnded(ran) => {
                world.write_message(ToolRunEnded(ran));
            }
            Input::ImagesFitted(fitted) => {
                world.write_message(ImagesFitted(fitted));
            }
            Input::BackgroundWorkFinished(finished) => {
                world.write_message(BackgroundFinished(*finished));
            }
            Input::AddedToolsChanged => {
                world.write_message(AddedToolsChanged);
            }
            Input::TimerFired(fire) => {
                world.write_message(TimerFired(fire));
            }
            // The input channels give a deadline as the timers it
            // is.
            Input::TimerDue => {}
        }
    }
}

/// Who sent the input being run, so a failure can tell them.
#[derive(Resource, Default, Clone)]
pub(crate) enum InputOrigin {
    /// The server itself: a timer, a task, the jobs runner.
    #[default]
    Server,
    /// A client's request, answered with the failure.
    Client {
        client: u64,
        request: Option<RequestId>,
    },
    /// A runtime's message, answered with the failure when the
    /// runtime awaits an answer to `awaited`.
    Runtime {
        runtime: Entity,
        awaited: Option<String>,
    },
}

/// Begins the input's transaction, which `PostUpdate` keeps or rolls
/// back.
fn begin_input(
    mut realm: ResMut<Realm>,
    mut origin: ResMut<InputOrigin>,
) -> bevy_ecs::error::Result {
    *origin = InputOrigin::Server;
    hamlet_store::begin(realm.store().connection())?;
    Ok(())
}

/// The message of type `T` the input brought, which its system takes
/// once.
#[derive(SystemParam)]
pub(crate) struct InputMessage<'w, T: Message> {
    messages: ResMut<'w, Messages<T>>,
}

impl<T: Message> InputMessage<'_, T> {
    /// The message, taken; none when the input brought none.
    pub fn take(&mut self) -> Option<T> {
        self.messages.drain().next()
    }
}

/// Adds the systems that take in inputs.
pub(crate) trait AddHandler {
    /// Adds `handler` to `Update` for the input message `T`, run only
    /// when an input brought one.
    fn add_handler<T: Message, M>(
        &mut self,
        handler: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self;
}

impl AddHandler for App {
    fn add_handler<T: Message, M>(
        &mut self,
        handler: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self {
        let handler: ScheduleConfigs<ScheduleSystem> =
            handler.run_if(on_message::<T>);
        self.add_message::<T>().add_systems(Update, handler)
    }
}

/// The time the changes to the tools added by hand settle before they
/// load again.
const ADDED_TOOLS_SETTLE_TIME: Duration = Duration::from_millis(200);

/// The receiving end of a channel an area's plugin made for its
/// inputs, until the runner's input channels take it.
#[derive(Resource)]
pub(crate) struct InputReceiver<T: Send + 'static>(
    Mutex<mpsc::UnboundedReceiver<T>>,
);

/// A channel for inputs of `T`: its sending end, and the receiving
/// end for the runner's input channels, kept in the world until it
/// takes it.
pub(crate) fn input_channel<T: Send + 'static>(
    app: &mut App,
) -> mpsc::UnboundedSender<T> {
    let (to, from) = mpsc::unbounded_channel();
    app.insert_resource(InputReceiver(Mutex::new(from)));
    to
}

/// The receiving end of the inputs of `T`, taken from the world.
fn take_input_receiver<T: Send + 'static>(
    world: &mut World,
) -> Result<mpsc::UnboundedReceiver<T>> {
    let receiver = world
        .remove_resource::<InputReceiver<T>>()
        .context("a channel of inputs no plugin made")?;
    Ok(receiver
        .0
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner))
}

/// The channels inputs arrive on.
pub(crate) struct InputChannels {
    runtime_lines: mpsc::UnboundedReceiver<Received>,
    jobs_done: mpsc::UnboundedReceiver<Vec<Job>>,
    connection_events: mpsc::Receiver<Event>,
    ended_tool_runs: mpsc::UnboundedReceiver<Ran>,
    fitted_images: mpsc::UnboundedReceiver<FittedImages>,
    finished_background_work:
        mpsc::UnboundedReceiver<BackgroundResult>,
    added_tools_changes: mpsc::UnboundedReceiver<()>,
    client_listener: Option<TcpListener>,
}

impl InputChannels {
    /// The next input, or none after `wait`. The soonest timer is due
    /// at `due`.
    async fn next(
        &mut self,
        wait: Duration,
        due: Option<Instant>,
    ) -> Option<Input> {
        let due = async move {
            match due {
                Some(at) => tokio::time::sleep_until(at.into()).await,
                None => std::future::pending().await,
            }
        };
        let listener = self.client_listener.as_ref();
        tokio::select! {
            Some(received) = self.runtime_lines.recv() => {
                Some(Input::RuntimeLine(received))
            }
            Some(done) = self.jobs_done.recv() => {
                Some(Input::JobsDone(done))
            }
            Some(event) = self.connection_events.recv() => {
                Some(Input::ConnectionEvent(event))
            }
            Some(ran) = self.ended_tool_runs.recv() => {
                Some(Input::ToolRunEnded(ran))
            }
            Some(fitted) = self.fitted_images.recv() => {
                Some(Input::ImagesFitted(fitted))
            }
            Some(finished) = self.finished_background_work.recv() => {
                Some(Input::BackgroundWorkFinished(Box::new(finished)))
            }
            Some(()) = self.added_tools_changes.recv() => {
                // A save is many changes: once they settle, one load
                // takes them all.
                tokio::time::sleep(ADDED_TOOLS_SETTLE_TIME).await;
                while self.added_tools_changes.try_recv().is_ok() {}
                Some(Input::AddedToolsChanged)
            }
            Ok((stream, _)) = async {
                match listener {
                    Some(listener) => listener.accept().await,
                    None => std::future::pending().await,
                }
            } => Some(Input::ClientConnected(stream)),
            () = due => Some(Input::TimerDue),
            () = tokio::time::sleep(wait) => None,
        }
    }
}

/// An error after which the realm cannot be served safely.
#[derive(Debug, thiserror::Error)]
#[error("{0:#}")]
pub(crate) struct Fatal(pub anyhow::Error);

/// SQLite's own words for SQLITE_CORRUPT and SQLITE_NOTADB, which are
/// all diesel keeps of the code.
const BROKEN: [&str; 2] =
    ["database disk image is malformed", "file is not a database"];

/// The check that `error` means the realm cannot be served safely:
/// built as [`Fatal`], or a database SQLite calls corrupt or not a
/// database.
fn is_fatal(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.is::<Fatal>()
            || cause
                .downcast_ref::<diesel::result::Error>()
                .is_some_and(|error| match error {
                    diesel::result::Error::DatabaseError(_, info) => {
                        BROKEN.contains(&info.message())
                    }
                    _ => false,
                })
    })
}

/// A failure of an input: the words whoever sent it is told, every
/// cause for the log, and whether the realm can still be served.
#[derive(Clone, Debug)]
pub(crate) struct Failure {
    pub shown: String,
    pub logged: String,
    pub fatal: bool,
}

impl Failure {
    pub fn of(error: &anyhow::Error) -> Self {
        Self {
            shown: format!("{error}"),
            logged: format!("{error:#}"),
            fatal: is_fatal(error),
        }
    }

    /// The failure a system returned or panicked with. Its causes
    /// are kept only in its debug form, as `anyhow` writes them.
    fn of_system(mut error: BevyError) -> Self {
        if let Some(panic) = error.take_payload() {
            return Self::panicked(&*panic);
        }
        let whole = format!("{error:?}");
        Self {
            shown: error.to_string().trim_end().to_owned(),
            fatal: BROKEN.iter().any(|words| whole.contains(words)),
            logged: causes(&whole),
        }
    }

    fn panicked(panic: &(dyn std::any::Any + Send)) -> Self {
        let words = diagnostic::panicked(panic);
        Self {
            shown: words.clone(),
            logged: words,
            fatal: false,
        }
    }
}

/// The causes of an error's debug form on one line, as `{:#}` writes
/// them: `anyhow` writes each on a line of its own under "Caused
/// by:", numbered when there are several.
fn causes(debug: &str) -> String {
    debug
        .lines()
        .map(str::trim)
        .take_while(|line| *line != "Stack backtrace:")
        .filter(|line| !line.is_empty() && *line != "Caused by:")
        .map(|line| {
            line.split_once(": ")
                .filter(|(number, _)| number.parse::<usize>().is_ok())
                .map_or(line, |(_, cause)| cause)
        })
        .collect::<Vec<_>>()
        .join(": ")
}

thread_local! {
    /// The failures of the input being run, as the error handler
    /// records them. The server runs on one thread.
    static FAILURES: RefCell<Vec<Failure>> = const { RefCell::new(Vec::new()) };
}

/// The world's error handler: a system that fails or panics fails
/// only the input it ran for, which `PostUpdate` rolls back.
fn record(error: BevyError, _: ErrorContext) {
    fail(Failure::of_system(error));
}

/// Fails the input being run with `failure`.
pub(crate) fn fail(failure: Failure) {
    FAILURES.with_borrow_mut(|failures| failures.push(failure));
}

/// The input's first failure so far, if it failed.
pub(crate) fn first_failure() -> Option<Failure> {
    FAILURES.with_borrow(|failures| failures.first().cloned())
}

/// The failures recorded since the last call.
fn recorded() -> Vec<Failure> {
    FAILURES.with_borrow_mut(std::mem::take)
}

/// The served realm: the app whose world holds it, and the channels
/// its inputs arrive on.
pub(crate) struct Server {
    pub app: App,
    core: SystemState<Core<'static, 'static>>,
    inbox: InputChannels,
    /// The watch on the folder of the tools added by hand, which
    /// lasts as long as this; none when it could not begin.
    #[expect(dead_code, reason = "held, so the watch goes on")]
    watching: Option<RecommendedWatcher>,
}

impl Server {
    /// Takes `realm` on in `app`, which has [`crate::ServerPlugin`]:
    /// its jobs runner and its harness's runtime. Its first update
    /// runs `Startup`, which follows the turns that ended before the
    /// last stop but were not followed.
    pub fn start(
        mut app: App,
        mut realm: Realm,
        env: Env,
        pi: Pi,
        sandbox: Sandbox,
        closing: watch::Receiver<bool>,
    ) -> Result<Self> {
        let credentials = Credentials::open(realm.layout().root())?;
        let (lines_to, lines) = mpsc::unbounded_channel();
        let (worked_to, worked) = mpsc::unbounded_channel();
        let (events_to, events) = mpsc::channel(128);
        let writing = RepositoryWriteLock::default();
        let runner = Runner::start(
            realm.layout(),
            worked_to,
            writing.clone(),
        )?;
        let layout = realm.layout().clone();
        crate::knowledge::library::open(&layout, clock::now())?;
        // Agents read it, so it is there even with nothing in it.
        for folder in [layout.tools(), layout.images()] {
            std::fs::create_dir_all(&folder).with_context(|| {
                format!("making {}", folder.display())
            })?;
        }
        let (tools_to, tools) = mpsc::unbounded_channel();
        let watching = added::watch(&layout.tools(), tools_to)
            .inspect_err(|error| {
                tell(
                    Secrets::Fixed,
                    format!(
                        "the tools added by hand load only at \
                         start: {error}"
                    ),
                );
            })
            .ok();
        let providers = Providers::open(&layout, env.overrides.faux)?;
        let workspace = credentials
            .workspace()
            .cloned()
            .map(Transport::new)
            .transpose()?;
        runtimes::of_harness(
            realm.store().connection(),
            agents::HARNESS,
        )?;
        let clients = Clients::new(events_to, closing, clock::now())?;
        // What the areas start from, which only the start knows.
        app.insert_resource(realm)
            .insert_resource(env)
            .insert_resource(Harness::new(pi, lines_to))
            .insert_resource(credentials)
            .insert_resource(clients)
            .insert_resource(Confined(sandbox))
            .insert_resource(Shells::local())
            .insert_resource(providers)
            .insert_resource(Workspace(workspace))
            .insert_resource(runner)
            .insert_resource(writing);
        let world = app.world_mut();
        let inbox = InputChannels {
            runtime_lines: lines,
            jobs_done: worked,
            connection_events: events,
            ended_tool_runs: take_input_receiver(world)?,
            fitted_images: take_input_receiver(world)?,
            finished_background_work: take_input_receiver(world)?,
            added_tools_changes: tools,
            client_listener: None,
        };
        let mut server = Self {
            core: SystemState::new(app.world_mut()),
            app,
            inbox,
            watching,
        };
        let failures = server.update().map_err(|fatal| fatal.0)?;
        if let Some(failure) = failures.first() {
            anyhow::bail!("starting the realm: {}", failure.logged);
        }
        Ok(server)
    }

    /// The realm's live state, for one input.
    pub fn core(&mut self) -> Result<Core<'_, '_>> {
        Ok(self.core.get_mut(self.app.world_mut())?)
    }

    /// Takes clients' connections from `listener` too.
    pub fn listen(&mut self, listener: TcpListener) {
        self.inbox.client_listener = Some(listener);
    }

    /// Runs what woke the server next, or the idle work after `wait`.
    /// Returns whether anything came.
    pub async fn step(
        &mut self,
        wait: Duration,
    ) -> Result<bool, Fatal> {
        let due =
            timers::next(self.app.world_mut()).map_err(Fatal)?;
        match self.inbox.next(wait, due).await {
            Some(Input::TimerDue) => {
                let world = self.app.world_mut();
                let fires = timers::due(world, Instant::now())
                    .map_err(Fatal)?;
                // Each timer is an input of its own: one that fails
                // fails alone.
                for fire in fires {
                    self.run(Input::TimerFired(fire))?;
                }
                Ok(true)
            }
            Some(input) => {
                self.run(input)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Runs `input`: writes it as its message and updates the app
    /// once, in one transaction. One that fails or panics is rolled
    /// back whole, nothing it queued goes out, and it is logged; the
    /// server goes on unless the realm cannot be served. Nothing here
    /// awaits: what awaits runs on a task, whose result is an input.
    pub fn run(&mut self, input: Input) -> Result<(), Fatal> {
        input.write(self.app.world_mut());
        let failures = self.update()?;
        let core = self.core().map_err(Fatal)?;
        for failure in failures {
            tell(
                Secrets::Of(&core.credentials),
                format!("an input failed: {}", failure.logged),
            );
        }
        Ok(())
    }

    /// Updates the app once. Gives what failed in it.
    fn update(&mut self) -> Result<Vec<Failure>, Fatal> {
        let updated =
            catch_unwind(AssertUnwindSafe(|| self.app.update()));
        let mut failures = recorded();
        if let Err(panic) = updated {
            // A panic no system caught leaves the input open.
            failures.push(Failure::panicked(&*panic));
            self.unwind()
                .context("rolling back a failed input")
                .map_err(Fatal)?;
        }
        if let Some(fatal) = failures.iter().find(|f| f.fatal) {
            return Err(Fatal(anyhow::anyhow!(fatal.logged.clone())));
        }
        Ok(failures)
    }

    /// Rolls back the input that failed, and drops what it queued.
    fn unwind(&mut self) -> Result<()> {
        hamlet_store::unwind(
            self.core()?.realm.store().connection(),
        )?;
        discard(self.app.world_mut())
    }

    /// Readies the realm to stop: every sign-in ends, and a
    /// credential rotation begun may finish within `grace`, so no
    /// refreshed token is lost.
    pub async fn close(&mut self, grace: Duration) -> Result<()> {
        self.inbox.client_listener = None;
        {
            let mut core = self.core()?;
            core.providers.close_all();
            core.send_providers()?;
        }
        // No input follows to deliver them.
        let world = self.app.world_mut();
        self.core.apply(world);
        world
            .run_system_cached(|mut runtimes: Runtimes| {
                runtimes.deliver();
            })
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let deadline = Instant::now() + grace;
        while self.core()?.leases.is_holding()
            && Instant::now() < deadline
        {
            self.step(Duration::from_millis(100)).await?;
        }
        Ok(())
    }

    /// Stops the runtimes, giving each `grace` to finish.
    pub async fn stop(mut self, grace: Duration) -> Result<()> {
        let world = self.app.world_mut();
        // Each run's task stops with its entity.
        let runs = world
            .query_filtered::<Entity, With<ToolRun>>()
            .iter(world)
            .collect::<Vec<_>>();
        for run in runs {
            world.despawn(run);
        }
        let processes = self.core()?.runtimes.take_processes();
        for process in processes {
            process.stop(grace).await;
        }
        let world = self.app.world_mut();
        world.resource::<Leftovers>().end(|_| true);
        drop(world.remove_resource::<Runner>());
        Ok(())
    }
}

/// A test's wait on a condition of the realm.
#[cfg(all(test, feature = "harness"))]
pub(crate) async fn until(
    server: &mut Server,
    what: &str,
    mut done: impl FnMut(&mut Core<'_, '_>) -> Result<bool>,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(20);
    // What the test changed itself is kept and followed as an
    // input's changes are.
    server.core.apply(server.app.world_mut());
    let failures = server.update()?;
    if let Some(failure) = failures.first() {
        anyhow::bail!(
            "following the test's changes: {}",
            failure.logged
        );
    }
    while !done(&mut server.core()?)? {
        anyhow::ensure!(
            Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        server.step(Duration::from_millis(100)).await?;
    }
    Ok(())
}

#[cfg(all(test, feature = "harness"))]
mod tests {
    use hamlet_protocol::client as protocol;
    use hamlet_store::ids::Actor;
    use hamlet_testing::Scratch;

    use super::*;
    use crate::settings::{Overrides, Settings};
    use crate::society::hand;
    use crate::talk::{chats, inputs, items};

    /// A realm on `dir` served with the faux `script`, and a chief's
    /// line `text` to the Hand, answered.
    async fn told(
        dir: &Scratch,
        script: &str,
        text: &str,
    ) -> (Server, chats::Said) {
        let (mut server, said) = say(dir, script, text);
        ended(&mut server, &said).await;
        (server, said)
    }

    async fn ended(server: &mut Server, said: &chats::Said) {
        until(server, "the Hand's answer", |core| {
            let db = core.realm.store().connection();
            let input =
                inputs::find(db, said.conversation, &said.request)?;
            Ok(input.is_some_and(|i| i.is_ended() && i.followed))
        })
        .await
        .unwrap();
    }

    /// A realm on `dir` served with the faux `script`, and a chief's
    /// line `text` to the Hand, kept.
    fn say(
        dir: &Scratch,
        script: &str,
        text: &str,
    ) -> (Server, chats::Said) {
        let faux = dir.join("faux.json");
        std::fs::write(&faux, script).unwrap();
        let realm =
            Realm::open(&dir.join("realm"), "chief", clock::now())
                .unwrap();
        let chief_id = realm.chief();
        let env = Env {
            settings: Settings::default(),
            overrides: Overrides {
                faux: true,
                ..Overrides::default()
            },
        };
        let sandbox =
            Sandbox::open(hamlet_sandbox::Isolation::Sandboxed)
                .unwrap();
        let (_closing, closed) = watch::channel(false);
        let mut app = App::new();
        app.add_plugins(crate::ServerPlugin {
            options: crate::options::Options {
                home: dir.join("realm"),
                port: 0,
                faux: Some(faux.clone()),
                isolation: hamlet_sandbox::Isolation::Sandboxed,
            },
        });
        let mut server = Server::start(
            app,
            realm,
            env,
            Pi::from_env(Some(faux)).unwrap(),
            sandbox,
            closed,
        )
        .unwrap();
        let mut core = server.core().unwrap();
        let hand = hand::id(core.realm.store().connection()).unwrap();
        let said = core
            .change(Actor::Chief(chief_id), |cx, env| {
                chats::say(cx, env, hand, chief_id, "r1", text)
            })
            .unwrap();
        (server, said)
    }

    #[tokio::test]
    async fn a_scholars_dream_writes_its_main_through_its_tools() {
        let dir = Scratch::new("server-core-dream");
        let script = r#"{
            "responses": ["Hello."],
            "kinds": {"dream": [
                {"calls": [{"name": "write", "arguments": {"path": "memory/shared.md", "content": "---\ndescription: What every hire knows.\n---\nBe brief."}}]},
                "Kept one shared note."
            ]}
        }"#;
        let (mut server, _) = told(&dir, script, "Hi").await;
        let mut core = server.core().unwrap();
        let chief = core.realm.chief();
        let ada = core
            .attempt(Actor::Chief(chief), |cx, _| {
                crate::society::city::make(
                    cx, "ada", chief, "Careful.", None,
                )
            })
            .unwrap()
            .unwrap();
        until(&mut server, "the scholar planted", |core| {
            let db = core.realm.store().connection();
            Ok(crate::society::city::get(db, ada)?.planted)
        })
        .await
        .unwrap();
        let address = format!("scholar#{}", ada.0);
        let began = server
            .core()
            .unwrap()
            .dream_command(None, &[&address])
            .unwrap();
        assert!(began.is_ok(), "{began:?}");
        until(&mut server, "the dream's end", |core| {
            let db = core.realm.store().connection();
            Ok(crate::society::dreams::last_of_scholar(db, ada)?
                .is_some_and(|dream| dream.ended.is_some()))
        })
        .await
        .unwrap();
        let repo = hamlet_git::Repository::open(
            &server.core().unwrap().realm.layout().repository(ada),
        )
        .unwrap();
        let main = repo.tip(hamlet_git::MAIN).unwrap().unwrap();
        assert!(
            repo.read(main, "memory/shared.md").unwrap().is_some(),
            "the dream's note is on main"
        );
        server.stop(Duration::from_secs(5)).await.unwrap();
    }

    #[tokio::test]
    async fn a_networked_command_waits_for_the_chief_and_runs_once_allowed()
     {
        let dir = Scratch::new("server-core-gate");
        let script = r#"{"responses": [
            {"calls": [{"name": "bash", "arguments": {"command": "echo curl reached"}}]},
            "Done."
        ]}"#;
        let (mut server, said) = say(&dir, script, "Fetch it");
        until(&mut server, "the approval", |core| {
            let db = core.realm.store().connection();
            Ok(!crate::oversight::open(db)?.is_empty())
        })
        .await
        .unwrap();
        let mut core = server.core().unwrap();
        let db = core.realm.store().connection();
        let wait = crate::oversight::open(db).unwrap().remove(0);
        assert_eq!(
            wait.danger,
            Some(crate::oversight::Danger::Network)
        );
        let answered = core
            .answer_wait(
                &wait.id.to_string(),
                &protocol::Choice::Allow,
                None,
                None,
            )
            .unwrap();
        assert!(answered.is_ok(), "{answered:?}");
        ended(&mut server, &said).await;
        let mut core = server.core().unwrap();
        let db = core.realm.store().connection();
        let page =
            items::page(db, said.conversation, None, 20).unwrap();
        assert!(
            page.iter().any(|(_, item)| {
                item.tool.as_deref() == Some("bash")
                    && item.text.contains("curl reached")
            }),
            "{page:#?}"
        );
        assert!(crate::oversight::open(db).unwrap().is_empty());
        server.stop(Duration::from_secs(5)).await.unwrap();
    }

    #[tokio::test]
    async fn the_hand_writes_its_lodge_and_reads_it_back_with_a_command()
     {
        let dir = Scratch::new("server-core-tools");
        let script = r#"{"responses": [
            {"calls": [{"name": "write", "arguments": {"path": "notes/seen.md", "content": "seen it"}}]},
            {"calls": [{"name": "bash", "arguments": {"command": "cat notes/seen.md"}}]},
            "Done."
        ]}"#;
        let (mut server, said) = told(&dir, script, "Note it").await;
        let mut core = server.core().unwrap();
        let lodge = core.realm.layout().hand();
        assert_eq!(
            std::fs::read_to_string(lodge.join("notes/seen.md"))
                .unwrap(),
            "seen it"
        );
        let db = core.realm.store().connection();
        let page =
            items::page(db, said.conversation, None, 20).unwrap();
        assert!(
            page.iter().any(|(_, item)| {
                item.tool.as_deref() == Some("bash")
                    && item.text.contains("seen it")
            }),
            "{page:#?}"
        );
        server.stop(Duration::from_secs(5)).await.unwrap();
    }

    #[tokio::test]
    async fn the_hand_answers_a_chief_through_pi() {
        let dir = Scratch::new("server-core");
        let (mut server, said) =
            told(&dir, r#"{"responses": ["Hello, chief."]}"#, "Hi")
                .await;
        let mut core = server.core().unwrap();
        let db = core.realm.store().connection();
        let page =
            items::page(db, said.conversation, None, 20).unwrap();
        assert!(
            page.iter()
                .any(|(_, item)| item.text.contains("Hello, chief.")),
            "{page:#?}"
        );
        let task = crate::talk::tasks::get(db, said.task).unwrap();
        assert!(
            task.done_at.is_some(),
            "the instruction is answered"
        );
        server.stop(Duration::from_secs(5)).await.unwrap();
    }
}
