//! The core plugin: settings, the model, the sandbox and the home
//! folder as resources, plus the events that change them.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use bevy_app::{App, Plugin, Startup, Update};
use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::*;
use hamlet_home::{Home, Skill};
use hamlet_model::catalogue::{self, Catalogue, Listed};
use hamlet_model::config::{self, Config};
use hamlet_model::provider::{Endpoints, Provider, Replay};
use hamlet_model::{chatgpt, signin};
use hamlet_sandbox::Sandbox;
use serde_json::Value;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::agents::AgentsPlugin;

/// An endpoint's name, whether it is on this machine, and its
/// models.
type ListResult = (String, bool, Result<Vec<Listed>, String>);

/// The models a test of the connection found.
type Tested = Result<Vec<String>, String>;

/// The outcome of a trial of the model.
type Tried = Result<(), String>;

/// The key, or the account's email, a sign-in gave.
type SignedIn = Result<String, String>;

/// The settings, the model, the sandbox and the home as resources,
/// and the hamlets of the home as entities.
pub struct CorePlugin {
    pub home: Home,
    pub sandbox: &'static dyn Sandbox,
    pub tokio: tokio::runtime::Handle,
    pub config: Config,
    /// Recorded assistant messages that answer in a model's place.
    pub replay: Option<Vec<Value>>,
}

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        let provider: Arc<dyn Provider> = match &self.replay {
            Some(responses) => {
                Arc::new(Replay::new(responses.clone()))
            }
            None => provider_for(&self.config, &self.home.root),
        };
        app.insert_resource(Tokio(self.tokio.clone()))
            .insert_resource(HomeDir(self.home.clone()))
            .insert_resource(Sandboxing(self.sandbox))
            .insert_resource(Model {
                provider,
                replay: self.replay.is_some(),
            })
            .insert_resource(Settings {
                config: self.config.clone(),
                error: None,
            })
            .init_resource::<Rounds<Tested>>()
            .init_resource::<Rounds<ListResult>>()
            .init_resource::<Rounds<Tried>>()
            .init_resource::<Trial>()
            .add_observer(try_model)
            .init_resource::<Rounds<SignedIn>>()
            .init_resource::<Signing>()
            .add_observer(sign_in)
            .init_resource::<Listing>()
            .add_observer(list_models)
            .init_resource::<Link>()
            .init_resource::<Skills>()
            .init_resource::<Shelved>()
            .add_observer(save_settings)
            .add_observer(test_connection)
            .add_observer(
                |_: On<RefreshLists>, mut commands: Commands| {
                    commands.queue(refresh_lists)
                },
            )
            .add_systems(Startup, refresh_lists)
            .add_systems(
                Update,
                (
                    hear::<Tested, Link>.run_if(answered::<Tested>),
                    hear::<Tried, Trial>.run_if(answered::<Tried>),
                    hear::<SignedIn, Signing>
                        .run_if(answered::<SignedIn>),
                    poll_lists.run_if(answered::<ListResult>),
                ),
            );
        app.add_plugins((
            AgentsPlugin {
                home: self.home.root.clone(),
            },
            crate::catalogue::plugin,
        ));
    }
}

/// The tokio runtime that background work is spawned on.
#[derive(Resource)]
pub struct Tokio(pub tokio::runtime::Handle);

/// The home the app opened.
#[derive(Resource)]
pub struct HomeDir(pub Home);

/// The sandbox the agents' tools run in.
#[derive(Resource)]
pub struct Sandboxing(pub &'static dyn Sandbox);

/// The settings in force.
#[derive(Resource)]
pub struct Settings {
    pub config: Config,
    /// The error of the last save, if it failed.
    pub error: Option<String>,
}

fn provider_for(c: &Config, home: &Path) -> Arc<dyn Provider> {
    Arc::new(Endpoints::new(c, home))
}

/// The provider every job is handed, as the settings stand.
#[derive(Resource)]
pub struct Model {
    provider: Arc<dyn Provider>,
    /// Answers come from a recorded session, so settings don't
    /// change the provider.
    pub replay: bool,
}

impl Model {
    pub fn provider(&self) -> Arc<dyn Provider> {
        self.provider.clone()
    }
}

/// The endpoint's reach as of the last test.
#[derive(Resource, Clone, Debug, PartialEq, Default)]
pub enum Link {
    #[default]
    Unknown,
    Testing,
    Ok {
        models: Vec<String>,
    },
    Failed(String),
}

impl From<Tested> for Link {
    fn from(found: Tested) -> Self {
        match found {
            Ok(models) => Link::Ok { models },
            Err(e) => Link::Failed(e),
        }
    }
}

/// The library's skills as last read.
#[derive(Resource, Default)]
pub struct Skills(pub Vec<Skill>);

/// The time the library's folder had last changed when its skills
/// were last read.
#[derive(Resource, Default)]
struct Shelved(Option<SystemTime>);

impl Shelved {
    fn now(home: &Home) -> Option<SystemTime> {
        let shelves = hamlet_rules::library::Library::of(&home.root);
        std::fs::metadata(shelves.root()).ok()?.modified().ok()
    }
}

/// The answers to something asked in the background, round after
/// round. Only the last round asked is heard.
#[derive(Resource)]
struct Rounds<R: Send + 'static> {
    tx: UnboundedSender<(u64, R)>,
    rx: UnboundedReceiver<(u64, R)>,
    /// The number of the last round asked.
    latest: u64,
}

