//! The app's runner: the server from start to stop. Its first update
//! opens and starts what it serves (`startup.rs`); then it runs the
//! inputs one at a time until the app exits, which a stop signal
//! leads to (`shutdown.rs`).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::PoisonError;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use bevy_app::{App, AppExit};
use bevy_ecs::system::SystemState;
use bevy_log::error;
use tokio::sync::mpsc;

use super::failure::{self, Failure, Fatal};
use super::follow_up::discard;
use super::input::{Input, InputReceiver, InputSender};
use super::shutdown::{self, StopRequested};
use super::timers::{self, TimerFired};
use crate::core::Core;

/// The longest the runner sleeps with nothing coming.
const IDLE: Duration = Duration::from_secs(60);

/// The app's runner: serves it on a Tokio runtime of one thread,
/// sleeping until an input comes, until it exits. A server that fails
/// says why.
pub(crate) fn runner(app: App) -> AppExit {
    let served = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|runtime| runtime.block_on(serve(app)));
    match served {
        Ok(()) => AppExit::Success,
        Err(error) => {
            failed(&error);
            AppExit::error()
        }
    }
}

/// Says why the server could not serve, as `main` would have.
#[expect(
    clippy::print_stderr,
    reason = "the failure is the server's last output for whoever \
              started it"
)]
fn failed(error: &anyhow::Error) {
    eprintln!("Error: {error:?}");
}

/// Serves `app` until it exits. A realm that can no longer be
/// served is stopped too, and its error is the server's.
async fn serve(app: App) -> Result<()> {
    let signals = Signals::new()?;
    hamlet_os::process::reap_with_server()?;
    let mut server = Server::start(app)?;
    tokio::spawn(stop_on_signals(signals, server.inputs()));
    let mut fatal = None;
    while !server.has_exited() {
        match server.step(IDLE).await {
            Ok(_) => {}
            // A second while it stops ends the server where it is.
            Err(error) if fatal.is_some() => return Err(error.0),
            Err(error) => {
                fatal = Some(error.0);
                let _ = server.inputs().send(StopRequested);
            }
        }
    }
    fatal.map_or(Ok(()), Err)
}

/// The first SIGTERM or Ctrl-C triggers [`StopRequested`], and a
/// second exits at once.
async fn stop_on_signals(mut signals: Signals, inputs: InputSender) {
    signals.next().await;
    let _ = inputs.send(StopRequested);
    signals.next().await;
    std::process::exit(130);
}

struct Signals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(windows)]
    interrupt: tokio::signal::windows::CtrlC,
    /// The signal `hamlet dev` sends the server it keeps in a
    /// process group of its own.
    #[cfg(windows)]
    ctrl_break: tokio::signal::windows::CtrlBreak,
}

impl Signals {
    fn new() -> Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Ok(Self {
                interrupt: signal(SignalKind::interrupt())?,
                terminate: signal(SignalKind::terminate())?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                interrupt: tokio::signal::windows::ctrl_c()?,
                ctrl_break: tokio::signal::windows::ctrl_break()?,
            })
        }
    }

    async fn next(&mut self) {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = self.interrupt.recv() => {}
                _ = self.terminate.recv() => {}
            }
        }
        #[cfg(windows)]
        {
            tokio::select! {
                _ = self.interrupt.recv() => {}
                _ = self.ctrl_break.recv() => {}
            }
        }
    }
}

/// The served realm: the app whose world holds it, and the channel
/// its inputs arrive on.
pub(crate) struct Server {
    pub app: App,
    core: SystemState<Core<'static, 'static>>,
    inputs: mpsc::UnboundedReceiver<Input>,
}

impl Server {
    /// Serves `app`, which [`crate::create_app`] made: its first
    /// update opens what it serves in `PreStartup` and starts it in
    /// `Startup`.
    pub fn start(mut app: App) -> Result<Self> {
        let inputs = app
            .world_mut()
            .remove_resource::<InputReceiver>()
            .context("the inputs are taken already")?
            .0
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);
        let mut server = Self {
            core: SystemState::new(app.world_mut()),
            app,
            inputs,
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

    /// Runs the next input, a timer that came due, or nothing after
    /// `wait`. Returns whether anything ran. A stopping server runs
    /// no timer.
    pub async fn step(
        &mut self,
        wait: Duration,
    ) -> Result<bool, Fatal> {
        let due = if shutdown::is_stopping(self.app.world()) {
            None
        } else {
            timers::next(self.app.world_mut()).map_err(Fatal)?
        };
        let due = async move {
            match due {
                Some(at) => tokio::time::sleep_until(at.into()).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            Some(input) = self.inputs.recv() => {
                self.run(input)?;
                Ok(true)
            }
            () = due => {
                let world = self.app.world_mut();
                let fires = timers::due(world, Instant::now())
                    .map_err(Fatal)?;
                // Each timer is an input of its own: one that fails
                // fails alone.
                for fire in fires {
                    self.run(Input::of(TimerFired(fire)))?;
                }
                Ok(true)
            }
            () = tokio::time::sleep(wait) => Ok(false),
        }
    }

    /// Runs `input`: writes its message and updates the app once, in
    /// one transaction. One that fails or panics is rolled back
    /// whole, nothing it queued goes out, and it is logged; the
    /// server goes on unless the realm cannot be served. Nothing here
    /// awaits: what awaits runs on a task, whose result is an input.
    pub fn run(&mut self, input: Input) -> Result<(), Fatal> {
        input.write(self.app.world_mut());
        let failures = self.update()?;
        let core = self.core().map_err(Fatal)?;
        for failure in failures {
            let words = core.credentials.scrub_text(&failure.logged);
            error!("an input failed: {words}");
        }
        Ok(())
    }

    /// Updates the app once. Gives what failed in it.
    fn update(&mut self) -> Result<Vec<Failure>, Fatal> {
        let updated =
            catch_unwind(AssertUnwindSafe(|| self.app.update()));
        let mut failures = failure::recorded();
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
        Ok(self.app.world_mut().run_system_cached(discard)?)
    }

    /// Where the inputs go, for what sends them from off the loop.
    pub fn inputs(&self) -> InputSender {
        self.app.world().resource::<InputSender>().clone()
    }

    /// Whether the app has exited: the server stopped.
    pub fn has_exited(&self) -> bool {
        self.app.should_exit().is_some()
    }
}

/// A test's stop of the server: the signal a stop is, then its inputs
/// until it exits.
#[cfg(all(test, feature = "harness"))]
pub(crate) async fn shut_down(server: &mut Server) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    server
        .inputs()
        .send(StopRequested)
        .map_err(|_| anyhow::anyhow!("the server stopped already"))?;
    while !server.has_exited() {
        anyhow::ensure!(
            Instant::now() < deadline,
            "timed out stopping"
        );
        server.step(Duration::from_millis(100)).await?;
    }
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
    use crate::conversations::{chats, inputs, items};
    use crate::society::hand;

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
        let app = crate::create_app(crate::app::options::Options {
            home: dir.join("realm"),
            port: 0,
            faux: Some(faux),
            isolation: hamlet_sandbox::Isolation::Sandboxed,
        });
        let mut server = Server::start(app).unwrap();
        let mut core = server.core().unwrap();
        let chief_id = core.realm.chief();
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
        shut_down(&mut server).await.unwrap();
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
        shut_down(&mut server).await.unwrap();
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
        shut_down(&mut server).await.unwrap();
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
        let task =
            crate::conversations::tasks::get(db, said.task).unwrap();
        assert!(
            task.done_at.is_some(),
            "the instruction is answered"
        );
        shut_down(&mut server).await.unwrap();
    }
}
