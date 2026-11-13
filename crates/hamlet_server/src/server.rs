//! The server as a Bevy app. The realm's live state is the world's
//! resources; a runner of its own sleeps until an input comes, runs
//! it, and sleeps again. Nothing runs on a frame.
//!
//! Every input goes through [`Server::run`]: one that fails or
//! panics is rolled back and logged, and the server goes on. Only a
//! [`Fatal`] error, or a database that is no longer a realm's, stops
//! it.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use bevy_app::App;
use bevy_ecs::entity::Entity;
use bevy_ecs::error::{BevyError, ErrorContext};
use bevy_ecs::query::With;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{In, IntoSystem, ResMut, SystemState};
use bevy_ecs::world::World;
use hamlet_protocol::runtime::{
    ErrorCode, ProtocolError, RuntimeMessage, ServerMessage,
};
use hamlet_sandbox::Sandbox;
use hamlet_store::ids::Actor;
use notify::RecommendedWatcher;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};

use crate::apply::{Apply, Changed, discard};
use crate::calls::{Ran, Running};
use crate::clients::check::{failed, fault};
use crate::clients::connection::Event;
use crate::clients::{Clients, asked};
use crate::clock;
use crate::core::{Confined, Core, Leftovers, Remote, Workspace};
use crate::credentials::Credentials;
use crate::deadline::{self, Fire};
use crate::diagnostic::{self, Secrets, tell};
use crate::jobs::Job;
use crate::knowledge::runner::{Runner, Writing};
use crate::later::Finished;
use crate::providers::Providers;
use crate::realm::Realm;
use crate::society::agents;
use crate::talk::follow::follow;
use crate::talk::process::{Pi, Received};
use crate::talk::runtimes::{self, Harness, Runtimes};
use crate::talk::sync::Fitted;
use crate::talk::{Env, heard, inputs};
use crate::work::added;
use crate::work::tools::shell::Shells;
use crate::workspace::Transport;

/// An input that wakes the server.
pub(crate) enum Input {
    /// A line from a runtime, or the end of its output.
    Line(Received),
    /// A pass of the jobs runner that did jobs.
    Worked(Vec<Job>),
    /// An event of a client's connection.
    Connection(Event),
    /// A client connected.
    Accepted(TcpStream),
    /// A tool run ended.
    Ran(Ran),
    /// The images of an input were fitted to its model.
    Fitted(Fitted),
    /// A task finished, with what it awaited off the loop.
    Finished(Box<Finished>),
    /// The folder of the tools added by hand changed.
    Tools,
    /// A timer's deadline came.
    Due,
    /// A timer that was due runs.
    Timer(Fire),
}

/// The time the changes to the tools added by hand settle before they
/// load again.
const SETTLE: Duration = Duration::from_millis(200);

/// The receiving end of a channel an area's plugin made for its
/// inputs, until the runner's inbox takes it.
#[derive(Resource)]
pub(crate) struct Arriving<T: Send + 'static>(
    Mutex<mpsc::UnboundedReceiver<T>>,
);

/// A channel for inputs of `T`: its sending end, and the receiving
/// end for the runner's inbox, kept in the world until it takes it.
pub(crate) fn arriving<T: Send + 'static>(
    app: &mut App,
) -> mpsc::UnboundedSender<T> {
    let (to, from) = mpsc::unbounded_channel();
    app.insert_resource(Arriving(Mutex::new(from)));
    to
}

/// The receiving end of the inputs of `T`, taken from the world.
fn take<T: Send + 'static>(
    world: &mut World,
) -> Result<mpsc::UnboundedReceiver<T>> {
    let arriving = world
        .remove_resource::<Arriving<T>>()
        .context("a channel of inputs no plugin made")?;
    Ok(arriving
        .0
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner))
}

