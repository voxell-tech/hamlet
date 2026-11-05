//! The chief's model catalogue, what the server asks of a runtime's
//! providers, and sign-ins owned by the connection that began them
//! (S7).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_protocol::client::{
    Check, EditCatalogueSet, LinkState, Listed, ListedPriced, Login,
    LoginMethod, LoginProgress, LoginStep, Price as ListedPrice,
    ProviderView, Read, ReadResult, RequestId, ServerMessage,
};
use hamlet_protocol::runtime::{
    self, ModelRef, Price, ProviderControl, ProviderEvent,
    ProviderRequest, ProviderRequestAction, ProviderRequestMethod,
    ProviderResult, ProviderResultProviders, Selection,
};
use hamlet_store::Db;
use hamlet_store::schema::document;
use serde_json::Value;

use crate::clients::check::reply;
use crate::core::Core;
use crate::credentials::write_whole;
use crate::layout::Layout;
use crate::settings::Settings;
use crate::society::refusal::Refusal;
use crate::talk::runtimes::Phase;

/// The price in dollars per million tokens of a model nothing prices.
const RATE: f64 = 1.0;
/// [`RATE`] for a model served on this machine.
const LOCAL_RATE: f64 = 0.1;
/// The provider of the faux models a faux realm runs.
pub(crate) const FAUX: &str = "faux";

pub(crate) struct Providers {
    file: PathBuf,
    /// Each endpoint's models as its provider lists them.
    lists: BTreeMap<String, Vec<Value>>,
    providers: Vec<ProviderResultProviders>,
    pending: HashMap<String, Pending>,
    logins: HashMap<String, SignIn>,
    /// Requests not yet sent to a runtime.
    outgoing: Vec<ProviderRequest>,
    controls: Vec<ProviderControl>,
    faux: bool,
}

struct Pending {
    /// The connection and request it answers; unset when the server
    /// asked.
    from: Option<(u64, RequestId)>,
    provider: String,
    action: ProviderRequestAction,
    login: Option<String>,
    /// Whether it answers a read of the catalogue.
    read: bool,
    expires: Option<Instant>,
}

/// A sign-in in progress, owned by the connection that began it.
struct SignIn {
    client: u64,
    id: RequestId,
    scope: String,
    prompt: Option<String>,
}

impl Pending {
    fn client(&self) -> Option<u64> {
        self.from.as_ref().map(|(client, _)| *client)
    }
}

impl Providers {
    pub fn open(layout: &Layout, faux: bool) -> Result<Self> {
        let file = layout.models();
        let lists = match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .context("reading models.json")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                BTreeMap::new()
            }
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            file,
            lists,
            providers: Vec::new(),
            pending: HashMap::new(),
            logins: HashMap::new(),
            outgoing: Vec::new(),
            controls: Vec::new(),
            faux,
        })
    }

    /// Ends what the connection `client` began.
    pub fn disconnect(&mut self, client: u64) {
        for (scope, _) in self
            .pending
            .iter()
            .filter(|(_, s)| s.client() == Some(client))
        {
            self.controls.push(control(scope, None, None));
        }
        self.logins.retain(|_, s| s.client != client);
        self.pending.retain(|_, s| s.client() != Some(client));
    }

    /// Ends every sign-in and request, as the server closes.
    pub fn close_all(&mut self) {
        for scope in self.pending.keys() {
            self.controls.push(control(scope, None, None));
        }
        self.pending.clear();
        self.logins.clear();
        self.outgoing.clear();
    }

    /// The soonest a request times out.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.pending.values().filter_map(|p| p.expires).min()
    }

    /// The results of the requests that timed out.
    pub fn expired(&self) -> Vec<ProviderResult> {
        self.pending
            .iter()
            .filter(|(_, p)| {
                p.expires.is_some_and(|end| Instant::now() >= end)
            })
            .map(|(key, _)| ProviderResult {
                v: runtime::VERSION,
                id: "timeout".into(),
                re: key.clone(),
                error: Some(
                    "The provider did not answer in time.".into(),
                ),
                credential: None,
                models: None,
                providers: None,
                text: None,
            })
            .collect()
    }

    /// What `model` of `provider` is listed as, or null.
    fn listed(&self, provider: &str, model: &str) -> Value {
        self.lists
            .get(provider)
            .and_then(|list| list.iter().find(|m| m["id"] == model))
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// The model call of `model` as it is made: its address, its size
    /// and its price.
    pub fn select(
        &self,
        db: &mut Db,
        settings: &Settings,
        model: ModelRef,
        effort: Option<crate::society::hires::Effort>,
    ) -> Result<Selection> {
        let listed = self.listed(&model.provider, &model.id);
        let base_url = model.base_url.clone().or_else(|| {
            settings
                .base_url(&model.provider)
                .map(String::from)
                .or_else(|| {
                    listed["baseUrl"].as_str().map(String::from)
                })
        });
        let size =
            |key: &str| listed[key].as_u64().filter(|n| *n > 0);
        let name = format!("{}::{}", model.provider, model.id);
        let cards = catalogue(db)?;
        let price = if model.provider == FAUX {
            Price {
                input: 0.0,
                output: 0.0,
                cache_read: 0.0,
                cache_write: 0.0,
            }
        } else {
            match cards
                .get(&name)
                .and_then(|card| card.price.as_ref())
            {
                Some(&ListedPrice { input, output }) => Price {
                    input,
                    output,
                    cache_read: input,
                    cache_write: input,
                },
                None => {
                    serde_json::from_value(listed["cost"].clone())
                        .unwrap_or_else(|_| {
                            let rate =
                                if is_local(base_url.as_deref()) {
                                    LOCAL_RATE
                                } else {
                                    RATE
                                };
                            Price {
                                input: rate,
                                output: rate,
                                cache_read: rate,
                                cache_write: rate,
                            }
                        })
                }
            }
        };
        Ok(Selection {
            context_window: size("contextWindow"),
            max_tokens: size("maxTokens"),
            effort: effort.map(crate::society::hires::Effort::wire),
            model: ModelRef { base_url, ..model },
            price,
        })
    }
}

