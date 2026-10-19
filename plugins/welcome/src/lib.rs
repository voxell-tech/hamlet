//! The first-run screen: leads the chief of a home that was never
//! set up to a model that answers, by a provider's key or by a
//! server on this computer.

use std::path::Path;

mod pages;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input_focus::InputFocus;
use bevy::text::EditableText;
use hamlet_core::core::{
    HomeDir, Link, ListModels, Model, SaveSettings, Settings, SignIn,
    Signing, TestConnection, Trial, TryModel,
};
use hamlet_limits::models;
use hamlet_model::catalogue::{self, Catalogue, Listed};
use hamlet_model::chatgpt;
use hamlet_model::config::providers::{
    LOCAL, OPENROUTER, OTHER, PROVIDERS, served,
};
use hamlet_model::config::{Config, Endpoint};
use hamlet_sdk::frame::{Place, PointerWait};
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};
use hamlet_sdk::settings::{AddEndpoint, SetUpAgain, ShowModels};
use hamlet_sdk::{
    AnyView, Cx, View, feel, icons, looks, platform, with_cx,
};

pub use self::pages::view;

const NOTHING_LOCAL: &str = "No model server answered here. Install \
                             Ollama from ollama.com, download a \
                             model, then choose Look Again.";

/// Adds the Welcome plugin, open on a home that was never set up.
pub fn plugin(app: &mut App) {
    let home = &app.world().resource::<HomeDir>().0.root;
    let replay = app.world().resource::<Model>().replay;
    let welcome = Welcome::at_launch(home, replay);
    app.insert_resource(welcome)
        .init_resource::<Committing>()
        .init_resource::<PointerWait>()
        .add_observer(|_: On<SetUpAgain>, mut commands: Commands| {
            commands.queue(again);
        })
        .add_observer(|_: On<AddEndpoint>, mut commands: Commands| {
            commands.queue(add);
        })
        .add_systems(
            Update,
            (
                (
                    run_committed.run_if(|waits: Res<Committing>| {
                        waits.waits()
                    }),
                    wait.run_if(resource_changed::<Committing>),
                )
                    .chain(),
                follow,
                read_narrow,
            ),
        )
        .add_hamlet_plugin::<WelcomePlugin>();
}

/// The plugin that leads the chief to a model that answers.
pub struct WelcomePlugin;

impl HamletPlugin for WelcomePlugin {
    fn manifest() -> Manifest {
        Manifest::built_in("welcome").named("Welcome")
    }

    fn entry(entry: &mut Entry) {
        entry.frame_part(Place::Dock, 0, view);
    }
}

/// The step of the screen the chief is at.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Step {
    /// The two ways in.
    #[default]
    Choice,
    /// A provider and its key.
    Key,
    /// The chief is approving in a browser.
    SignIn,
    /// The search for a server on this computer.
    Local,
    /// The models of whoever answered.
    Models,
}

/// The first-run screen: its being open, and the choices and words of
/// the chief on it.
#[derive(Resource, Default)]
pub struct Welcome {
    pub open: bool,
    /// It is adding a place for models to a home that has its model
    /// already, which it then leaves as it is.
    pub adding: bool,
    pub step: Step,
    /// The provider chosen, by its place in the list. Past the
    /// list's end for an address the chief types.
    pub provider: usize,
    url: String,
    key: String,
    narrow: String,
    /// The endpoint that was asked last.
    asked: Option<Endpoint>,
    /// The settings whose default model is being asked for a word.
    trying: Option<Config>,
    /// Its answer is still awaited.
    pub asking: bool,
    /// The sign-in in the browser that is awaited.
    pub signing: Option<SignIn>,
    /// The server on this computer that is being tried.
    local: usize,
    /// The name of whoever answered, as the chief knows them, and
    /// their models.
    pub from: String,
    pub models: Vec<String>,
    /// The reason the last thing asked did not work, with what to
    /// do.
    pub problem: Option<String>,
}