/// The channels inputs arrive on.
pub(crate) struct Inbox {
    lines: mpsc::UnboundedReceiver<Received>,
    worked: mpsc::UnboundedReceiver<Vec<Job>>,
    events: mpsc::Receiver<Event>,
    ran: mpsc::UnboundedReceiver<Ran>,
    fitted: mpsc::UnboundedReceiver<Fitted>,
    finished: mpsc::UnboundedReceiver<Finished>,
    tools: mpsc::UnboundedReceiver<()>,
    listener: Option<TcpListener>,
}

impl Inbox {
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
        let listener = self.listener.as_ref();
        tokio::select! {
            Some(received) = self.lines.recv() => {
                Some(Input::Line(received))
            }
            Some(done) = self.worked.recv() => Some(Input::Worked(done)),
            Some(event) = self.events.recv() => {
                Some(Input::Connection(event))
            }
            Some(ran) = self.ran.recv() => Some(Input::Ran(ran)),
            Some(fitted) = self.fitted.recv() => {
                Some(Input::Fitted(fitted))
            }
            Some(finished) = self.finished.recv() => {
                Some(Input::Finished(Box::new(finished)))
            }
            Some(()) = self.tools.recv() => {
                // A save is many changes: once they settle, one load
                // takes them all.
                tokio::time::sleep(SETTLE).await;
                while self.tools.try_recv().is_ok() {}
                Some(Input::Tools)
            }
            Ok((stream, _)) = async {
                match listener {
                    Some(listener) => listener.accept().await,
                    None => std::future::pending().await,
                }
            } => Some(Input::Accepted(stream)),
            () = due => Some(Input::Due),
            () = tokio::time::sleep(wait) => None,
        }
    }
}

/// An error after which the realm cannot be served safely.
#[derive(Debug, thiserror::Error)]
#[error("{0:#}")]
pub(crate) struct Fatal(pub anyhow::Error);