impl Core {
    /// Asks the providers what they offer, and each endpoint for its
    /// models.
    pub(crate) fn start_providers(&mut self) -> Result<()> {
        self.ask_provider(
            None,
            "",
            ProviderRequestAction::Describe,
            None,
            None,
            false,
        )?;
        if !self.providers.faux {
            let endpoints = self.env.settings.endpoints.clone();
            for endpoint in &endpoints {
                self.ask_provider(
                    None,
                    &endpoint.name,
                    ProviderRequestAction::Refresh,
                    None,
                    None,
                    false,
                )?;
            }
        }
        Ok(())
    }

    fn ask_provider(
        &mut self,
        from: Option<(u64, &RequestId)>,
        provider: &str,
        action: ProviderRequestAction,
        model: Option<String>,
        login: Option<String>,
        read: bool,
    ) -> Result<String> {
        let settings = &self.env.settings;
        let serving = settings.provider().unwrap_or_default();
        let base_url = settings.base_url(provider).map(String::from);
        let model = model.or_else(|| {
            settings
                .model
                .split_once("::")
                .filter(|(p, _)| *p == provider)
                .map(|(_, m)| m.into())
        });
        let credential = self
            .credentials
            .model(provider, serving)
            .map(crate::credentials::pack)
            .transpose()?;
        let key = format!("provider:{}", uuid::Uuid::new_v4());
        let expires = match action {
            ProviderRequestAction::Describe
            | ProviderRequestAction::Refresh => Some(12),
            ProviderRequestAction::Hello => Some(95),
            ProviderRequestAction::Login => None,
        }
        .map(|seconds| Instant::now() + Duration::from_secs(seconds));
        self.providers.pending.insert(
            key.clone(),
            Pending {
                from: from.map(|(client, id)| (client, id.clone())),
                provider: provider.into(),
                action: action.clone(),
                login,
                read,
                expires,
            },
        );
        let models = self
            .providers
            .lists
            .get(provider)
            .into_iter()
            .flatten()
            .filter_map(|m| m.as_object().cloned())
            .map(|m| m.into_iter().collect())
            .collect();
        self.providers.outgoing.push(ProviderRequest {
            v: runtime::VERSION,
            id: key.clone(),
            action,
            provider: provider.into(),
            base_url,
            model,
            credential,
            method: None,
            installation_id: Some(
                self.credentials.installation_id().into(),
            ),
            models,
        });
        Ok(key)
    }

