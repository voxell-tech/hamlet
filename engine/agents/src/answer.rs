//! The requests of plugins: each checked against what the person
//! allowed, asked about on its first use, and answered.

use bevy::app::{App, PostUpdate};
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::system::{Commands, Local, Query, Res};
use bevy::ecs::world::World;
use hamlet_core::agents;
use hamlet_core::catalogue::ModelCatalogue;
use hamlet_core::core::{
    self, HomeDir, ListModels, SaveSettings, Settings, SignIn,
    TestConnection, Tokio, TryModel,
};
use hamlet_core::oversight::grants::Grants as Held;
use hamlet_core::oversight::{Revoke, Veto};
use hamlet_core::session::{SessionKey, Transcript, UserMessage};
use hamlet_engine_api::agents::{
    AgentsReply, AgentsRequest, LOCAL, Recipient, SignInTo, Stage,
};
use hamlet_engine_api::{Failure, Request};
use hamlet_engine_grants::{
    Decided, FirstUse, Grants, Verdict, Waiting,
};
use hamlet_engine_plugins::{Asked, Deliver, PluginInfo};
use hamlet_home::charter;
use hamlet_home::limits::{self, Permission};
use hamlet_model::catalogue::{self, Catalogue, Listed, Priced};
use hamlet_model::chatgpt;
use hamlet_model::config::Config;
use hamlet_model::config::providers::{LOCAL as SERVERS, served};
use hamlet_rules::hamlet::HOME;

use crate::archive::ArchiveWorldExt;
use crate::conversation::ConversationWorldExt;
use crate::focus::FocusWorldExt;
use crate::model_choice::ModelChoiceWorldExt;
use crate::needs::NeedsWorldExt;
use crate::roster::RosterWorldExt;
use crate::setup::{AgentsConfigExt, SetupState};
use crate::watch::Watchers;
use crate::{CHANNEL, Chief, Unavailable, views};

/// The most a person's own price of a model may be, in dollars per
/// million tokens.
const MOST_PRICE: f64 = 10_000.0;

/// The most turns between two reflections.
const MOST_REFLECT: u32 = 1000;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, take);
}

/// Takes the agents requests that may go on, asks the person about
/// those used for the first time, and refuses the rest.
pub(crate) fn take(
    mut asked: MessageReader<Asked>,
    mut decided: MessageReader<Decided>,
    plugins: Query<&PluginInfo>,
    grants: Res<Grants>,
    unavailable: Option<Res<Unavailable>>,
    mut parked: Local<Waiting<Asked>>,
    mut deliver: MessageWriter<Deliver>,
    mut first_use: MessageWriter<FirstUse>,
    mut commands: Commands,
) {
    parked.keep(|plugin| plugins.contains(plugin));
    let refused = |request: &Asked, capability: &str| {
        request.reply(Err(Failure::Refused(format!(
            "{capability} {LOCAL} is not allowed"
        ))))
    };
    let mut ready = Vec::new();
    for (waiting, allowed) in parked.settle(decided.read()) {
        if allowed {
            ready.push(waiting);
        } else if let Request::Agents(request) = &waiting.request {
            let name = request.capability().name();
            deliver.write(refused(&waiting, name));
        }
    }
    for request in asked.read() {
        let Request::Agents(agents) = &request.request else {
            continue;
        };
        let Ok(plugin) = plugins.get(request.plugin) else {
            continue;
        };
        let capability = agents.capability();
        match grants.verdict(plugin, capability, LOCAL) {
            Verdict::Allowed => ready.push(request.clone()),
            Verdict::Asking => parked.park(
                request.plugin,
                capability,
                LOCAL.into(),
                request.clone(),
                &mut first_use,
            ),
            Verdict::Refused => {
                deliver.write(refused(request, capability.name()));
            }
        }
    }
    for request in ready {
        if let Some(why) = &unavailable {
            deliver.write(request.reply(Err(Failure::Failed(
                format!("the agents cannot run: {}", why.0),
            ))));
            continue;
        }
        commands.queue(move |world: &mut World| {
            world.answer_agents(request)
        });
    }
}

/// The agents service's methods on [`World`].
pub trait AgentsWorldExt {
    /// Answers the agents request `asked`, which may go on.
    fn answer_agents(&mut self, asked: Asked);

    /// What the request `request` of the plugin `asked` comes to.
    fn agents_reply(
        &mut self,
        asked: &Asked,
        request: AgentsRequest,
    ) -> Result<AgentsReply, Failure>;

