//! The chief's catalogue, provider requests and connection-owned
//! sign-ins (S7).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use hamlet_protocol::v0::{
    Check, ClientMessage, EditCatalogueSet, Login, LoginMethod, Read,
    ServerMessage,
};
use serde_json::{Value, json};

use crate::Store;
use crate::credentials::write_whole;
use crate::harness::link::{
    ModelRef, Price, ProviderControl, ProviderEvent, ProviderRequest,
    ProviderRequestAction, ProviderRequestMethod, ProviderResult,
    Selection,
};
use crate::harness::{Binding, LINK_VERSION};
use crate::protocol::{Refusal, reply};
use crate::service::{Service, document, save};
use crate::settings::{Link, Settings};

/// The price in dollars per million tokens of a model nothing prices.
const RATE: f64 = 1.0;
/// [`RATE`] for a model served on this machine.
const LOCAL_RATE: f64 = 0.1;

pub(crate) struct Models {
    file: PathBuf,
    lists: BTreeMap<String, Vec<Value>>,
    providers: Vec<Value>,
    pending: HashMap<String, Pending>,
    logins: HashMap<String, SignIn>,
    pub outgoing: Vec<ProviderRequest>,
    pub controls: Vec<ProviderControl>,
    faux: bool,
}

struct Pending {
    client: Option<u64>,
    id: String,
    provider: String,
    action: ProviderRequestAction,
    login: Option<String>,
    read: bool,
    expires: Option<Instant>,
}
struct SignIn {
    client: u64,
    id: String,
    scope: String,
    prompt: Option<String>,
}