    /// Sends what waits for the providers to the default harness's
    /// runtime once it is ready.
    pub(crate) async fn send_providers(&mut self) -> Result<()> {
        let db = self.realm.store().connection();
        let id = crate::talk::runtimes::of_harness(
            db,
            crate::society::agents::HARNESS,
        )?;
        let Some(index) = self.runtimes.index_of(&id) else {
            return Ok(());
        };
        if self.runtimes.list[index].phase != Phase::Ready {
            return Ok(());
        }
        // A request keeps its own id, which its result answers.
        for request in std::mem::take(&mut self.providers.outgoing) {
            self.leases.grant(index, &request.id, &request.provider);
            self.runtimes
                .send(index, |_, _| {
                    runtime::ServerMessage::ProviderRequest(request)
                })
                .await;
        }
        for control in std::mem::take(&mut self.providers.controls) {
            let indices = (0..self.runtimes.list.len())
                .filter(|at| self.leases.is_open(*at, &control.scope))
                .collect::<Vec<_>>();
            for at in indices {
                if control.cancel {
                    self.leases.finish(at, &control.scope);
                }
                let control = control.clone();
                self.runtimes
                    .send(at, |_, _| {
                        runtime::ServerMessage::ProviderControl(
                            control,
                        )
                    })
                    .await;
            }
        }
        Ok(())
    }

    /// Answers the provider requests that timed out.
    pub(crate) async fn expire_providers(&mut self) -> Result<()> {
        for result in self.providers.expired() {
            self.provider_result(None, result).await?;
        }
        Ok(())
    }