    /// Saves the settings as `edit` leaves them, and switches the
    /// model.
    fn edit_settings(
        &mut self,
        edit: impl FnOnce(&mut Config),
    ) -> Result<AgentsReply, Failure>;

    fn remove_endpoint(
        &mut self,
        name: &str,
    ) -> Result<AgentsReply, Failure>;

    /// The endpoint `name` of the settings, with its key.
    fn endpoint_named(
        &self,
        name: &str,
    ) -> Result<hamlet_model::config::Endpoint, Failure>;

    /// Changes the model `name` of the catalogue as `edit` does, and
    /// writes it.
    fn edit_model(
        &mut self,
        name: &str,
        edit: impl FnOnce(&mut catalogue::Model) -> Result<(), Failure>,
    ) -> Result<AgentsReply, Failure>;

    /// The chief's session with `to`.
    fn chief_key(&mut self, to: &Recipient) -> SessionKey;

    /// Asks `endpoint` for its models, as the setup's, said to be
    /// `from`. The answer lands in the core's link.
    fn ask_endpoint(
        &mut self,
        endpoint: hamlet_model::config::Endpoint,
        from: &str,
    );

    /// Saves `config`, whose default model answered, allows that
    /// model, and ends the setup.
    fn keep_model(&mut self, config: Config);

    /// Puts the place that answered among the endpoints and lists
    /// what it serves, for a setup that adds one.
    fn add_found(&mut self);
}

impl AgentsWorldExt for World {
    fn answer_agents(&mut self, asked: Asked) {
        let Request::Agents(request) = asked.request.clone() else {
            return;
        };
        let reply = self
            .agents_reply(&asked, request)
            .map(hamlet_engine_api::Reply::Agents);
        self.write_message(asked.reply(reply));
    }