impl Welcome {
    /// The screen as a launch finds it: open on a home that was
    /// never set up.
    pub(crate) fn at_launch(home: &Path, replay: bool) -> Welcome {
        Welcome {
            open: !replay && unset(home),
            ..Welcome::default()
        }
    }
}

/// Nobody chose the home's model: it has no settings of its own, and
/// none was named for this run.
fn unset(home: &Path) -> bool {
    !Config::saved(home) && std::env::var_os("HAMLET_MODEL").is_none()
}

/// A step of the screen that waits for the field being edited to
/// commit its text.
#[derive(Clone, Copy)]
enum Act {
    Provider(usize),
    Connect,
}

/// The step that runs a few frames from now, once the field that
/// lost the focus has committed its text.
#[derive(Resource, Default)]
pub struct Committing(Option<(Act, u8)>);

impl Committing {
    /// A step is about to run.
    pub fn waits(&self) -> bool {
        self.0.is_some()
    }
}

/// Has the pointer say the app is busy while a step waits.
fn wait(
    committing: Res<Committing>,
    mut pointer: ResMut<PointerWait>,
) {
    pointer.set_if_neq(PointerWait(committing.waits()));
}

fn request(world: &mut World, act: Act) {
    world.resource_mut::<InputFocus>().clear();
    world.resource_mut::<Committing>().0 = Some((act, 3));
}

fn run_committed(world: &mut World) {
    let Some((act, frames)) = world.resource::<Committing>().0 else {
        return;
    };
    if frames > 0 {
        world.resource_mut::<Committing>().0 =
            Some((act, frames - 1));
        return;
    }
    world.resource_mut::<Committing>().0 = None;
    match act {
        Act::Provider(which) => {
            let mut welcome = world.resource_mut::<Welcome>();
            welcome.provider = which;
            welcome.problem = None;
        }
        Act::Connect => connect(world),
    }
}

/// Opens the screen from its start.
pub fn again(world: &mut World) {
    *world.resource_mut::<Welcome>() = Welcome {
        open: true,
        ..Welcome::default()
    };
}

/// Opens the screen to add another place for models, by the same
/// steps a home is first set up with.
pub fn add(world: &mut World) {
    *world.resource_mut::<Welcome>() = Welcome {
        open: true,
        adding: true,
        ..Welcome::default()
    };
}

/// Puts the endpoint that answered among the home's, lists what it
/// serves, and shows the models for the chief to allow.
fn added(world: &mut World) {
    let Some(endpoint) = world.resource::<Welcome>().asked.clone()
    else {
        return;
    };
    let mut config = world.resource::<Settings>().config.clone();
    settle(&mut config, endpoint);
    world.trigger(SaveSettings(config));
    if let Some(why) = world.resource::<Settings>().error.clone() {
        let mut welcome = world.resource_mut::<Welcome>();
        welcome.problem =
            Some(format!("It could not be saved. ({why})"));
        return;
    }
    world.trigger(ListModels);
    world.trigger(ShowModels);
    close(world);
}

fn close(world: &mut World) {
    *world.resource_mut::<Welcome>() = Welcome::default();
}

fn go(world: &mut World, step: Step) {
    let mut welcome = world.resource_mut::<Welcome>();
    welcome.step = step;
    welcome.asking = false;
    welcome.signing = None;
    welcome.trying = None;
    welcome.problem = None;
    welcome.narrow.clear();
}

/// Asks `endpoint` for its models. The answer lands in [`Link`].
fn test(endpoint: &Endpoint) -> TestConnection {
    TestConnection(Config {
        model: catalogue::named(&endpoint.name, ""),
        endpoints: vec![endpoint.clone()],
        ..Config::default()
    })
}

fn ask(world: &mut World, endpoint: Endpoint, from: &str) {
    let asking = test(&endpoint);
    let mut welcome = world.resource_mut::<Welcome>();
    welcome.asked = Some(endpoint);
    welcome.from = from.to_string();
    welcome.asking = true;
    welcome.problem = None;
    world.trigger(asking);
}