    /// Takes a client's sign-in, check or read of the models. `false`
    /// when `message` is none of them.
    pub(crate) async fn model_request(
        &mut self,
        client: u64,
        message: &hamlet_protocol::client::ClientMessage,
    ) -> Result<bool> {
        use hamlet_protocol::client::ClientMessage;
        let mut answers = Vec::new();
        match message {
            ClientMessage::Check(check) if self.providers.faux => {
                let id = match check {
                    Check::Model { id, .. }
                    | Check::Connection { id } => id,
                };
                let words = "Every agent runs the faux model here.";
                self.model_link(
                    LinkState::Connected,
                    None,
                    Some(words),
                )?;
                self.answer(
                    client,
                    reply(id, Ok::<_, Refusal>(words.into())),
                )?;
                self.publish().await?;
                return Ok(true);
            }
            ClientMessage::Check(check) => {
                let (id, name, action) = match check {
                    Check::Model { id, model } => (
                        id,
                        model.clone(),
                        ProviderRequestAction::Hello,
                    ),
                    Check::Connection { id } => {
                        (id, None, ProviderRequestAction::Refresh)
                    }
                };
                let name = name.unwrap_or_else(|| {
                    self.env.settings.model.clone()
                });
                let (provider, model) = match self
                    .named_model(&name)?
                {
                    Ok(found) => found,
                    Err(refusal) => {
                        self.answer(client, reply(id, Err(refusal)))?;
                        return Ok(true);
                    }
                };
                self.model_link(LinkState::Testing, None, None)?;
                self.ask_provider(
                    Some((client, id)),
                    &provider,
                    action,
                    Some(model),
                    None,
                    false,
                )?;
            }
            ClientMessage::Read(Read::Models { id, refresh }) => {
                if *refresh == Some(true) && !self.providers.faux {
                    let endpoints =
                        self.env.settings.endpoints.clone();
                    for endpoint in &endpoints {
                        self.ask_provider(
                            Some((client, id)),
                            &endpoint.name,
                            ProviderRequestAction::Refresh,
                            None,
                            None,
                            true,
                        )?;
                    }
                    if endpoints.is_empty() {
                        answers.push(self.catalogue_read(id)?);
                    }
                } else {
                    answers.push(self.catalogue_read(id)?);
                }
            }
            ClientMessage::Login(login) => match login {
                Login::Start { id, start } => {
                    let key = uuid::Uuid::new_v4().to_string();
                    let provider = self
                        .provider_views()?
                        .into_iter()
                        .find(|p| p.id == start.provider);
                    let failure = match &provider {
                        Some(provider) => (!provider
                            .methods
                            .contains(&start.method))
                        .then(|| {
                            format!(
                                "Signing in with {} is not offered. \
                                 Use an API key.",
                                start.provider
                            )
                        }),
                        None => Some(format!(
                            "Unknown provider: {}",
                            start.provider
                        )),
                    };
                    if let Some(failed) = failure {
                        answers.push(login_step(
                            id,
                            &key,
                            LoginStep::Failed { failed },
                        ));
                    } else {
                        let scope = self.ask_provider(
                            Some((client, id)),
                            &start.provider,
                            ProviderRequestAction::Login,
                            None,
                            Some(key.clone()),
                            false,
                        )?;
                        let request = self
                            .providers
                            .outgoing
                            .last_mut()
                            .context("the sign-in's request")?;
                        request.method = Some(
                            if start.method == LoginMethod::Key {
                                ProviderRequestMethod::Key
                            } else {
                                ProviderRequestMethod::Subscription
                            },
                        );
                        self.providers.logins.insert(
                            key,
                            SignIn {
                                client,
                                id: id.clone(),
                                scope,
                                prompt: None,
                            },
                        );
                    }
                }
                Login::Reply { id, reply: answer } => {
                    let Some(signin) = self
                        .providers
                        .logins
                        .get_mut(&answer.login)
                        .filter(|s| s.client == client)
                    else {
                        self.answer(
                            client,
                            reply(
                                id,
                                Err(Refusal::Usage {
                                    words: "No sign-in of yours is \
                                            open."
                                        .into(),
                                }),
                            ),
                        )?;
                        return Ok(true);
                    };
                    let Some(prompt) = signin.prompt.take() else {
                        self.answer(
                            client,
                            reply(
                                id,
                                Err(Refusal::Usage {
                                    words: "That sign-in is not \
                                            waiting for an answer."
                                        .into(),
                                }),
                            ),
                        )?;
                        return Ok(true);
                    };
                    let scope = signin.scope.clone();
                    self.credentials.remember(&answer.value);
                    self.providers.controls.push(control(
                        &scope,
                        Some(prompt),
                        Some(answer.value.clone()),
                    ));
                    answers.push(reply(
                        id,
                        Ok::<_, Refusal>(
                            "Sign-in answer received.".into(),
                        ),
                    ));
                }
                Login::Cancel { id, cancel } => {
                    let Some(signin) = self
                        .providers
                        .logins
                        .get(cancel)
                        .filter(|s| s.client == client)
                    else {
                        self.answer(
                            client,
                            reply(
                                id,
                                Err(Refusal::Usage {
                                    words: "No sign-in of yours is \
                                            open."
                                        .into(),
                                }),
                            ),
                        )?;
                        return Ok(true);
                    };
                    answers.push(login_step(
                        &signin.id,
                        cancel,
                        LoginStep::Failed {
                            failed: "Sign-in cancelled.".into(),
                        },
                    ));
                    let scope = signin.scope.clone();
                    self.providers
                        .controls
                        .push(control(&scope, None, None));
                    self.providers.logins.remove(cancel);
                    self.providers.pending.retain(|_, p| {
                        p.login.as_ref() != Some(cancel)
                    });
                    answers.push(reply(
                        id,
                        Ok::<_, Refusal>("Sign-in cancelled.".into()),
                    ));
                }
            },
            _ => return Ok(false),
        }
        for answer in answers {
            self.answer(client, answer)?;
        }
        self.send_providers().await?;
        Ok(true)
    }

    /// A step of a sign-in the runtime of `index` runs.
    pub(crate) fn provider_event(
        &mut self,
        index: usize,
        event: ProviderEvent,
    ) -> Result<()> {
        if !self.leases.is_open(index, &event.scope) {
            return Ok(());
        }
        let Some(pending) = self.providers.pending.get(&event.scope)
        else {
            return Ok(());
        };
        let Some(login) = pending.login.clone() else {
            return Ok(());
        };
        let Some(signin) = self.providers.logins.get_mut(&login)
        else {
            return Ok(());
        };
        if let Some(prompt) = event.prompt {
            signin.prompt = (!prompt.is_empty()).then_some(prompt);
        }
        if event.step.is_empty() {
            return Ok(());
        }
        let (client, id) = (signin.client, signin.id.clone());
        let step =
            serde_json::from_value(serde_json::to_value(event.step)?)
                .context("a sign-in step from the harness")?;
        self.answer(client, login_step(&id, &login, step))
    }

