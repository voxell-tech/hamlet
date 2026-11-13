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
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use bevy_app::App;
use bevy_ecs::error::{BevyError, ErrorContext};
use bevy_ecs::system::SystemState;
use futures::FutureExt;
use hamlet_protocol::client::ClientMessage;
use hamlet_protocol::runtime::{
    ErrorCode, ProtocolError, RuntimeMessage, ServerMessage,
};
use hamlet_sandbox::{AllLeftovers, Sandbox};
use hamlet_store::ids::Actor;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};

use crate::apply::Apply;
use crate::calls::{self, Calls, Ran};
use crate::clients::check::fault;
use crate::clients::connection::Event;
use crate::clients::{Clients, asked};
use crate::clock;
use crate::core::{
    Added, Catalogue, Confined, Core, Leftovers, Lives, Lodged,
    Moving, ToolFaults, Unpushed, Workspace,
};
use crate::credentials::Credentials;
use crate::diagnostic::{self, Secrets, tell};
use crate::files::FileUploads;
use crate::images::Uploads;
use crate::jobs::Job;
use crate::knowledge::runner::{Runner, Writing};
use crate::providers::Providers;
use crate::realm::Realm;
use crate::society::agents;
use crate::talk::follow::follow;
use crate::talk::leases::Authority;
use crate::talk::process::{Pi, Received};
use crate::talk::runtimes::{self, Runtimes};
use crate::talk::sync::{Fitted, Fitting};
use crate::talk::{Env, heard, inputs};
use crate::work::added;
use crate::work::tools::{self, Shells};
use crate::workspace::Transport;

/// What wakes the server.
pub(crate) enum Input {
    /// A line from a runtime, or the end of its output.
    Line(Received),
    /// A pass of the jobs runner that did jobs.
    Worked(Vec<Job>),
    /// What a client's connection did.
    Connection(Event),
    /// A client connected.
    Accepted(TcpStream),
    /// A tool run ended.
    Ran(Ran),
    /// The images of an input were fitted to its model.
    Fitted(Fitted),
    /// A failed runtime is due to start again.
    Restart,
    /// Requests to the providers ran out of time.
    Expiry,
    /// The hour of the dreams came round.
    Dream,
    /// The images no one holds may go.
    SweepImages,
    /// The files no one holds may go.
    SweepFiles,
}

/// The channels inputs arrive on.
pub(crate) struct Inbox {
    lines: mpsc::UnboundedReceiver<Received>,
    worked: mpsc::UnboundedReceiver<Vec<Job>>,
    events: mpsc::Receiver<Event>,
    ran: mpsc::UnboundedReceiver<Ran>,
    fitted: mpsc::UnboundedReceiver<Fitted>,
    listener: Option<TcpListener>,
}