/// Asks the provider chosen, with the key typed.
fn connect(world: &mut World) {
    let welcome = world.resource::<Welcome>();
    let key = welcome.key.trim().to_string();
    let known = PROVIDERS.get(welcome.provider);
    let base_url = match known {
        Some(provider) => provider.base_url.to_string(),
        None => welcome.url.trim().to_string(),
    };
    let problem = if base_url.is_empty() {
        Some("Type the address where your model runs first.")
    } else if !base_url.contains("://") {
        Some("The address should start with https:// or http://.")
    } else if known.is_some() && key.is_empty() {
        Some("Paste your key first.")
    } else {
        None
    };
    if let Some(why) = problem {
        world.resource_mut::<Welcome>().problem = Some(why.into());
        return;
    }
    let endpoint = Endpoint {
        name: known
            .map_or(OTHER, |provider| provider.endpoint)
            .into(),
        base_url,
        chatgpt: false,
        api_key: (!key.is_empty()).then_some(key),
    };
    let from = known.map_or("It", |provider| provider.name);
    ask(world, endpoint, from);
}

/// Sends the chief to sign in in a browser: to OpenRouter, to come
/// back with a key, or to ChatGPT, to come back signed in to the
/// plan.
fn sign_in(world: &mut World, to: SignIn) {
    go(world, Step::SignIn);
    world.resource_mut::<Welcome>().signing = Some(to);
    world.trigger(to);
}

/// Looks for a server on this computer, the first of the list
/// first.
fn look(world: &mut World) {
    go(world, Step::Local);
    world.resource_mut::<Welcome>().local = 0;
    ask(world, served(&LOCAL[0]), LOCAL[0].name);
}

/// The reason an endpoint did not list its models, or a model of it
/// did not answer (`tried`), in plain words with what to do next.
fn plainly(why: &str, tried: bool) -> String {
    let low = why.to_lowercase();
    let said = |status: u16| low.contains(&format!("http {status}"));
    // Its errors are written to be shown as they are.
    if low.contains("chatgpt") {
        why.chars().take(400).collect()
    } else if said(401) || said(403) || low.contains("api key") {
        "That key was not accepted. Check that you copied all of it \
         and that it is still active."
            .into()
    } else if said(402)
        || ["credit", "quota", "billing"]
            .iter()
            .any(|word| low.contains(word))
    {
        "Your provider account has no credit left. Add some, then \
         try again."
            .into()
    } else if low.contains("could not reach") {
        "Could not reach it. Check your connection and the address."
            .into()
    } else if low.contains("in time") {
        "It took too long to answer. Try again, or pick a smaller \
         model."
            .into()
    } else if tried {
        let short = why.chars().take(160).collect::<String>();
        format!(
            "That model did not answer a hello. Pick another one. \
             ({short})"
        )
    } else if said(404) {
        "That address did not answer as a model provider. It usually \
         ends in /v1."
            .into()
    } else {
        let short = why.chars().take(160).collect::<String>();
        format!("It answered with an error. Try again. ({short})")
    }
}