    fn agents_reply(
        &mut self,
        asked: &Asked,
        request: AgentsRequest,
    ) -> Result<AgentsReply, Failure> {
        let home = self.resource::<HomeDir>().0.root.clone();
        let done = Ok(AgentsReply::Done);
        let failed = |why: &dyn std::fmt::Display| {
            Failure::Failed(format!("{why:#}"))
        };
        match request {
            AgentsRequest::Watch => {
                self.resource_mut::<Watchers>()
                    .watch(asked.plugin, asked.ticket);
                Ok(AgentsReply::Watching)
            }
            AgentsRequest::Settings => Ok(AgentsReply::Settings(
                views::settings(self, &home),
            )),
            AgentsRequest::SetModel(model) => {
                let saved = self
                    .edit_settings(|config| config.model = model)?;
                let config =
                    self.resource::<Settings>().config.clone();
                self.trigger(TestConnection(config));
                Ok(saved)
            }
            AgentsRequest::SetReflectEvery(turns) => {
                let turns = turns.min(MOST_REFLECT) as usize;
                self.edit_settings(|config| {
                    config.reflect_every = turns
                })
            }
            AgentsRequest::SetAllowNet(on) => {
                self.edit_settings(|config| config.allow_net = on)
            }
            AgentsRequest::RemoveEndpoint(name) => {
                self.remove_endpoint(&name)
            }
            AgentsRequest::SetKey { endpoint, key } => {
                let key = key.trim().to_string();
                if key.is_empty() {
                    return Err(Failure::Refused(
                        "the key is empty".into(),
                    ));
                }
                self.endpoint_named(&endpoint)?;
                self.edit_settings(|config| {
                    for at in &mut config.endpoints {
                        if at.name == endpoint {
                            at.api_key = Some(key.clone());
                        }
                    }
                })
            }
            AgentsRequest::ForgetKey(endpoint) => {
                self.endpoint_named(&endpoint)?;
                self.edit_settings(|config| {
                    for at in &mut config.endpoints {
                        if at.name == endpoint {
                            at.api_key = None;
                        }
                    }
                })
            }
            AgentsRequest::OpenUsage => {
                core::browse(chatgpt::USAGE)
                    .map_err(|why| failed(&why))?;
                done
            }
            AgentsRequest::TestConnection => {
                let config =
                    self.resource::<Settings>().config.clone();
                self.trigger(TestConnection(config));
                done
            }
            AgentsRequest::ListModels => {
                self.trigger(ListModels);
                done
            }
            AgentsRequest::Catalogue => {
                Ok(AgentsReply::Catalogue(views::catalogue(
                    &self.resource::<ModelCatalogue>().catalogue,
                )))
            }
            AgentsRequest::AllowModel { model, allowed } => self
                .edit_model(&model, |model| {
                    model.allowed = allowed;
                    Ok(())
                }),
            AgentsRequest::PriceModel {
                model,
                input,
                output,
            } => {
                let fits = |price: f64| {
                    price.is_finite()
                        && (0.0..=MOST_PRICE).contains(&price)
                };
                if !fits(input) || !fits(output) {
                    return Err(Failure::Refused(format!(
                        "a price is from 0 to {MOST_PRICE}"
                    )));
                }
                self.edit_model(&model, |model| {
                    if !matches!(
                        model.priced,
                        Priced::Unknown | Priced::Yours
                    ) {
                        return Err(Failure::Refused(
                            "its price is the endpoint's or the \
                             home's"
                                .into(),
                        ));
                    }
                    model.input = input;
                    model.output = output;
                    model.priced = Priced::Yours;
                    Ok(())
                })
            }
            AgentsRequest::TryModel(model) => {
                let mut config =
                    self.resource::<Settings>().config.clone();
                config.model = model;
                self.trigger(TryModel(config));
                done
            }
            AgentsRequest::Providers => {
                Ok(AgentsReply::Providers(views::providers()))
            }
            AgentsRequest::Setup => Ok(AgentsReply::Setup(
                self.resource::<SetupState>().view(),
            )),
            AgentsRequest::BeginSetup { adding } => {
                *self.resource_mut::<SetupState>() = SetupState {
                    begun: true,
                    adding,
                    ..SetupState::default()
                };
                done
            }
            AgentsRequest::Connect { provider, url, key } => {
                match SetupState::endpoint(
                    provider.as_deref(),
                    &url,
                    &key,
                ) {
                    Ok((endpoint, from)) => {
                        self.ask_endpoint(endpoint, from);
                        done
                    }
                    Err(why) => {
                        self.resource_mut::<SetupState>().problem =
                            Some(why.clone());
                        Err(Failure::Refused(why))
                    }
                }
            }
            AgentsRequest::SignIn(to) => {
                let mut setup = self.resource_mut::<SetupState>();
                setup.stage = Stage::SigningIn(to);
                setup.signing = Some(to);
                setup.problem = None;
                setup.models.clear();
                self.trigger(match to {
                    SignInTo::OpenRouter => SignIn::OpenRouter,
                    SignInTo::ChatGpt => SignIn::ChatGpt,
                });
                done
            }
            AgentsRequest::FindLocal => {
                let mut setup = self.resource_mut::<SetupState>();
                setup.stage = Stage::Looking;
                setup.local = 0;
                setup.problem = None;
                setup.models.clear();
                self.ask_endpoint(
                    served(&SERVERS[0]),
                    SERVERS[0].name,
                );
                done
            }
            AgentsRequest::PickModel(id) => {
                let Some(endpoint) =
                    self.resource::<SetupState>().asked.clone()
                else {
                    return Err(Failure::Refused(
                        "no place has answered yet".into(),
                    ));
                };
                let mut config =
                    self.resource::<Settings>().config.clone();
                // A home that was never set up has only the endpoint
                // it was born with, which nobody chose.
                if !Config::saved(&home) {
                    config.endpoints.clear();
                }
                let name = config.settle(endpoint);
                config.model = catalogue::named(&name, &id);
                let mut setup = self.resource_mut::<SetupState>();
                setup.trying = Some(config.clone());
                setup.stage = Stage::Trying;
                setup.problem = None;
                self.trigger(TryModel(config));
                done
            }
            AgentsRequest::EndSetup => {
                *self.resource_mut::<SetupState>() =
                    SetupState::default();
                done
            }
            AgentsRequest::Charter => {
                Ok(AgentsReply::Charter(views::charter(&home)))
            }
            AgentsRequest::EditCharter => {
                let file = charter::adopt(&home)
                    .map_err(|why| failed(&why))?;
                core::browse(&file.to_string_lossy())
                    .map_err(|why| failed(&why))?;
                done
            }
            AgentsRequest::ResetCharter => {
                charter::reset(&home).map_err(|why| failed(&why))?;
                done
            }
            AgentsRequest::Limits => {
                Ok(AgentsReply::Limits(views::limits(&home)))
            }
            AgentsRequest::SetLimit {
                section,
                key,
                value,
            } => {
                if !views::of_the_realm(&section, &key) {
                    return Err(Failure::Refused(format!(
                        "{section}.{key} is not a limit of the realm"
                    )));
                }
                limits::set(&home, &section, &key, value.as_deref())
                    .map_err(|why| failed(&why))?;
                done
            }
            AgentsRequest::Grants => Ok(AgentsReply::Grants(
                views::grants(self.resource::<Held>()),
            )),
            AgentsRequest::Revoke { agent, permission } => {
                let Some(permission) = Permission::named(&permission)
                else {
                    return Err(Failure::NotFound);
                };
                let chief = self.resource::<Chief>().name.clone();
                self.trigger(Revoke {
                    agent,
                    permission,
                    chief,
                });
                done
            }
            AgentsRequest::Status => views::status(self)
                .map(AgentsReply::Status)
                .ok_or(Failure::NotFound),
            AgentsRequest::Send { to, text } => {
                let key = self.chief_key(&to);
                let user = self.resource::<Chief>().name.clone();
                self.trigger(UserMessage { key, user, text });
                done
            }
            AgentsRequest::Veto(agent) => {
                let chief = self.resource::<Chief>().name.clone();
                self.trigger(Veto { agent, chief });
                done
            }
            AgentsRequest::Sessions => {
                Ok(AgentsReply::Sessions(views::sessions(self)))
            }
            AgentsRequest::Open(to) => self
                .open(to)
                .map(AgentsReply::Session)
                .ok_or(Failure::NotFound),
            AgentsRequest::Messages {
                session,
                after,
                most,
            } => {
                let key = views::session_key(&session);
                let said = self
                    .query::<(&SessionKey, &Transcript)>()
                    .iter(self)
                    .find(|(at, _)| **at == key)
                    .map(|(_, said)| {
                        views::messages(said, after, most)
                    });
                said.map(AgentsReply::Messages)
                    .ok_or(Failure::NotFound)
            }
            AgentsRequest::Focus => self
                .focus()
                .map(AgentsReply::Focus)
                .ok_or(Failure::NotFound),
            AgentsRequest::Select(thing) => {
                self.select(thing);
                done
            }
            AgentsRequest::Related => {
                Ok(AgentsReply::Related(self.related()))
            }
            AgentsRequest::Rewind { session, entry } => {
                self.rewind(&session, entry).and(done)
            }
            AgentsRequest::Decree { hamlet, text } => {
                self.decree(&hamlet, text);
                done
            }
            AgentsRequest::Papers => {
                Ok(AgentsReply::Papers(self.papers()))
            }
            AgentsRequest::Prompt => {
                self.prompt().map(AgentsReply::Prompt)
            }
            AgentsRequest::EditCharacter => {
                self.edit_character().and(done)
            }
            AgentsRequest::Skills => {
                Ok(AgentsReply::Skills(self.skills()))
            }
            AgentsRequest::Pasts { query } => {
                Ok(AgentsReply::Pasts(self.pasts(&query)))
            }
            AgentsRequest::ReadPast(name) => {
                self.read_past(&name).map(AgentsReply::Messages)
            }
            AgentsRequest::DeletePast(name) => {
                self.delete_past(&name).and(done)
            }
            AgentsRequest::Roster => {
                Ok(AgentsReply::Roster(self.roster()))
            }
            AgentsRequest::Record(hamlet) => {
                self.record(&hamlet).map(AgentsReply::Record)
            }
            AgentsRequest::Bylaws(hamlet) => {
                self.hamlet_root(&hamlet).map(|root| {
                    AgentsReply::Bylaws(views::bylaws(&root))
                })
            }
            AgentsRequest::SetBylaw {
                hamlet,
                section,
                key,
                value,
            } => {
                if views::of_the_realm(&section, &key) {
                    return Err(Failure::Refused(format!(
                        "{section}.{key} is a limit of the realm"
                    )));
                }
                let root = self.hamlet_root(&hamlet)?;
                limits::set(&root, &section, &key, value.as_deref())
                    .map_err(|why| failed(&why))?;
                done
            }
            AgentsRequest::Needs => {
                Ok(AgentsReply::Needs(self.needs()))
            }
            AgentsRequest::Answer { need, choice, text } => {
                self.answer(need, choice, text).and(done)
            }
            AgentsRequest::Pick {
                need,
                question,
                pick,
            } => self.pick(need, question, pick).and(done),
            AgentsRequest::Turn { need, to } => {
                self.turn(need, to).and(done)
            }
            AgentsRequest::Submit(need) => {
                self.submit(need).and(done)
            }
            AgentsRequest::RunModel { agent, model } => {
                self.run_model(&agent, &model).and(done)
            }
        }
    }