impl Inbox {
    /// The next input, or none after `wait`. A failed runtime is due
    /// at `restart`, and provider requests run out at `expiry`.
    async fn next(
        &mut self,
        wait: Duration,
        restart: Option<Instant>,
        expiry: Option<Instant>,
    ) -> Option<Input> {
        let at = |instant: Option<Instant>| async move {
            match instant {
                Some(at) => {
                    tokio::time::sleep_until(at.into()).await;
                }
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
            Ok((stream, _)) = async {
                match listener {
                    Some(listener) => listener.accept().await,
                    None => std::future::pending().await,
                }
            } => Some(Input::Accepted(stream)),
            () = at(restart) => Some(Input::Restart),
            () = at(expiry) => Some(Input::Expiry),
            () = tokio::time::sleep(wait) => None,
        }
    }
}

/// An error after which the realm cannot be served safely.
#[derive(Debug, thiserror::Error)]
#[error("{0:#}")]
pub(crate) struct Fatal(pub anyhow::Error);

/// Whether `error` means the realm cannot be served safely: built as
/// [`Fatal`], or a database SQLite calls corrupt or not a database.
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
    core: SystemState<Core<'static>>,
    inbox: Inbox,
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
        let (ran_to, ran) = mpsc::unbounded_channel();
        let (fitted_to, fitted) = mpsc::unbounded_channel();
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
        let shells = Shells::local();
        let (added, faults) = added::load(layout.root());
        for fault in &faults {
            tell(
                Secrets::Fixed,
                format!("a tool added by hand did not load: {fault}"),
            );
        }
        let mut catalogue = tools::specs(&shells);
        catalogue.extend(calls::specs());
        catalogue.extend(
            added.iter().map(|manifest| manifest.spec.clone()),
        );
        let providers = Providers::open(&layout, env.overrides.faux)?;
        let workspace = credentials
            .workspace()
            .cloned()
            .map(Transport::new)
            .transpose()?;
        let db = realm.store().connection();
        runtimes::of_harness(db, agents::HARNESS)?;
        let mut runtimes = Runtimes::new(pi, lines_to);
        runtimes.start(db, &layout)?;
        let clients = Clients::new(events_to, closing, clock::now())?;
        app.set_error_handler(record)
            .add_plugins((
                crate::apply::plugin,
                crate::commands::plugin,
            ))
            .insert_resource(realm)
            .insert_resource(env)
            .insert_resource(runtimes)
            .insert_resource(credentials)
            .insert_resource(clients)
            .insert_resource(Lives(Default::default()))
            .insert_resource(Confined(sandbox))
            .insert_resource(shells)
            .insert_resource(Leftovers(AllLeftovers::default()))
            .insert_resource(Added(added))
            .insert_resource(ToolFaults(faults))
            .insert_resource(Catalogue(catalogue))
            .insert_resource(Calls::new(ran_to))
            .insert_resource(Fitting::new(fitted_to))
            .insert_resource(providers)
            .insert_resource(Authority::default())
            .insert_resource(Uploads::default())
            .insert_resource(FileUploads::default())
            .insert_resource(Workspace(workspace))
            .insert_resource(Lodged(Default::default()))
            .insert_resource(Unpushed(Default::default()))
            .insert_resource(Moving(None))
            .insert_resource(runner)
            .insert_resource(writing);
        let mut server = Self {
            core: SystemState::new(app.world_mut()),
            app,
            inbox: Inbox {
                lines,
                worked,
                events,
                ran,
                fitted,
                listener: None,
            },
        };
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
    pub fn core(&mut self) -> Result<Core<'_>> {
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
        let (restart, expiry) = {
            let core = self.core().map_err(Fatal)?;
            (core.runtimes.next_start(), core.providers.next_expiry())
        };
        match self.inbox.next(wait, restart, expiry).await {
            Some(input) => {
                self.run(input).await?;
                Ok(true)
            }
            None => {
                for input in [
                    Input::Dream,
                    Input::SweepImages,
                    Input::SweepFiles,
                ] {
                    self.run(input).await?;
                }
                Ok(false)
            }
        }
    }

    /// Runs `input`. One that fails or panics leaves its transaction
    /// rolled back and is logged; the server goes on unless the
    /// realm cannot be served.
    pub async fn run(&mut self, input: Input) -> Result<(), Fatal> {
        let ran =
            AssertUnwindSafe(self.handle(input)).catch_unwind().await;
        let mut failures = recorded();
        match ran {
            Ok(Ok(())) => {}
            Ok(Err(error)) if is_fatal(&error) => {
                return Err(Fatal(error));
            }
            Ok(Err(error)) => failures.push(format!("{error:#}")),
            Err(panic) => {
                failures.push(diagnostic::panicked(&*panic));
            }
        }
        if !failures.is_empty() {
            // A connection that cannot roll back what a panic left
            // open cannot be trusted with the next change.
            let mut core = self.core().map_err(Fatal)?;
            hamlet_store::unwind(core.realm.store().connection())
                .context("rolling back a failed input")
                .map_err(Fatal)?;
        }
        // What the input kept is followed up whether or not it went
        // on to fail.
        failures.extend(self.apply());
        let mut core = self.core().map_err(Fatal)?;
        // What the input's changes owe the remote workspace is pushed
        // before the next input.
        core.push_owed().await;
        for failure in failures {
            tell(
                Secrets::Of(&core.credentials),
                format!("an input failed: {failure}"),
            );
        }
        Ok(())
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

    async fn handle(&mut self, input: Input) -> Result<()> {
        match input {
            Input::Line(received) => self.line(received).await,
            Input::Connection(event) => self.connection(event).await,
            input => {
                let mut core = self.core()?;
                Self::dispatch(&mut core, input).await
            }
        }
    }

    /// Takes in what a connection did. A request is run by the system
    /// for it; one whose handling fails leaves its transaction rolled
    /// back, and the client is told.
    async fn connection(&mut self, event: Event) -> Result<()> {
        let (client, text) = match event {
            Event::Late(client) => {
                self.core()?.late(client);
                return Ok(());
            }
            Event::Gone(client) => return self.core()?.gone(client),
            Event::Message(client, text) => (client, text),
        };
        let Some(message) = self.core()?.checked(client, &text)?
        else {
            return Ok(());
        };
        let id = message.id().cloned();
        let asked = AssertUnwindSafe(self.request(client, message))
            .catch_unwind()
            .await;
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
        let mut core = self.core()?;
        hamlet_store::unwind(core.realm.store().connection())?;
        tell(
            Secrets::Of(&core.credentials),
            format!("a request failed: {logged}"),
        );
        core.answer(
            client,
            fault(id.as_ref(), format!("The server failed: {shown}")),
        )
    }

    /// Runs the system for `message` from `client`; a request that
    /// awaits files or the workspace is carried out here.
    async fn request(
        &mut self,
        client: u64,
        message: ClientMessage,
    ) -> Result<()> {
        match asked::ask(self.app.world_mut(), client, message)? {
            Some(ClientMessage::Say(say)) => {
                self.core()?.say(client, say).await
            }
            Some(ClientMessage::Read(read)) => {
                self.core()?.read(client, read).await
            }
            Some(ClientMessage::MoveLodges(asked)) => {
                self.core()?.move_lodges(client, &asked.id).await
            }
            Some(_) | None => Ok(()),
        }
    }

    /// Takes in a runtime's line: its message is run by the system
    /// for it. A message whose handling fails leaves its transaction
    /// rolled back, and a runtime that waits is told; it goes on.
    async fn line(&mut self, received: Received) -> Result<()> {
        let mut core = self.core()?;
        if !core.runtimes.is_current(&received) {
            return Ok(());
        }
        let index = received.runtime;
        let Some(line) = received.line else {
            core.runtimes.failed(index);
            core.leases.gone(index);
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
        let re = heard::awaited(&message);
        let heard = AssertUnwindSafe(self.hear(index, message))
            .catch_unwind()
            .await;
        let failure = match heard {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(format!("{error:#}")),
            Err(panic) => Some(diagnostic::panicked(&*panic)),
        };
        let mut core = self.core()?;
        if let Some(failure) = failure {
            hamlet_store::unwind(core.realm.store().connection())?;
            tell(
                Secrets::Of(&core.credentials),
                format!("a runtime's line failed: {failure}"),
            );
            if let Some(re) = re {
                let message = core.credentials.scrub_text(&format!(
                    "The server failed: {failure}"
                ));
                core.runtimes.send(index, |v, id| {
                    ServerMessage::Error(ProtocolError {
                        code: ErrorCode::Failed,
                        id,
                        message,
                        re: Some(re),
                        v,
                    })
                });
            }
        }
        core.answer_credentials();
        Ok(())
    }

    /// Runs the system for `message` from runtime `index`; a tool
    /// call's request awaits the workspace and is carried out here.
    async fn hear(
        &mut self,
        index: usize,
        message: RuntimeMessage,
    ) -> Result<()> {
        match heard::hear(self.app.world_mut(), index, message)? {
            Some(RuntimeMessage::Allow(asked)) => {
                self.core()?.allow_call(index, asked).await
            }
            Some(RuntimeMessage::Operation(asked)) => {
                self.core()?.operation(index, asked).await
            }
            Some(_) | None => Ok(()),
        }
    }

    async fn dispatch(
        core: &mut Core<'_>,
        input: Input,
    ) -> Result<()> {
        match input {
            // The runner takes these in itself.
            Input::Line(_) | Input::Connection(_) => Ok(()),
            Input::Worked(done) => {
                core.owe_pushes(&done);
                core.publish()
            }
            Input::Accepted(stream) => {
                core.clients.accept(stream);
                Ok(())
            }
            Input::Ran(ran) => core.ran(ran).await,
            Input::Fitted(fitted) => {
                core.fitting.fitted(fitted);
                core.flush(false)
            }
            Input::Restart => {
                let layout = core.realm.layout().clone();
                let db = core.realm.store().connection();
                core.runtimes.start(db, &layout)
            }
            Input::Expiry => core.expire_providers(),
            Input::Dream => core.dream_tick(),
            Input::SweepImages => core.sweep_images(),
            Input::SweepFiles => core.sweep_files(),
        }
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
        world.resource_mut::<Calls>().stop_all();
        world.resource_mut::<Runtimes>().stop(grace).await;
        world.resource::<Leftovers>().end(|_| true);
        drop(world.remove_resource::<Runner>());
        Ok(())
    }
}

/// What a test reads to wait on a condition of the realm.
#[cfg(all(test, feature = "harness"))]
pub(crate) async fn until(
    server: &mut Server,
    what: &str,
    mut done: impl FnMut(&mut Core<'_>) -> Result<bool>,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(20);
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