    /// The end of a provider request, from the runtime of `index` or
    /// its timeout.
    pub(crate) async fn provider_result(
        &mut self,
        index: Option<usize>,
        result: ProviderResult,
    ) -> Result<()> {
        if let Some(index) = index {
            self.leases.finish(index, &result.re);
        }
        let Some(pending) = self.providers.pending.remove(&result.re)
        else {
            return Ok(());
        };
        if result.id == "timeout" {
            self.providers
                .controls
                .push(control(&result.re, None, None));
        }
        if let Some(login) = pending.login {
            if self.providers.logins.remove(&login).is_none() {
                return Ok(());
            }
            if result.error.is_none()
                && let Some(credential) = result.credential.clone()
            {
                let value = crate::credentials::unpack(credential)?;
                self.credentials
                    .save_model(&pending.provider, Some(value))?;
            }
            let step = match result.error {
                None => LoginStep::Done {
                    done: pending.provider.clone(),
                },
                Some(failed) => LoginStep::Failed { failed },
            };
            if let Some((client, id)) = &pending.from {
                self.answer(*client, login_step(id, &login, step))?;
            }
            self.publish().await?;
            return Ok(());
        }
        if let Some(providers) = result.providers {
            self.providers.providers = providers;
        }
        if let Some(models) = result.models {
            let mut grouped = BTreeMap::<String, Vec<Value>>::new();
            for model in models {
                let value = serde_json::to_value(model)?;
                let provider = value["provider"]
                    .as_str()
                    .unwrap_or(&pending.provider)
                    .to_owned();
                grouped.entry(provider).or_default().push(value);
            }
            if !pending.provider.is_empty() {
                grouped.entry(pending.provider.clone()).or_default();
            }
            for (provider, mut list) in grouped {
                for model in &mut list {
                    self.credentials.scrub(model);
                }
                self.take_models(&provider, &list)?;
                self.providers.lists.insert(provider, list);
            }
            write_whole(
                &self.providers.file,
                &serde_json::to_vec_pretty(&self.providers.lists)?,
            )?;
        }
        if pending.action == ProviderRequestAction::Describe {
            self.publish().await?;
            return Ok(());
        }
        let count = self
            .providers
            .lists
            .get(&pending.provider)
            .map_or(0, Vec::len);
        let words = match &result.error {
            Some(error) => plainly(
                error,
                pending.action == ProviderRequestAction::Hello,
            ),
            None => result.text.clone().unwrap_or_else(|| {
                format!("Connected, {count} model(s)")
            }),
        };
        self.model_link(
            if result.error.is_some() {
                LinkState::Failed
            } else {
                LinkState::Connected
            },
            (pending.action == ProviderRequestAction::Refresh
                && result.error.is_none())
            .then(|| i64::try_from(count))
            .transpose()?,
            Some(&words),
        )?;
        if let Some((client, id)) = &pending.from {
            if pending.read {
                let more = self
                    .providers
                    .pending
                    .values()
                    .any(|p| p.from == pending.from);
                if !more {
                    let read = self.catalogue_read(id)?;
                    self.answer(*client, read)?;
                }
            } else {
                self.answer(
                    *client,
                    reply(id, Ok::<_, Refusal>(words)),
                )?;
            }
        }
        self.publish().await
    }

    fn named_model(
        &mut self,
        name: &str,
    ) -> Result<Result<(String, String), Refusal>> {
        let usage = |words: String| Ok(Err(Refusal::Usage { words }));
        let Some((provider, id)) = name
            .split_once("::")
            .filter(|(p, m)| !p.is_empty() && !m.is_empty())
        else {
            return usage(format!(
                "`{name}` names no endpoint. A model is named \
                 `endpoint::model`."
            ));
        };
        if !self.provider_views()?.iter().any(|p| p.id == provider) {
            return usage(format!(
                "`{name}` is served by the endpoint `{provider}`, \
                 which the settings do not have."
            ));
        }
        let listed =
            self.providers.lists.get(provider).is_some_and(|list| {
                list.iter().any(|m| m["id"] == id)
            });
        if !listed && self.env.settings.model != name {
            return Ok(Err(Refusal::NoModel { model: name.into() }));
        }
        Ok(Ok((provider.into(), id.into())))
    }