    fn edit_settings(
        &mut self,
        edit: impl FnOnce(&mut Config),
    ) -> Result<AgentsReply, Failure> {
        let mut config = self.resource::<Settings>().config.clone();
        edit(&mut config);
        self.trigger(SaveSettings(config));
        match self.resource::<Settings>().error.clone() {
            Some(why) => Err(Failure::Failed(why)),
            None => Ok(AgentsReply::Done),
        }
    }

    fn remove_endpoint(
        &mut self,
        name: &str,
    ) -> Result<AgentsReply, Failure> {
        let home = self.resource::<HomeDir>().0.root.clone();
        let signed_in = self.endpoint_named(name)?.chatgpt;
        let done = self.edit_settings(|config| {
            config.endpoints.retain(|endpoint| endpoint.name != name)
        })?;
        Config::forget_key(&home, name);
        if signed_in {
            // Signed out here even when OpenAI cannot be told.
            self.resource::<Tokio>().0.spawn(async move {
                let _ = chatgpt::sign_out(&home).await;
            });
        }
        Ok(done)
    }

    fn endpoint_named(
        &self,
        name: &str,
    ) -> Result<hamlet_model::config::Endpoint, Failure> {
        self.resource::<Settings>()
            .config
            .endpoint(name)
            .cloned()
            .ok_or(Failure::NotFound)
    }