/// Moves the screen on when the endpoint it asked has answered,
/// and keeps the model that was tried once it has said its word.
pub fn follow(
    link: Res<Link>,
    trial: Res<Trial>,
    signing: Res<Signing>,
    mut welcome: ResMut<Welcome>,
    mut commands: Commands,
) {
    if let Some(to) = welcome.signing {
        match (&*signing, to) {
            (Signing::Idle | Signing::Waiting, _) => {}
            // The key it gave is used as one that was typed.
            (Signing::Done(key), SignIn::OpenRouter) => {
                welcome.signing = None;
                welcome.provider = OPENROUTER;
                welcome.key.clone_from(key);
                welcome.step = Step::Key;
                commands.queue(connect);
            }
            (Signing::Done(_), SignIn::ChatGpt) => {
                welcome.signing = None;
                commands.queue(|world: &mut World| {
                    ask(world, chatgpt::endpoint(), "ChatGPT")
                });
            }
            (Signing::Failed(why), SignIn::OpenRouter) => {
                welcome.signing = None;
                welcome.step = Step::Key;
                welcome.provider = OPENROUTER;
                welcome.problem = Some(format!(
                    "Signing in did not finish ({why}). Try again, \
                     or make a key and paste it here."
                ));
            }
            (Signing::Failed(why), SignIn::ChatGpt) => {
                welcome.signing = None;
                welcome.problem = Some(format!(
                    "Signing in did not finish ({why}). Go back and \
                     try again."
                ));
            }
        }
        return;
    }
    if !welcome.asking {
        return;
    }
    if let Some(config) = welcome.trying.clone() {
        match &*trial {
            Trial::Untried | Trial::Trying => {}
            Trial::Answered => {
                commands.queue(move |world: &mut World| {
                    keep(world, config)
                });
                welcome.trying = None;
            }
            Trial::Failed(why) => {
                welcome.problem = Some(plainly(why, true));
                welcome.trying = None;
                welcome.asking = false;
            }
        }
        return;
    }
    let found = match &*link {
        Link::Unknown | Link::Testing => return,
        Link::Ok { models } => Ok(models.clone()),
        Link::Failed(why) => Err(why.clone()),
    };
    welcome.asking = false;
    if let Ok(models) = &found
        && !models.is_empty()
    {
        welcome.problem = None;
        if welcome.adding {
            commands.queue(added);
            return;
        }
        welcome.models.clone_from(models);
        welcome.step = Step::Models;
        return;
    }
    if welcome.step != Step::Local {
        welcome.problem = Some(match found {
            Ok(_) => "It answered, but it has no models for this \
                      key."
                .into(),
            Err(why) => plainly(&why, false),
        });
        return;
    }
    if found.is_ok() && welcome.problem.is_none() {
        let name = LOCAL[welcome.local].name;
        welcome.problem = Some(format!(
            "{name} is running but has no model. Download one, then \
             choose Look Again."
        ));
    }
    welcome.local += 1;
    match LOCAL.get(welcome.local) {
        Some(next) => {
            let endpoint = served(next);
            commands.trigger(test(&endpoint));
            welcome.asked = Some(endpoint);
            welcome.from = next.name.into();
            welcome.asking = true;
        }
        None if welcome.problem.is_none() => {
            welcome.problem = Some(NOTHING_LOCAL.into());
        }
        None => {}
    }
}

/// Puts `endpoint` among those of `config` and returns the name it
/// has there. One at the same address, signed in to the same way, is
/// given the key and keeps its name, and a name another has is
/// numbered.
fn settle(config: &mut Config, endpoint: Endpoint) -> String {
    let at = |url: &str| url.trim_end_matches('/').to_string();
    let same = config.endpoints.iter_mut().find(|known| {
        at(&known.base_url) == at(&endpoint.base_url)
            && known.chatgpt == endpoint.chatgpt
    });
    if let Some(known) = same {
        known.api_key = endpoint.api_key;
        return known.name.clone();
    }
    let name = std::iter::once(endpoint.name.clone())
        .chain((2..).map(|n| format!("{}-{n}", endpoint.name)))
        .find(|name| config.endpoint(name).is_none())
        .expect("the numbers do not end");
    config.endpoints.push(Endpoint {
        name: name.clone(),
        ..endpoint
    });
    name
}

/// Asks the model `id` of the endpoint that answered for a word,
/// as the home's default model would be asked.
fn pick(world: &mut World, id: &str) {
    let home = world.resource::<HomeDir>().0.root.clone();
    let Some(endpoint) = world.resource::<Welcome>().asked.clone()
    else {
        return;
    };
    let mut config = world.resource::<Settings>().config.clone();
    // A home that was never set up has only the endpoint it was
    // born with, which nobody chose.
    if !Config::saved(&home) {
        config.endpoints.clear();
    }
    let name = settle(&mut config, endpoint);
    config.model = catalogue::named(&name, id);
    let mut welcome = world.resource_mut::<Welcome>();
    welcome.trying = Some(config.clone());
    welcome.asking = true;
    welcome.problem = None;
    world.trigger(TryModel(config));
}