    fn model_link(
        &mut self,
        state: LinkState,
        count: Option<i64>,
        words: Option<&str>,
    ) -> Result<()> {
        let words = words.map(|w| self.credentials.scrub_text(w));
        let db = self.realm.store().connection();
        let mut settings = crate::settings::stored(db)?;
        settings.link = hamlet_protocol::client::Link {
            state,
            models: count,
            words,
        };
        crate::settings::keep(
            db,
            &settings,
            crate::clock::now(),
            None,
        )?;
        self.env.settings.link = settings.link;
        Ok(())
    }

    /// The providers a chief may sign in to, and whether each is.
    pub(crate) fn provider_views(
        &mut self,
    ) -> Result<Vec<ProviderView>> {
        let serving = self
            .env
            .settings
            .provider()
            .unwrap_or_default()
            .to_owned();
        let mut providers = self.providers.providers.clone();
        if !self.providers.faux {
            providers.extend(self.env.settings.endpoints.iter().map(
                |e| ProviderResultProviders {
                    id: e.name.clone(),
                    key: true,
                    name: e.name.clone(),
                    subscription: false,
                },
            ));
        }
        providers.sort_by(|a, b| a.id.cmp(&b.id));
        providers.dedup_by(|a, b| a.id == b.id);
        Ok(providers
            .into_iter()
            .map(|p| {
                let mut methods = Vec::new();
                if p.key {
                    methods.push(LoginMethod::Key);
                }
                if p.subscription
                    && ["chatgpt", "openrouter"].contains(&&*p.id)
                {
                    methods.push(LoginMethod::Subscription);
                }
                let label = match &*p.id {
                    "chatgpt" => "Continue with ChatGPT".into(),
                    "openrouter" => "Sign in with OpenRouter".into(),
                    _ => p.name.clone(),
                };
                ProviderView {
                    configured: self
                        .credentials
                        .model(&p.id, &serving)
                        .is_some(),
                    methods,
                    label,
                    id: p.id,
                    name: p.name,
                }
            })
            .collect())
    }

    /// Takes the models `provider` lists into the catalogue, keeping
    /// a chief's own prices.
    fn take_models(
        &mut self,
        provider: &str,
        list: &[Value],
    ) -> Result<()> {
        let db = self.realm.store().connection();
        let mut cards = catalogue(db)?;
        for model in list {
            let name = format!(
                "{provider}::{}",
                model["id"].as_str().unwrap_or_default()
            );
            let card = cards.entry(name.clone()).or_insert(Listed {
                allowed: false,
                context: None,
                name,
                note: String::new(),
                price: None,
                priced: ListedPriced::Unknown,
            });
            let context = model
                .get("listedContext")
                .unwrap_or(&model["contextWindow"]);
            if let Some(context) = context.as_i64() {
                card.context = Some(context);
            }
            if card.priced == ListedPriced::Yours {
                continue;
            }
            let price = match model.get("listedPrice") {
                Some(listed) => {
                    serde_json::from_value(listed.clone())
                        .context("a model's listed price")?
                }
                None => {
                    let cost = &model["cost"];
                    cost["input"]
                        .as_f64()
                        .zip(cost["output"].as_f64())
                        .map(|(input, output)| ListedPrice {
                            input,
                            output,
                        })
                }
            };
            if price.is_some() {
                card.price = price;
                card.priced = ListedPriced::Listed;
            } else if matches!(
                card.priced,
                ListedPriced::Unknown | ListedPriced::Assigned
            ) {
                (card.price, card.priced) =
                    if is_local(model["baseUrl"].as_str()) {
                        (
                            Some(ListedPrice {
                                input: LOCAL_RATE,
                                output: LOCAL_RATE,
                            }),
                            ListedPriced::Assigned,
                        )
                    } else {
                        (None, ListedPriced::Unknown)
                    };
            }
        }
        keep_catalogue(db, &cards)
    }

    /// The catalogue, as a read of it answers.
    pub(crate) fn catalogue_read(
        &mut self,
        id: &RequestId,
    ) -> Result<ServerMessage> {
        let db = self.realm.store().connection();
        let models = catalogue(db)?.into_values().collect();
        Ok(ServerMessage::Read(ReadResult::Models {
            to: id.clone(),
            models,
        }))
    }