impl<R: Send + 'static> Default for Rounds<R> {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Rounds { tx, rx, latest: 0 }
    }
}

impl<R: Send + 'static> Rounds<R> {
    /// Begins a round and returns the taker of its answers.
    fn begin(&mut self) -> impl Fn(R) + Clone + Send + 'static {
        self.latest += 1;
        let (round, tx) = (self.latest, self.tx.clone());
        move |answer| {
            let _ = tx.send((round, answer));
        }
    }

    /// The next answer of the last round asked.
    fn next(&mut self) -> Option<R> {
        loop {
            let (round, answer) = self.rx.try_recv().ok()?;
            // An earlier round may answer after a later one was
            // asked.
            if round == self.latest {
                return Some(answer);
            }
        }
    }
}

/// True when an answer is waiting in `rounds`.
fn answered<R: Send + 'static>(rounds: Res<Rounds<R>>) -> bool {
    !rounds.rx.is_empty()
}

/// Makes `state` what the last round asked came to.
fn hear<R, S>(mut rounds: ResMut<Rounds<R>>, mut state: ResMut<S>)
where
    R: Send + 'static,
    S: Resource<Mutability = Mutable> + From<R>,
{
    while let Some(answer) = rounds.next() {
        *state = answer.into();
    }
}

/// Saves the settings and switches the model right away.
#[derive(Event)]
pub struct SaveSettings(pub Config);

/// Lists the endpoint's models in the background; the result lands in
/// `Link`.
#[derive(Event)]
pub struct TestConnection(pub Config);

/// Asks every endpoint of the settings for its models and writes them
/// into the home's catalogue. Each endpoint's answer lands in
/// `Listing`.
#[derive(Event)]
pub struct ListModels;

/// Each endpoint's answer when it was last asked for its models: how
/// many it has, or the reason it could not be asked.
#[derive(Resource, Default)]
pub struct Listing {
    /// Endpoints that have not answered yet.
    pub asking: usize,
    pub said: BTreeMap<String, Result<usize, String>>,
}

/// Re-reads the skill and session lists from disk.
#[derive(Event)]
pub struct RefreshLists;

/// Asks the default model of these settings for a word in the
/// background, to prove that it answers; the result lands in
/// `Trial`.
#[derive(Event)]
pub struct TryModel(pub Config);

/// The outcome of the last trial of a model.
#[derive(Resource, Clone, Debug, PartialEq, Default)]
pub enum Trial {
    #[default]
    Untried,
    Trying,
    Answered,
    Failed(String),
}

impl From<Tried> for Trial {
    fn from(went: Tried) -> Self {
        match went {
            Ok(()) => Trial::Answered,
            Err(e) => Trial::Failed(e),
        }
    }
}

/// Signs the chief in in a browser, in the background; what it gives
/// lands in `Signing`.
#[derive(Event, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignIn {
    /// For a key to OpenRouter.
    OpenRouter,
    /// For the use of the chief's ChatGPT plan, kept in the home.
    ChatGpt,
}

/// The state of the last sign-in.
#[derive(Resource, Clone, Debug, PartialEq, Default)]
pub enum Signing {
    #[default]
    Idle,
    /// The browser is open and the chief has yet to approve.
    Waiting,
    /// The key the chief was given, or for ChatGPT the email of the
    /// account.
    Done(String),
    Failed(String),
}

impl From<SignedIn> for Signing {
    fn from(went: SignedIn) -> Self {
        match went {
            Ok(key) => Signing::Done(key),
            Err(e) => Signing::Failed(e),
        }
    }
}

/// Opens `url`, or a file, in the program the system keeps for it.
pub fn browse(url: &str) -> std::io::Result<()> {
    use std::process::Command;
    let mut open = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(target_os = "windows") {
        // Not `start`, which reads an ampersand as the end of the
        // address.
        let mut open = Command::new("rundll32");
        open.arg("url.dll,FileProtocolHandler");
        open
    } else {
        Command::new("xdg-open")
    };
    open.arg(url).spawn().map(|_| ())
}