/// Saves `config`, whose default model has answered, allows that
/// model in the catalogue, and closes the screen.
fn keep(world: &mut World, config: Config) {
    let home = world.resource::<HomeDir>().0.root.clone();
    world.trigger(SaveSettings(config.clone()));
    let mut all = Catalogue::load(&home);
    if let Some((name, id)) = catalogue::parts(&config.model) {
        let chosen = Listed {
            id: id.to_string(),
            price: None,
            context: None,
        };
        let local = config
            .endpoint(name)
            .is_some_and(|at| catalogue::is_local(&at.base_url));
        all.take_in(name, &[chosen], local);
    }
    if let Some(model) = all.get_mut(&config.model) {
        model.allowed = true;
    }
    let failed =
        world.resource::<Settings>().error.clone().or_else(|| {
            all.save(&home).err().map(|e| format!("{e:#}"))
        });
    if let Some(why) = failed {
        let mut welcome = world.resource_mut::<Welcome>();
        welcome.asking = false;
        welcome.problem =
            Some(format!("Your choice could not be saved. ({why})"));
        return;
    }
    // The rest of what the endpoint serves, with its prices.
    world.trigger(ListModels);
    world.trigger(TestConnection(config));
    close(world);
}

/// The field the list of models is narrowed in.
#[derive(Component)]
pub(crate) struct Narrowing;

/// Follows what narrows the models as it is typed.
pub(crate) fn read_narrow(
    field: Query<&Children, With<Narrowing>>,
    texts: Query<&EditableText>,
    mut welcome: ResMut<Welcome>,
) {
    let Some(text) = field
        .iter()
        .flat_map(|children| children.iter().copied())
        .find_map(|child| texts.get(child).ok())
    else {
        return;
    };
    if text.value() != welcome.narrow.as_str() {
        welcome.narrow = text.value().to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chosen_endpoint_keeps_the_name_of_one_at_its_address() {
        let mut config = Config::default();
        let ollama = served(&LOCAL[0]);
        assert_eq!(settle(&mut config, ollama), "local");
        assert_eq!(config.endpoints.len(), 1);

        // The same name at another address is another endpoint.
        let far = |key: &str| Endpoint {
            name: "local".into(),
            base_url: "https://far.example/v1/".into(),
            chatgpt: false,
            api_key: Some(key.into()),
        };
        assert_eq!(settle(&mut config, far("first")), "local-2");
        // Chosen again, it is the same one with the new key.
        assert_eq!(settle(&mut config, far("second")), "local-2");
        assert_eq!(config.endpoints.len(), 2);
        assert_eq!(
            config.endpoints[1].api_key.as_deref(),
            Some("second")
        );
    }

    #[test]
    fn every_provider_is_named_as_an_endpoint_may_be() {
        for provider in PROVIDERS.iter().chain(&LOCAL) {
            assert!(
                hamlet_model::config::names_endpoint(
                    provider.endpoint
                ),
                "{}",
                provider.endpoint
            );
            // The test asks the endpoint its own model's name tells
            // of.
            let asked = test(&served(provider)).0;
            assert_eq!(
                asked.default_endpoint().map(|e| &*e.base_url),
                Some(provider.base_url)
            );
        }
        assert!(hamlet_model::config::names_endpoint(OTHER));
    }

    #[test]
    fn a_refused_key_is_told_from_a_server_that_is_not_there() {
        let refused =
            plainly("HTTP 401 Unauthorized: {\"message\":1}", false);
        assert!(refused.contains("key was not accepted"));
        // Not every provider answers a bad key with the same status.
        let invalid =
            "HTTP 400 Bad Request: Please pass a valid API key";
        assert_eq!(plainly(invalid, false), refused);
        let away =
            plainly("could not reach the endpoint: refused", true);
        assert!(away.contains("Could not reach it"));
    }
}