    /// Sets what a chief says of the model `name` in the catalogue.
    pub(crate) fn edit_catalogue(
        &mut self,
        name: &str,
        set: EditCatalogueSet,
    ) -> Result<Result<String, Refusal>> {
        let bad =
            set.price.as_ref().and_then(|p| p.as_ref()).is_some_and(
                |p| {
                    [p.input, p.output]
                        .iter()
                        .any(|n| !n.is_finite() || *n < 0.0)
                },
            );
        if bad {
            return Ok(Err(Refusal::Usage {
                words: "A cost is a number of zero or more.".into(),
            }));
        }
        let db = self.realm.store().connection();
        let mut cards = catalogue(db)?;
        let Some(card) = cards.get_mut(name) else {
            return Ok(Err(Refusal::NoModel { model: name.into() }));
        };
        if let Some(allowed) = set.allowed {
            card.allowed = allowed;
        }
        if let Some(note) = set.note {
            card.note = self.credentials.scrub_text(&note);
        }
        if let Some(price) = set.price {
            card.priced = if price.is_some() {
                ListedPriced::Yours
            } else {
                ListedPriced::Unknown
            };
            card.price = price;
        }
        keep_catalogue(db, &cards)?;
        Ok(Ok("Saved.".into()))
    }
}

fn control(
    scope: &str,
    prompt: Option<String>,
    value: Option<String>,
) -> ProviderControl {
    ProviderControl {
        v: runtime::VERSION,
        id: uuid::Uuid::new_v4().to_string(),
        scope: scope.into(),
        cancel: value.is_none(),
        prompt,
        value,
    }
}

fn login_step(
    id: &RequestId,
    login: &str,
    step: LoginStep,
) -> ServerMessage {
    ServerMessage::Login(LoginProgress {
        to: Some(id.clone()),
        login: login.into(),
        step,
    })
}

/// The model catalogue: each model's card, by its name.
type Catalogue = BTreeMap<String, Listed>;

fn catalogue(db: &mut Db) -> Result<Catalogue> {
    let text = document::table
        .find("catalogue")
        .select(document::value)
        .first::<String>(db)
        .optional()?;
    match text {
        Some(text) => serde_json::from_str(&text)
            .context("reading the model catalogue"),
        None => Ok(Catalogue::new()),
    }
}

fn keep_catalogue(db: &mut Db, cards: &Catalogue) -> Result<()> {
    diesel::replace_into(document::table)
        .values((
            document::kind.eq("catalogue"),
            document::value.eq(serde_json::to_string(cards)?),
            document::at.eq(crate::clock::now()),
        ))
        .execute(db)?;
    Ok(())
}

/// Whether `base_url` is served on this machine.
pub(crate) fn is_local(base_url: Option<&str>) -> bool {
    base_url
        .and_then(|s| url::Url::parse(s).ok())
        .is_some_and(|u| {
            u.host_str().is_some_and(|h| {
                h == "localhost"
                    || h == "[::1]"
                    || h == "0.0.0.0"
                    || h.starts_with("127.")
            })
        })
}

/// A provider's error in words a chief can act on.
fn plainly(error: &str, hello: bool) -> String {
    let lower = error.to_lowercase();
    if [
        "http 401",
        "http 403",
        "error: 401",
        "error: 403",
        "api key",
    ]
    .iter()
    .any(|s| lower.contains(s))
    {
        return "That key was not accepted. Check that you copied \
                all of it and that it is still active."
            .into();
    }
    if ["http 402", "error: 402", "credit", "quota", "billing"]
        .iter()
        .any(|s| lower.contains(s))
    {
        return "Your provider account has no credit left. Add \
                some, then try again."
            .into();
    }
    if lower.contains("could not reach") {
        return "Could not reach it. Check your connection and the \
                address."
            .into();
    }
    if lower.contains("in time") {
        return "It took too long to answer. Try again, or pick a \
                smaller model."
            .into();
    }
    let detail = error.chars().take(160).collect::<String>();
    if hello {
        return format!(
            "That model did not answer a hello. Pick another one. \
             ({detail})"
        );
    }
    if lower.contains("http 404") {
        return "That address did not answer as a model provider. \
                It usually ends in /v1."
            .into();
    }
    format!("It answered with an error. Try again. ({detail})")
}