impl Models {
    pub fn open(root: &Path, faux: bool) -> Result<Self> {
        let file = root.join("models.json");
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

    pub fn starting(&self) -> bool {
        self.pending.values().any(|p| p.client.is_none())
    }

    pub fn disconnect(&mut self, client: u64) {
        for (scope, _) in self
            .pending
            .iter()
            .filter(|(_, s)| s.client == Some(client))
        {
            self.controls.push(control(scope, None, None));
        }
        self.logins.retain(|_, s| s.client != client);
        self.pending.retain(|_, s| s.client != Some(client));
    }

    pub(crate) fn base_url(
        &self,
        provider: &str,
        model: &str,
    ) -> Option<&str> {
        self.lists
            .get(provider)?
            .iter()
            .find(|entry| entry["id"] == model)?["baseUrl"]
            .as_str()
    }

    pub fn expired(&self) -> Vec<ProviderResult> {
        self.pending
            .iter()
            .filter(|(_, p)| {
                p.expires.is_some_and(|end| Instant::now() >= end)
            })
            .map(|(key, _)| ProviderResult {
                v: LINK_VERSION,
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
}

impl Service {
    pub(crate) fn start_models(&mut self) -> Result<()> {
        self.ask_provider(
            None,
            "",
            "",
            ProviderRequestAction::Describe,
            None,
            None,
            false,
        )?;
        if !self.models.faux {
            let settings = self.settings()?;
            for endpoint in &settings.endpoints {
                self.ask_provider(
                    None,
                    "",
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
        client: Option<u64>,
        id: &str,
        provider: &str,
        action: ProviderRequestAction,
        model: Option<String>,
        login: Option<String>,
        read: bool,
    ) -> Result<()> {
        let settings = self.settings()?;
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
        self.models.pending.insert(
            key.clone(),
            Pending {
                client,
                id: id.into(),
                provider: provider.into(),
                action: action.clone(),
                login,
                read,
                expires: match action {
                    ProviderRequestAction::Describe
                    | ProviderRequestAction::Refresh => Some(12),
                    ProviderRequestAction::Hello => Some(95),
                    ProviderRequestAction::Login => None,
                }
                .map(|seconds| {
                    Instant::now() + Duration::from_secs(seconds)
                }),
            },
        );
        self.models.outgoing.push(ProviderRequest {
            v: LINK_VERSION,
            id: key,
            action,
            provider: provider.into(),
            base_url,
            model,
            credential,
            method: None,
            installation_id: Some(
                self.credentials.installation_id().into(),
            ),
            models: self
                .models
                .lists
                .get(provider)
                .into_iter()
                .flatten()
                .filter_map(|m| m.as_object().cloned())
                .map(|m| m.into_iter().collect())
                .collect(),
        });
        Ok(())
    }

    pub(crate) fn model_request(
        &mut self,
        client: u64,
        message: &ClientMessage,
    ) -> Result<Option<Vec<ServerMessage>>> {
        let mut answers = Vec::new();
        match message {
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
                    self.settings()
                        .map(|s| s.model)
                        .unwrap_or_default()
                });
                let (provider, model) = match self
                    .named_model(&name)?
                {
                    Ok(found) => found,
                    Err(error) => {
                        return Ok(Some(vec![reply(id, Err(error))]));
                    }
                };
                self.model_link("testing", None, None)?;
                self.ask_provider(
                    Some(client),
                    id,
                    &provider,
                    action,
                    Some(model),
                    None,
                    false,
                )?;
            }
            ClientMessage::Read(Read::Models { id, refresh }) => {
                if *refresh == Some(true) && !self.models.faux {
                    let settings = self.settings()?;
                    let endpoints = settings.endpoints;
                    for endpoint in &endpoints {
                        self.ask_provider(
                            Some(client),
                            id,
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
                        .find(|p| p["id"] == start.provider);
                    let method = if start.method == LoginMethod::Key {
                        "key"
                    } else {
                        "subscription"
                    };
                    let failure = if let Some(provider) = &provider {
                        (!provider["methods"].as_array().is_some_and(
                            |methods| {
                                methods.iter().any(|m| m == method)
                            },
                        ))
                        .then(|| {
                            format!(
                                "Signing in with {} is not offered. \
                                 Use an API key.",
                                start.provider
                            )
                        })
                    } else {
                        Some(format!(
                            "Unknown provider: {}",
                            start.provider
                        ))
                    };
                    if let Some(failure) = failure {
                        answers.push(login_step(
                            id,
                            &key,
                            json!({"failed":failure}),
                        )?);
                    } else {
                        self.ask_provider(
                            Some(client),
                            id,
                            &start.provider,
                            ProviderRequestAction::Login,
                            None,
                            Some(key.clone()),
                            false,
                        )?;
                        let request = self
                            .models
                            .outgoing
                            .last_mut()
                            .context("login request")?;
                        request.method = Some(
                            if start.method == LoginMethod::Key {
                                ProviderRequestMethod::Key
                            } else {
                                ProviderRequestMethod::Subscription
                            },
                        );
                        self.models.logins.insert(
                            key,
                            SignIn {
                                client,
                                id: id.clone(),
                                scope: request.id.clone(),
                                prompt: None,
                            },
                        );
                    }
                }
                Login::Reply { id, reply: answer } => {
                    let Some(signin) = self
                        .models
                        .logins
                        .get_mut(&answer.login)
                        .filter(|s| s.client == client)
                    else {
                        return Ok(Some(vec![reply(
                            id,
                            Err(Refusal::Invalid(
                                "There is no sign-in here with that \
                                 id."
                                .into(),
                            )),
                        )]));
                    };
                    let Some(prompt) = signin.prompt.take() else {
                        return Ok(Some(vec![reply(
                            id,
                            Err(Refusal::Invalid(
                                "That sign-in is not waiting for an \
                                 answer."
                                    .into(),
                            )),
                        )]));
                    };
                    self.credentials.remember(&answer.value);
                    self.models.controls.push(control(
                        &signin.scope,
                        Some(prompt),
                        Some(answer.value.clone()),
                    ));
                    answers.push(reply(
                        id,
                        Ok("Sign-in answer received.".into()),
                    ));
                }
                Login::Cancel { id, cancel } => {
                    let Some(signin) = self
                        .models
                        .logins
                        .get(cancel)
                        .filter(|s| s.client == client)
                    else {
                        return Ok(Some(vec![reply(
                            id,
                            Err(Refusal::Invalid(
                                "There is no sign-in here with that \
                                 id."
                                .into(),
                            )),
                        )]));
                    };
                    answers.push(login_step(
                        &signin.id,
                        cancel,
                        json!({"failed":"Sign-in cancelled."}),
                    )?);
                    self.models.controls.push(control(
                        &signin.scope,
                        None,
                        None,
                    ));
                    self.models.logins.remove(cancel);
                    self.models.pending.retain(|_, p| {
                        p.login.as_ref() != Some(cancel)
                    });
                    answers.push(reply(
                        id,
                        Ok("Sign-in cancelled.".into()),
                    ));
                }
            },
            _ => return Ok(None),
        }
        Ok(Some(answers))
    }

    pub(crate) fn provider_event(
        &mut self,
        event: ProviderEvent,
    ) -> Result<Vec<(u64, ServerMessage)>> {
        let Some(pending) = self.models.pending.get(&event.scope)
        else {
            return Ok(Vec::new());
        };
        let Some(login) = &pending.login else {
            return Ok(Vec::new());
        };
        let Some(signin) = self.models.logins.get_mut(login) else {
            return Ok(Vec::new());
        };
        if let Some(prompt) = event.prompt {
            signin.prompt = (!prompt.is_empty()).then_some(prompt);
        }
        if event.step.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![(
            signin.client,
            login_step(
                &signin.id,
                login,
                serde_json::to_value(event.step)?,
            )?,
        )])
    }

    pub(crate) fn provider_result(
        &mut self,
        result: ProviderResult,
    ) -> Result<Vec<(u64, ServerMessage)>> {
        let Some(pending) = self.models.pending.remove(&result.re)
        else {
            return Ok(Vec::new());
        };
        if result.id == "timeout" {
            self.models
                .controls
                .push(control(&result.re, None, None));
        }
        let mut answers = Vec::new();
        if let Some(login) = pending.login {
            if self.models.logins.remove(&login).is_none() {
                return Ok(answers);
            }
            let step = if result.error.is_none() {
                json!({"done":pending.provider})
            } else {
                json!({"failed":result.error.unwrap_or_else(||"Sign-in failed.".into())})
            };
            if let Some(client) = pending.client {
                answers.push((
                    client,
                    login_step(&pending.id, &login, step)?,
                ));
            }
            return Ok(answers);
        }
        if let Some(providers) = result.providers {
            self.models.providers = providers
                .into_iter()
                .map(serde_json::to_value)
                .collect::<std::result::Result<_, _>>()?;
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
                self.models.lists.insert(provider, list);
            }
            write_whole(
                &self.models.file,
                &serde_json::to_vec_pretty(&self.models.lists)?,
            )?;
        }
        if pending.action == ProviderRequestAction::Describe {
            return Ok(answers);
        }
        let count = self
            .models
            .lists
            .get(&pending.provider)
            .map_or(0, Vec::len);
        let words = result
            .error
            .as_ref()
            .map(|e| {
                plainly(
                    e,
                    pending.action == ProviderRequestAction::Hello,
                )
            })
            .unwrap_or_else(|| {
                result.text.unwrap_or_else(|| {
                    format!("Connected, {count} model(s)")
                })
            });
        self.model_link(
            if result.error.is_some() {
                "failed"
            } else {
                "connected"
            },
            if pending.action == ProviderRequestAction::Refresh
                && result.error.is_none()
            {
                Some(count)
            } else {
                None
            },
            Some(&words),
        )?;
        if let Some(client) = pending.client {
            if pending.read {
                if !self.models.pending.values().any(|p| {
                    p.client == Some(client) && p.id == pending.id
                }) {
                    answers.push((
                        client,
                        self.catalogue_read(&pending.id)?,
                    ));
                }
            } else {
                answers.push((client, reply(&pending.id, Ok(words))));
            }
        }
        Ok(answers)
    }

    fn named_model(
        &mut self,
        name: &str,
    ) -> Result<std::result::Result<(String, String), Refusal>> {
        let invalid = |text| Ok(Err(Refusal::Invalid(text)));
        let Some((provider, id)) = name
            .split_once("::")
            .filter(|(p, m)| !p.is_empty() && !m.is_empty())
        else {
            return invalid(format!(
                "`{name}` names no endpoint. A model is named \
                 `endpoint::model`"
            ));
        };
        if !self.provider_views()?.iter().any(|p| p["id"] == provider)
        {
            return invalid(format!(
                "`{name}` is served by the endpoint `{provider}`, \
                 which the settings do not have"
            ));
        }
        if !self
            .models
            .lists
            .get(provider)
            .is_some_and(|list| list.iter().any(|m| m["id"] == id))
        {
            let settings = self.settings()?;
            if settings.model != name {
                return invalid(format!("no model named `{name}`"));
            }
        }
        Ok(Ok((provider.into(), id.into())))
    }

    fn model_link(
        &mut self,
        state: &str,
        count: Option<usize>,
        words: Option<&str>,
    ) -> Result<()> {
        let db = self.home.store().connection();
        let mut settings = crate::settings::stored(db)?;
        settings.link = Link {
            state: state.into(),
            models: count,
            words: words.map(|w| self.credentials.scrub_text(w)),
        };
        crate::settings::keep(db, &settings)
    }

    pub(crate) fn provider_views(&mut self) -> Result<Vec<Value>> {
        let settings = self.settings()?;
        let serving = settings.provider().unwrap_or_default();
        let mut providers = self.models.providers.clone();
        if !self.models.faux {
            providers.extend(settings.endpoints.iter().map(
                |e| json!({"id":e.name,"name":e.name,"key":true}),
            ));
        }
        providers
            .sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        Ok(providers.iter().map(|p| {
            let id = p["id"].as_str().unwrap_or_default();
            let mut methods = Vec::new();
            if p["key"] == true { methods.push("key"); }
            if p["subscription"] == true && ["chatgpt", "openrouter"].contains(&id) { methods.push("subscription"); }
            let label = match id { "chatgpt" => "Continue with ChatGPT", "openrouter" => "Sign in with OpenRouter", _ => p["name"].as_str().unwrap_or_default() };
            json!({"id":id,"name":p["name"],"configured":self.credentials.model(id,serving).is_some(),"methods":methods,"label":label})
        }).collect())
    }

    fn take_models(
        &mut self,
        provider: &str,
        list: &[Value],
    ) -> Result<()> {
        let mut cards = document(
            self.home.store().connection(),
            "catalogue",
            "home",
        )?
        .unwrap_or_else(|| json!({}));
        for model in list {
            let name = format!(
                "{provider}::{}",
                model["id"].as_str().unwrap_or_default()
            );
            let card = &mut cards[&name];
            if card.is_null() {
                *card = json!({"name":name,"price":null,"priced":"unknown","context":null,"allowed":false,"note":""});
            }
            let context = model
                .get("listedContext")
                .unwrap_or(&model["contextWindow"]);
            if !context.is_null() {
                card["context"] = context.clone();
            }
            if card["priced"] != "yours" {
                let price = model.get("listedPrice").cloned().unwrap_or_else(||json!({"input":model["cost"]["input"],"output":model["cost"]["output"]}));
                if !price.is_null() {
                    card["price"] = price;
                    card["priced"] = "listed".into();
                } else if card["priced"] == "unknown"
                    || card["priced"] == "assigned"
                {
                    let local = local(model["baseUrl"].as_str());
                    card["price"] = if local {
                        json!({"input":LOCAL_RATE,"output":LOCAL_RATE})
                    } else {
                        Value::Null
                    };
                    card["priced"] =
                        if local { "assigned" } else { "unknown" }
                            .into();
                }
            }
        }
        save(
            self.home.store().connection(),
            "catalogue",
            "home",
            &cards,
        )
    }

    pub(crate) fn catalogue_read(
        &mut self,
        id: &str,
    ) -> Result<ServerMessage> {
        let cards = document(
            self.home.store().connection(),
            "catalogue",
            "home",
        )?
        .unwrap_or_else(|| json!({}));
        let mut cards = cards
            .as_object()
            .into_iter()
            .flat_map(|m| m.values().cloned())
            .collect::<Vec<_>>();
        cards.sort_by(|a, b| {
            a["name"].as_str().cmp(&b["name"].as_str())
        });
        Ok(serde_json::from_value(
            json!({"type":"read","to":id,"models":cards}),
        )?)
    }

    pub(crate) fn edit_catalogue(
        &mut self,
        name: &str,
        set: EditCatalogueSet,
    ) -> Result<std::result::Result<String, Refusal>> {
        if set.price.as_ref().and_then(|p| p.as_ref()).is_some_and(
            |p| {
                [p.input, p.output]
                    .iter()
                    .any(|n| !n.is_finite() || *n < 0.0)
            },
        ) {
            return Ok(Err(Refusal::Invalid(
                "a cost is a number of zero or more".into(),
            )));
        }
        let mut cards = document(
            self.home.store().connection(),
            "catalogue",
            "home",
        )?
        .unwrap_or_else(|| json!({}));
        let Some(card) = cards.get_mut(name) else {
            return Ok(Err(Refusal::Invalid(format!(
                "no model named `{name}`"
            ))));
        };
        if let Some(allowed) = set.allowed {
            card["allowed"] = allowed.into();
        }
        if let Some(note) = set.note {
            card["note"] = note.into();
        }
        if let Some(price) = set.price {
            card["priced"] =
                if price.is_some() { "yours" } else { "unknown" }
                    .into();
            card["price"] = serde_json::to_value(price)?;
        }
        self.credentials.scrub(&mut cards);
        save(
            self.home.store().connection(),
            "catalogue",
            "home",
            &cards,
        )?;
        Ok(Ok("Saved.".into()))
    }
}

fn control(
    scope: &str,
    prompt: Option<String>,
    value: Option<String>,
) -> ProviderControl {
    ProviderControl {
        v: LINK_VERSION,
        id: uuid::Uuid::new_v4().to_string(),
        scope: scope.into(),
        cancel: value.is_none(),
        prompt,
        value,
    }
}

fn login_step(
    id: &str,
    login: &str,
    step: Value,
) -> Result<ServerMessage> {
    Ok(serde_json::from_value(
        json!({"type":"login","to":id,"login":login,"step":step}),
    )?)
}

/// What a model call is chosen from, read once for many calls.
pub(crate) struct Choices {
    settings: Settings,
    cards: Value,
    /// The endpoints' model lists, by endpoint.
    lists: Value,
}

impl Choices {
    pub fn read(
        store: &Store,
        root: &Path,
        faux: bool,
    ) -> Result<Self> {
        let settings = crate::settings::Overrides::from_env(faux)?
            .apply(crate::settings::stored(store.connection())?);
        let lists = match std::fs::read(root.join("models.json")) {
            Ok(bytes) => serde_json::from_slice::<Value>(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                json!({})
            }
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            settings,
            cards: document(store.connection(), "catalogue", "home")?
                .unwrap_or_else(|| json!({})),
            lists,
        })
    }

    /// The model the next call of `binding` is made with, as the
    /// limits of its hamlet and the settings choose it now.
    pub fn select(
        &self,
        store: &Store,
        binding: &Binding,
    ) -> Result<Selection> {
        let record = store
            .record(&binding.agent.hamlet)?
            .context("model's hamlet")?;
        let agent = record
            .agents
            .iter()
            .find(|a| a.number == binding.agent.number)
            .context("model's agent")?;
        let name = record.limits.chief.model_of(
            agent,
            record.mayor == agent.number,
            &self.settings.model,
        );
        let (provider, id) = name
            .split_once("::")
            .context("a model is named endpoint::model")?;
        let listed = self.lists[provider]
            .as_array()
            .and_then(|list| list.iter().find(|m| m["id"] == id))
            .unwrap_or(&Value::Null);
        let base_url = match self.settings.base_url(provider) {
            Some(url) => Some(url.into()),
            None => listed["baseUrl"].as_str().map(String::from),
        };
        let size =
            |key: &str| listed[key].as_u64().filter(|n| *n > 0);
        let card = &self.cards[&name]["price"];
        let price =
            match (card["input"].as_f64(), card["output"].as_f64()) {
                (Some(input), Some(output)) => Price {
                    input,
                    output,
                    cache_read: input,
                    cache_write: input,
                },
                _ => serde_json::from_value(listed["cost"].clone())
                    .unwrap_or_else(|_| {
                        let rate = if local(base_url.as_deref()) {
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
                    }),
            };
        Ok(Selection {
            context_window: size("contextWindow"),
            max_tokens: size("maxTokens"),
            model: ModelRef {
                provider: provider.into(),
                id: id.into(),
                base_url,
            },
            price,
        })
    }
}

fn local(base_url: Option<&str>) -> bool {
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
