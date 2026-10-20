//! Setting up a place for models: a provider's key, a sign-in, or a
//! server on this computer, then a model of it that answers. The
//! endpoint being set up, its key with it, stays here.

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Res, ResMut};
use bevy::ecs::world::World;
use hamlet_core::core::{Link, Signing, Trial};
use hamlet_engine_api::agents::{SignInTo, Stage};
use hamlet_model::chatgpt;
use hamlet_model::config::providers::{
    LOCAL, OPENROUTER, PROVIDERS, served,
};
use hamlet_model::config::{Config, Endpoint};

use crate::AgentsWorldExt;

const NOTHING_LOCAL: &str = "No model server answered here. Install \
                             Ollama from ollama.com, download a \
                             model, then choose Look Again.";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<SetupState>()
        .add_systems(Update, follow);
}

/// The setup under way.
#[derive(Resource)]
pub(crate) struct SetupState {
    pub begun: bool,
    pub adding: bool,
    pub stage: Stage,
    pub from: String,
    pub models: Vec<String>,
    pub problem: Option<String>,
    /// The endpoint asked last, with its key.
    pub asked: Option<Endpoint>,
    /// The settings whose default model is asked for a word.
    pub trying: Option<Config>,
    /// The sign-in awaited.
    pub signing: Option<SignInTo>,
    /// The server on this computer being tried, by its place in
    /// [`LOCAL`].
    pub local: usize,
}

impl Default for SetupState {
    fn default() -> Self {
        Self {
            begun: false,
            adding: false,
            stage: Stage::Idle,
            from: String::new(),
            models: Vec::new(),
            problem: None,
            asked: None,
            trying: None,
            signing: None,
            local: 0,
        }
    }
}

impl SetupState {
    /// It as the protocol tells it, with no key.
    pub(crate) fn view(&self) -> hamlet_engine_api::agents::Setup {
        hamlet_engine_api::agents::Setup {
            begun: self.begun,
            adding: self.adding,
            stage: self.stage,
            from: self.from.clone(),
            plan: self
                .asked
                .as_ref()
                .is_some_and(|asked| asked.chatgpt),
            models: self.models.clone(),
            problem: self.problem.clone(),
        }
    }

    /// The endpoint of a provider `provider` names, or of the address
    /// `url`, with `key`, and who it is; or why it cannot be asked.
    pub(crate) fn endpoint(
        provider: Option<&str>,
        url: &str,
        key: &str,
    ) -> Result<(Endpoint, &'static str), String> {
        let known = match provider {
            Some(name) => Some(
                PROVIDERS
                    .iter()
                    .find(|known| known.endpoint == name)
                    .ok_or_else(|| {
                        format!("{name} is no provider")
                    })?,
            ),
            None => None,
        };
        let (key, url) = (key.trim(), url.trim());
        let base_url = known.map_or(url, |known| known.base_url);
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
            return Err(why.into());
        }
        let endpoint = Endpoint {
            name: known
                .map_or(
                    hamlet_model::config::providers::OTHER,
                    |known| known.endpoint,
                )
                .into(),
            base_url: base_url.into(),
            chatgpt: false,
            api_key: (!key.is_empty()).then(|| key.to_string()),
        };
        Ok((endpoint, known.map_or("It", |known| known.name)))
    }
}

/// The methods of [`Config`] for a setup.
pub trait AgentsConfigExt {
    /// Puts `endpoint` among the endpoints and returns the name it
    /// has there. One at the same address, signed in to the same way,
    /// is given the key and keeps its name, and a name another has is
    /// numbered.
    fn settle(&mut self, endpoint: Endpoint) -> String;
}

impl AgentsConfigExt for Config {
    fn settle(&mut self, endpoint: Endpoint) -> String {
        let at = |url: &str| url.trim_end_matches('/').to_string();
        let same = self.endpoints.iter_mut().find(|known| {
            at(&known.base_url) == at(&endpoint.base_url)
                && known.chatgpt == endpoint.chatgpt
        });
        if let Some(known) = same {
            known.api_key = endpoint.api_key;
            return known.name.clone();
        }
        let name = std::iter::once(endpoint.name.clone())
            .chain((2..).map(|n| format!("{}-{n}", endpoint.name)))
            .find(|name| self.endpoint(name).is_none())
            .unwrap_or_default();
        self.endpoints.push(Endpoint {
            name: name.clone(),
            ..endpoint
        });
        name
    }
}