/// The check that `error` means the realm cannot be served safely:
/// built as [`Fatal`], or a database SQLite calls corrupt or not a
/// database.
fn is_fatal(error: &anyhow::Error) -> bool {
    // SQLite's own words for SQLITE_CORRUPT and SQLITE_NOTADB, which
    // are all diesel keeps of the code.
    const BROKEN: [&str; 2] = [
        "database disk image is malformed",
        "file is not a database",
    ];
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

thread_local! {
    /// The failures of systems since the runner last looked, as the
    /// error handler records them. The server runs on one thread.
    static FAILURES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The world's error handler: a system that fails or panics fails
/// only the input it ran for, which the runner rolls back.
fn record(error: BevyError, context: ErrorContext) {
    FAILURES.with_borrow_mut(|failures| {
        failures.push(format!("{context}: {error}"));
    });
}

/// The failures recorded since the last call.
fn recorded() -> Vec<String> {
    FAILURES.with_borrow_mut(std::mem::take)
}

/// The served realm: the app whose world holds it, and the channels
/// its inputs arrive on.
pub(crate) struct Server {
    pub app: App,
    core: SystemState<Core<'static, 'static>>,
    inbox: Inbox,
    /// The watch on the folder of the tools added by hand, which
    /// lasts as long as this; none when it could not begin.
    #[expect(dead_code, reason = "held, so the watch goes on")]
    watching: Option<RecommendedWatcher>,
}

impl Server {
    /// Takes `realm` on in `app`: its jobs runner and its harness's
    /// runtime, and follows the turns that ended before the last stop
    /// but were not followed.
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
        let writing = Writing::default();
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
        app.set_error_handler(record)
            .insert_resource(realm)
            .insert_resource(env)
            .insert_resource(Harness::new(pi, lines_to))
            .insert_resource(credentials)
            .insert_resource(clients)
            .insert_resource(Confined(sandbox))
            .insert_resource(Shells::local())
            .insert_resource(providers)
            .insert_resource(Workspace(workspace))
            .insert_resource(runner)
            .insert_resource(writing)
            .add_plugins((
                crate::core::plugin,
                crate::apply::plugin,
                crate::later::plugin,
                crate::calls::plugin,
                crate::talk::sync::plugin,
                crate::clients::plugin,
                crate::commands::plugin,
                runtimes::plugin,
                crate::providers::plugin,
                crate::dreaming::plugin,
                crate::images::plugin,
                crate::files::plugin,
            ));
        let world = app.world_mut();
        let inbox = Inbox {
            lines,
            worked,
            events,
            ran: take(world)?,
            fitted: take(world)?,
            finished: take(world)?,
            tools,
            listener: None,
        };
        let mut server = Self {
            core: SystemState::new(app.world_mut()),
            app,
            inbox,
            watching,
        };
        let world = server.app.world_mut();
        for start in [
            world.register_system_cached(runtimes::start),
            world.register_system_cached(added::reload),
        ] {
            world
                .run_system(start)
                .map_err(|error| anyhow::anyhow!("{error}"))??;
        }
        let mut core = server.core()?;
        core.retool()?;
        core.start_providers()?;
        let unfollowed =
            inputs::unfollowed(core.realm.store().connection())?;
        for input in unfollowed {
            // No runtime waits on anything from before the stop.
            core.change(Actor::Realm, |cx, env| {
                follow(cx, env, input.conversation, &input.request)
            })?;
        }
        if let Some(failure) = server.apply().into_iter().next() {
            anyhow::bail!(
                "following the last stop's turns: {failure}"
            );
        }
        Ok(server)
    }

    /// The realm's live state, for one input.
    pub fn core(&mut self) -> Result<Core<'_, '_>> {
        Ok(self.core.get_mut(self.app.world_mut())?)
    }

    /// Takes clients' connections from `listener` too.
    pub fn listen(&mut self, listener: TcpListener) {
        self.inbox.listener = Some(listener);
    }

    /// Runs what woke the server next, or the idle work after `wait`.
    /// Returns whether anything came.
    pub async fn step(
        &mut self,
        wait: Duration,
    ) -> Result<bool, Fatal> {
        let due =
            deadline::next(self.app.world_mut()).map_err(Fatal)?;
        match self.inbox.next(wait, due).await {
            Some(Input::Due) => {
                let world = self.app.world_mut();
                let fires = deadline::due(world, Instant::now())
                    .map_err(Fatal)?;
                // Each timer is an input of its own: one that fails
                // fails alone.
                for fire in fires {
                    self.run(Input::Timer(fire))?;
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

    /// Runs `input` in one transaction, keeps it, then runs what
    /// follows it. One that fails or panics is rolled back whole,
    /// nothing it queued goes out, and it is logged; the server goes
    /// on unless the realm cannot be served. Nothing here awaits:
    /// what awaits runs on a task, whose result is an input.
    pub fn run(&mut self, input: Input) -> Result<(), Fatal> {
        let ran = catch_unwind(AssertUnwindSafe(|| {
            hamlet_store::begin(
                self.core()?.realm.store().connection(),
            )?;
            self.handle(input)?;
            // What the input spawned and despawned takes effect in
            // its transaction, with what that sets off.
            self.core.apply(self.app.world_mut());
            hamlet_store::commit(
                self.core()?.realm.store().connection(),
            )
        }));
        let failure = match ran {
            Ok(Ok(())) => None,
            Ok(Err(error)) if is_fatal(&error) => {
                return Err(Fatal(error));
            }
            Ok(Err(error)) => Some(format!("{error:#}")),
            Err(panic) => Some(diagnostic::panicked(&*panic)),
        };
        if failure.is_some() {
            self.core.apply(self.app.world_mut());
            // A connection that cannot roll back what a panic left
            // open cannot be trusted with the next change.
            self.unwind()
                .context("rolling back a failed input")
                .map_err(Fatal)?;
        } else if let Some(point) =
            self.app.world().resource::<Changed>().fault
        {
            crate::fault::point(point);
        }
        let mut failures = recorded();
        failures.extend(failure);
        failures.extend(self.apply());
        let core = self.core().map_err(Fatal)?;
        for failure in failures {
            tell(
                Secrets::Of(&core.credentials),
                format!("an input failed: {failure}"),
            );
        }
        Ok(())
    }

    /// Rolls back the input that failed, and drops what it queued.
    fn unwind(&mut self) -> Result<()> {
        hamlet_store::unwind(
            self.core()?.realm.store().connection(),
        )?;
        discard(self.app.world_mut())
    }

    /// Runs [`Apply`]: what follows the input's changes. Gives the
    /// failures of its steps.
    fn apply(&mut self) -> Vec<String> {
        let world = self.app.world_mut();
        let applied = catch_unwind(AssertUnwindSafe(|| {
            world.run_schedule(Apply);
        }));
        let mut failures = recorded();
        if let Err(panic) = applied {
            failures.push(diagnostic::panicked(&*panic));
        }
        failures
    }

    fn handle(&mut self, input: Input) -> Result<()> {
        let world = self.app.world_mut();
        match input {
            Input::Line(received) => self.line(received),
            Input::Connection(event) => self.connection(event),
            Input::Accepted(stream) => {
                // A connection is an entity until it goes.
                let connection = self.core()?.clients.accept(stream);
                self.app.world_mut().spawn(connection);
                Ok(())
            }
            Input::Timer(fire) => world
                .run_system(fire)
                .map_err(|error| anyhow::anyhow!("{error}"))?,
            Input::Worked(done) => once(world, worked, done),
            Input::Ran(ran) => once(world, ran_one, ran),
            Input::Fitted(fitted) => once(world, fitted_one, fitted),
            Input::Finished(finished) => self.finished(*finished),
            Input::Tools => world
                .run_system_cached(added::reload)
                .map_err(|error| anyhow::anyhow!("{error}"))?,
            // The inbox gives a deadline as the timers it is.
            Input::Due => Ok(()),
        }
    }

    /// Takes in what a task finished with.
    fn finished(&mut self, finished: Finished) -> Result<()> {
        let mut core = self.core()?;
        match finished {
            Finished::Reply { client, message } => {
                core.answer(client, message)
            }
            Finished::Moving { progress, answer } => {
                core.remote.moving.0 = Some(progress);
                core.changed.republish = true;
                if let Some((client, message)) = answer {
                    core.answer(client, message)?;
                }
                Ok(())
            }
            Finished::Looked { call, found, again } => {
                if let Some(receiver) = core.looks.found(call, found)
                {
                    core.remote.unpushed.insert(receiver);
                }
                // A runtime that started again since asks again.
                if core.runtimes.generation(again.from)
                    != Some(again.generation)
                {
                    return Ok(());
                }
                heard::hear(
                    self.app.world_mut(),
                    again.from,
                    again.message,
                )
            }
        }
    }

    /// Takes in what a connection did. A request is run by the system
    /// for it; one whose handling fails leaves its transaction rolled
    /// back, and the client is told.
    fn connection(&mut self, event: Event) -> Result<()> {
        let (client, text) = match event {
            Event::Late(client) => {
                self.core()?.late(client);
                return Ok(());
            }
            Event::Gone(client) => {
                // Its hooks and observers take what the client held,
                // and its uploads go with it.
                let gone = self
                    .core()?
                    .clients
                    .entities
                    .get(&client)
                    .copied();
                if let Some(gone) = gone {
                    self.app.world_mut().despawn(gone);
                }
                return Ok(());
            }
            Event::Message(client, text) => (client, text),
        };
        let Some(message) = self.core()?.checked(client, &text)?
        else {
            return Ok(());
        };
        let id = message.id().cloned();
        let world = self.app.world_mut();
        let asked = catch_unwind(AssertUnwindSafe(|| {
            asked::ask(world, client, message)
        }));
        let (shown, logged) = match asked {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(error)) => {
                (format!("{error}"), format!("{error:#}"))
            }
            Err(panic) => {
                let words = diagnostic::panicked(&*panic);
                (words.clone(), words)
            }
        };
        self.unwind()?;
        let mut core = self.core()?;
        tell(
            Secrets::Of(&core.credentials),
            format!("a request failed: {logged}"),
        );
        core.answer(client, fault(id.as_ref(), failed(shown)))
    }

    /// Takes in a runtime's line: its message is run by the system
    /// for it. A message whose handling fails leaves its transaction
    /// rolled back, and a runtime that waits is told; it goes on.
    fn line(&mut self, received: Received) -> Result<()> {
        let mut core = self.core()?;
        if !core.runtimes.is_current(&received) {
            return Ok(());
        }
        let from = received.runtime;
        let Some(line) = received.line else {
            core.runtimes.failed(from);
            core.leases.gone(from);
            return Ok(());
        };
        let message =
            match serde_json::from_str::<RuntimeMessage>(&line) {
                Ok(message) => message,
                Err(error) => {
                    tell(
                        Secrets::Of(&core.credentials),
                        format!(
                            "a runtime's line is not a message: \
                             {error}"
                        ),
                    );
                    return Ok(());
                }
            };
        let reply_address = heard::awaited(&message);
        let world = self.app.world_mut();
        let heard = catch_unwind(AssertUnwindSafe(|| {
            heard::hear(world, from, message)
        }));
        let failure = match heard {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(format!("{error:#}")),
            Err(panic) => Some(diagnostic::panicked(&*panic)),
        };
        if failure.is_some() {
            self.unwind()?;
        }
        let mut core = self.core()?;
        if let Some(failure) = failure {
            tell(
                Secrets::Of(&core.credentials),
                format!("a runtime's line failed: {failure}"),
            );
            if let Some(reply_address) = reply_address {
                let message =
                    core.credentials.scrub_text(&failed(failure));
                core.runtimes.send(from, |v, id| {
                    ServerMessage::Error(ProtocolError {
                        code: ErrorCode::Failed,
                        id,
                        message,
                        re: Some(reply_address),
                        v,
                    })
                });
            }
        }
        core.answer_credentials();
        Ok(())
    }

    /// Readies the realm to stop: every sign-in ends, and a
    /// credential rotation begun may finish within `grace`, so no
    /// refreshed token is lost.
    pub async fn close(&mut self, grace: Duration) -> Result<()> {
        self.inbox.listener = None;
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
            .query_filtered::<Entity, With<Running>>()
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

/// Runs `system` once with `input`, as a cached one-shot system.
fn once<I, M>(
    world: &mut World,
    system: impl IntoSystem<In<I>, Result<()>, M> + 'static,
    input: I,
) -> Result<()>
where
    I: Send + 'static,
{
    world
        .run_system_cached_with(system, input)
        .map_err(|error| anyhow::anyhow!("{error}"))?
}

/// A pass of the jobs runner did jobs: the lodges they changed are
/// owed a push, and the clients see what they did.
fn worked(
    In(done): In<Vec<Job>>,
    mut realm: ResMut<Realm>,
    mut remote: Remote,
    mut changed: ResMut<Changed>,
) -> Result<()> {
    remote.owe_pushes(realm.store().connection(), &done);
    changed.republish = true;
    Ok(())
}

/// A tool run ended.
fn ran_one(In(ran): In<Ran>, mut core: Core) -> Result<()> {
    core.ran(ran)
}

/// The images of an input were fitted: it may go to its runtime.
fn fitted_one(In(fitted): In<Fitted>, mut core: Core) -> Result<()> {
    core.fitting.fitted(fitted);
    core.changed.answered = true;
    Ok(())
}

/// A test's wait on a condition of the realm.
#[cfg(all(test, feature = "harness"))]
pub(crate) async fn until(
    server: &mut Server,
    what: &str,
    mut done: impl FnMut(&mut Core<'_, '_>) -> Result<bool>,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(20);
    // What the test changed itself is kept as an input's end keeps
    // it, and followed.
    server.core.apply(server.app.world_mut());
    hamlet_store::commit(server.core()?.realm.store().connection())?;
    if let Some(failure) = server.apply().into_iter().next() {
        anyhow::bail!("following the test's changes: {failure}");
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
    use hamlet_testing::Scratch;

    use super::*;
    use crate::settings::{Overrides, Settings};
    use crate::society::hand;
    use crate::talk::{chats, items};

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
        let mut server = Server::start(
            App::empty(),
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