fn sign_in(
    to: On<SignIn>,
    tokio: Res<Tokio>,
    home: Res<HomeDir>,
    mut sign_ins: ResMut<Rounds<SignedIn>>,
    mut signing: ResMut<Signing>,
) {
    *signing = Signing::Waiting;
    let (to, home) = (*to.event(), home.0.root.clone());
    let tell = sign_ins.begin();
    let browsed = |url: &str| {
        browse(url).map_err(|e| {
            anyhow::anyhow!("could not open the browser: {e}")
        })
    };
    tokio.0.spawn(async move {
        let went = async {
            match to {
                SignIn::OpenRouter => {
                    let begun = signin::begin().await?;
                    browsed(&begun.url)?;
                    begun.finish().await
                }
                SignIn::ChatGpt => {
                    let begun = chatgpt::begin(&home).await?;
                    browsed(&begun.url)?;
                    begun.finish(&home).await
                }
            }
        }
        .await
        .map_err(|e| format!("{e:#}"));
        tell(went);
    });
}

fn save_settings(
    save: On<SaveSettings>,
    home: Res<HomeDir>,
    mut model: ResMut<Model>,
    mut settings: ResMut<Settings>,
) {
    let config = save.event().0.clone();
    match config.save(&home.0.root).and_then(|()| {
        // The models of an endpoint taken out go with it.
        let mut all = Catalogue::load(&home.0.root);
        let known = all.models.len();
        all.models.retain(|model| {
            config.endpoint(&model.endpoint).is_some()
        });
        if all.models.len() < known {
            all.save(&home.0.root)?;
        }
        Ok(())
    }) {
        Ok(()) => {
            if !model.replay {
                model.provider = provider_for(&config, &home.0.root);
            }
            *settings = Settings {
                config,
                error: None,
            };
        }
        Err(e) => settings.error = Some(format!("{e:#}")),
    }
}

fn test_connection(
    test: On<TestConnection>,
    tokio: Res<Tokio>,
    home: Res<HomeDir>,
    mut tests: ResMut<Rounds<Tested>>,
    mut link: ResMut<Link>,
) {
    *link = Link::Testing;
    let (config, tell) = (test.event().0.clone(), tests.begin());
    let home = home.0.root.clone();
    tokio.0.spawn(async move {
        let found = config::test_connection(&config, &home)
            .await
            .map_err(|e| format!("{e:#}"));
        tell(found);
    });
}

fn try_model(
    asked: On<TryModel>,
    tokio: Res<Tokio>,
    home: Res<HomeDir>,
    mut trials: ResMut<Rounds<Tried>>,
    mut trial: ResMut<Trial>,
) {
    *trial = Trial::Trying;
    let (config, tell) = (asked.event().0.clone(), trials.begin());
    let home = home.0.root.clone();
    tokio.0.spawn(async move {
        let went = config::try_model(&config, &home)
            .await
            .map_err(|e| format!("{e:#}"));
        tell(went);
    });
}

fn list_models(
    _: On<ListModels>,
    tokio: Res<Tokio>,
    home: Res<HomeDir>,
    mut lists: ResMut<Rounds<ListResult>>,
    settings: Res<Settings>,
    mut listing: ResMut<Listing>,
) {
    listing.said.clear();
    // The answers of the round before are heard no more.
    listing.asking = 0;
    let tell = lists.begin();
    for endpoint in &settings.config.endpoints {
        let (endpoint, home) =
            (endpoint.clone(), home.0.root.clone());
        listing.asking += 1;
        let tell = tell.clone();
        tokio.0.spawn(async move {
            let listed = async {
                let key = endpoint.key(&home).await?;
                catalogue::list(&endpoint.base_url, key.as_deref())
                    .await
            }
            .await
            .map_err(|e| format!("{e:#}"));
            let local = catalogue::is_local(&endpoint.base_url);
            tell((endpoint.name, local, listed));
        });
    }
}

fn poll_lists(
    home: Res<HomeDir>,
    settings: Res<Settings>,
    mut lists: ResMut<Rounds<ListResult>>,
    mut listing: ResMut<Listing>,
) {
    while let Some((endpoint, local, listed)) = lists.next() {
        listing.asking = listing.asking.saturating_sub(1);
        // An endpoint taken out of the settings while it was asked.
        if !settings
            .config
            .endpoints
            .iter()
            .any(|e| e.name == endpoint)
        {
            continue;
        }
        let said = listed.and_then(|listed| {
            let mut all = Catalogue::load(&home.0.root);
            all.take_in(&endpoint, &listed, local);
            all.save(&home.0.root).map_err(|e| format!("{e:#}"))?;
            Ok(listed.len())
        });
        listing.said.insert(endpoint, said);
    }
}

/// Reads the skills again if the library's folder has changed.
pub(crate) fn reshelve(world: &mut World) {
    let home = &world.resource::<HomeDir>().0;
    if Shelved::now(home) != world.resource::<Shelved>().0 {
        refresh_lists(world);
    }
}

fn refresh_lists(world: &mut World) {
    let home = world.resource::<HomeDir>().0.clone();
    // Read before the shelves, so a change made meanwhile is seen
    // next time.
    world.resource_mut::<Shelved>().0 = Shelved::now(&home);
    world.resource_mut::<Skills>().0 =
        hamlet_rules::library::Library::of(&home.root).index();
}