/// The reason an endpoint did not list its models, or a model of it
/// did not answer (`tried`), in plain words with what to do next.
pub(crate) fn plainly(why: &str, tried: bool) -> String {
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

/// Moves the setup on as a sign-in, a test of a place or a trial of
/// a model comes to an end.
fn follow(
    link: Res<Link>,
    trial: Res<Trial>,
    signing: Res<Signing>,
    mut setup: ResMut<SetupState>,
    mut commands: Commands,
) {
    if let Some(to) = setup.signing {
        match (&*signing, to) {
            (Signing::Idle | Signing::Waiting, _) => {}
            // The key it gave is used as one that was typed.
            (Signing::Done(key), SignInTo::OpenRouter) => {
                setup.signing = None;
                let (name, endpoint) = (
                    PROVIDERS[OPENROUTER].name,
                    Endpoint {
                        name: PROVIDERS[OPENROUTER].endpoint.into(),
                        base_url: PROVIDERS[OPENROUTER]
                            .base_url
                            .into(),
                        chatgpt: false,
                        api_key: Some(key.clone()),
                    },
                );
                commands.queue(move |world: &mut World| {
                    world.ask_endpoint(endpoint, name)
                });
            }
            (Signing::Done(_), SignInTo::ChatGpt) => {
                setup.signing = None;
                commands.queue(|world: &mut World| {
                    world.ask_endpoint(chatgpt::endpoint(), "ChatGPT")
                });
            }
            (Signing::Failed(why), SignInTo::OpenRouter) => {
                setup.signing = None;
                setup.stage = Stage::Idle;
                setup.problem = Some(format!(
                    "Signing in did not finish ({why}). Try again, \
                     or make a key and paste it here."
                ));
            }
            (Signing::Failed(why), SignInTo::ChatGpt) => {
                setup.signing = None;
                setup.stage = Stage::Idle;
                setup.problem = Some(format!(
                    "Signing in did not finish ({why}). Go back and \
                     try again."
                ));
            }
        }
        return;
    }
    if !matches!(
        setup.stage,
        Stage::Asking | Stage::Looking | Stage::Trying
    ) {
        return;
    }
    if let Some(config) = setup.trying.clone() {
        match &*trial {
            Trial::Untried | Trial::Trying => {}
            Trial::Answered => {
                setup.trying = None;
                commands.queue(move |world: &mut World| {
                    world.keep_model(config)
                });
            }
            Trial::Failed(why) => {
                setup.problem = Some(plainly(why, true));
                setup.trying = None;
                setup.stage = Stage::Found;
            }
        }
        return;
    }
    let found = match &*link {
        Link::Unknown | Link::Testing => return,
        Link::Ok { models } => Ok(models.clone()),
        Link::Failed(why) => Err(why.clone()),
    };
    let looking = setup.stage == Stage::Looking;
    if let Ok(models) = &found
        && !models.is_empty()
    {
        setup.problem = None;
        setup.stage = Stage::Found;
        if setup.adding {
            commands.queue(|world: &mut World| world.add_found());
            return;
        }
        setup.models.clone_from(models);
        return;
    }
    if !looking {
        setup.stage = Stage::Idle;
        setup.problem = Some(match found {
            Ok(_) => "It answered, but it has no models for this \
                      key."
                .into(),
            Err(why) => plainly(&why, false),
        });
        return;
    }
    if found.is_ok() && setup.problem.is_none() {
        let name = LOCAL[setup.local].name;
        setup.problem = Some(format!(
            "{name} is running but has no model. Download one, then \
             choose Look Again."
        ));
    }
    setup.local += 1;
    match LOCAL.get(setup.local) {
        Some(next) => {
            let (endpoint, name) = (served(next), next.name);
            commands.queue(move |world: &mut World| {
                world.ask_endpoint(endpoint, name)
            });
        }
        None => {
            setup.stage = Stage::Idle;
            if setup.problem.is_none() {
                setup.problem = Some(NOTHING_LOCAL.into());
            }
        }
    }
}