    fn edit_model(
        &mut self,
        name: &str,
        edit: impl FnOnce(&mut catalogue::Model) -> Result<(), Failure>,
    ) -> Result<AgentsReply, Failure> {
        let home = self.resource::<HomeDir>().0.root.clone();
        let mut models = self.resource_mut::<ModelCatalogue>();
        let model = models
            .catalogue
            .get_mut(name)
            .ok_or(Failure::NotFound)?;
        edit(model)?;
        let saved = models.catalogue.save(&home);
        models.reread();
        saved
            .map(|()| AgentsReply::Done)
            .map_err(|why| Failure::Failed(format!("{why:#}")))
    }

    fn chief_key(&mut self, to: &Recipient) -> SessionKey {
        let chief = self.resource::<Chief>().name.clone();
        match to {
            Recipient::Agent(name) => {
                // A mayor is talked to by its office.
                let office = agents::named(self, name)
                    .filter(|agent| {
                        self.get::<agents::Mayor>(*agent).is_some()
                    })
                    .map(|agent| {
                        agents::hamlet_of(self, agent)
                            .unwrap_or(HOME)
                            .to_string()
                    });
                match office {
                    Some(hamlet) => SessionKey::with_mayor(
                        CHANNEL, &chief, &hamlet,
                    ),
                    None => {
                        SessionKey::with_agent(CHANNEL, &chief, name)
                    }
                }
            }
            Recipient::Mayor(hamlet) => {
                SessionKey::with_mayor(CHANNEL, &chief, hamlet)
            }
        }
    }

    fn ask_endpoint(
        &mut self,
        endpoint: hamlet_model::config::Endpoint,
        from: &str,
    ) {
        let test = TestConnection(Config {
            model: catalogue::named(&endpoint.name, ""),
            endpoints: vec![endpoint.clone()],
            ..Config::default()
        });
        let mut setup = self.resource_mut::<SetupState>();
        if setup.stage != Stage::Looking {
            setup.stage = Stage::Asking;
        }
        setup.asked = Some(endpoint);
        setup.from = from.to_string();
        setup.problem = None;
        self.trigger(test);
    }

    fn keep_model(&mut self, config: Config) {
        let home = self.resource::<HomeDir>().0.root.clone();
        self.trigger(SaveSettings(config.clone()));
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
            self.resource::<Settings>().error.clone().or_else(|| {
                all.save(&home).err().map(|why| format!("{why:#}"))
            });
        if let Some(why) = failed {
            let mut setup = self.resource_mut::<SetupState>();
            setup.stage = Stage::Found;
            setup.problem = Some(format!(
                "Your choice could not be saved. ({why})"
            ));
            return;
        }
        self.resource_mut::<ModelCatalogue>().reread();
        // The rest of what the endpoint serves, with its prices.
        self.trigger(ListModels);
        self.trigger(TestConnection(config));
        *self.resource_mut::<SetupState>() = SetupState {
            stage: Stage::Done,
            ..SetupState::default()
        };
    }

    fn add_found(&mut self) {
        let Some(endpoint) =
            self.resource::<SetupState>().asked.clone()
        else {
            return;
        };
        let mut config = self.resource::<Settings>().config.clone();
        config.settle(endpoint);
        self.trigger(SaveSettings(config));
        if let Some(why) = self.resource::<Settings>().error.clone() {
            self.resource_mut::<SetupState>().problem =
                Some(format!("It could not be saved. ({why})"));
            return;
        }
        self.trigger(ListModels);
        *self.resource_mut::<SetupState>() = SetupState {
            adding: true,
            stage: Stage::Done,
            ..SetupState::default()
        };
    }
}
