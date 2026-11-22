//! The chief's model catalogue, the runtimes' providers and sign-ins
//! (S7).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use bevy_ecs::entity::Entity;
use diesel::prelude::*;
use hamlet_protocol::client::{
    Check, EditCatalogueSet, ImageInput, LinkState, Listed,
    ListedPriced, Login, LoginMethod, LoginProgress, LoginReply,
    LoginStart, LoginStep, Price as ListedPrice, ProviderView,
    ReadResult, RequestId, ServerMessage,
};
use hamlet_protocol::runtime::{
    self, ModelRef, Price, ProviderControl, ProviderEvent,
    ProviderRequest, ProviderRequestAction, ProviderRequestMethod,
    ProviderResult, ProviderResultProviders, Selection,
};
use hamlet_store::Db;
use hamlet_store::schema::document;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::timers::AddTimer;
use crate::clients::check::reply;
use crate::core::Core;
use crate::realm::change::Done;
use crate::realm::credentials::write_whole;
use crate::realm::layout::Layout;
use crate::realm::settings::Settings;
use crate::society::agents::Effort;
use crate::society::rejection::Rejection;

mod limits;
mod listing;

use self::limits::Told;
use self::listing::Listing;

/// The price in dollars per million tokens of a model nothing prices.
const RATE: f64 = 1.0;
/// [`RATE`] for a model served on this machine.
const LOCAL_RATE: f64 = 0.1;
/// The provider of the faux models a faux realm runs.
pub(crate) const FAUX: &str = "faux";

pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_timer(expiry_due, expire);
}

/// The soonest a request to the providers runs out of time.
fn expiry_due(
    providers: bevy_ecs::system::Res<Providers>,
) -> Option<Instant> {
    providers.next_expiry()
}

/// Answers the requests to the providers that ran out of time.
fn expire(mut core: Core) -> Result<()> {
    core.expire_providers()
}

#[derive(bevy_ecs::resource::Resource)]
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
    /// What each provider last told of its limits.
    limits: Told,
    faux: bool,
}

struct Pending {
    /// The connection and request it answers; unset when the server
    /// sent it.
    from: Option<(u64, RequestId)>,
    provider: String,
    action: ProviderRequestAction,
    login: Option<String>,
    settles: Settles,
    expires: Option<Instant>,
    /// A read of limits made with a sign-in since changed or
    /// removed: what it tells is not the provider's now.
    forgotten: bool,
}

/// The answer a request's client gets at its end.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Settles {
    Reply,
    Catalogue,
    /// Once every provider asked for its limits answered.
    Limits,
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
            limits: Told::default(),
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

    /// Forgets what `provider` told of its limits, and what the reads
    /// of them under way tell, as when its sign-in changes.
    fn forget_limits(&mut self, provider: &str) {
        self.limits.forget(provider);
        for pending in self.pending.values_mut().filter(|p| {
            p.provider == provider
                && p.action == ProviderRequestAction::Limits
        }) {
            pending.forgotten = true;
        }
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
                limits: None,
                models: None,
                providers: None,
                text: None,
            })
            .collect()
    }

    /// The listing of `model` of `provider`; the empty one when it is
    /// not listed.
    fn listed(&self, provider: &str, model: &str) -> Listing {
        self.lists
            .get(provider)
            .into_iter()
            .flatten()
            .map(Listing::of)
            .find(|listed| listed.id == model)
            .unwrap_or_default()
    }

    /// The images `model` takes, none when it takes none (S30).
    pub fn images(
        &self,
        db: &mut Db,
        model: &ModelRef,
    ) -> Result<Option<ImageInput>> {
        if model.provider == FAUX {
            return Ok(Some(ImageInput::default()));
        }
        let name = format!("{}::{}", model.provider, model.id);
        Ok(catalogue(db)?.remove(&name).and_then(|card| {
            card.takes(&self.listed(&model.provider, &model.id))
        }))
    }

    /// The model call of `model` as it is made: its address, its size
    /// and its price.
    pub fn select(
        &self,
        db: &mut Db,
        settings: &Settings,
        model: ModelRef,
        effort: Option<Effort>,
        fast: bool,
    ) -> Result<Selection> {
        let listed = self.listed(&model.provider, &model.id);
        let base_url = model.base_url.clone().or_else(|| {
            settings
                .base_url(&model.provider)
                .map(String::from)
                .or_else(|| listed.base_url.clone())
        });
        let size = |size: Option<u64>| size.filter(|n| *n > 0);
        let name = format!("{}::{}", model.provider, model.id);
        let cards = catalogue(db)?;
        let price = if model.provider == FAUX {
            flat_price(0.0)
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
                None => listed.price().unwrap_or_else(|| {
                    flat_price(if is_local(base_url.as_deref()) {
                        LOCAL_RATE
                    } else {
                        RATE
                    })
                }),
            }
        };
        // A model set to an effort or tier it lacks gets the
        // nearest effort it has, and the standard tier.
        let card =
            cards.get(&name).filter(|_| model.provider != FAUX);
        let effort = effort.map(|effort| match card {
            Some(card) if !card.efforts.is_empty() => {
                effort.clamped(&card.efforts)
            }
            _ => effort,
        });
        Ok(Selection {
            context_window: size(listed.context_window),
            max_tokens: size(listed.max_tokens),
            effort: effort.map(Effort::wire),
            fast: fast && card.is_some_and(|card| card.fast),
            images: card
                .is_some_and(|card| card.takes(&listed).is_some()),
            model: ModelRef { base_url, ..model },
            price,
        })
    }
}

impl Core<'_, '_> {
    /// Requests what the providers offer, and each endpoint's
    /// models.
    pub(crate) fn start_providers(&mut self) -> Result<()> {
        self.send_provider_request(
            None,
            "",
            ProviderRequestAction::Describe,
            None,
            None,
            Settles::Reply,
        )?;
        if !self.providers.faux {
            let endpoints = self.env.settings.endpoints.clone();
            for endpoint in &endpoints {
                self.send_provider_request(
                    None,
                    &endpoint.name,
                    ProviderRequestAction::Refresh,
                    None,
                    None,
                    Settles::Reply,
                )?;
            }
        }
        Ok(())
    }

    fn send_provider_request(
        &mut self,
        from: Option<(u64, &RequestId)>,
        provider: &str,
        action: ProviderRequestAction,
        model: Option<String>,
        login: Option<String>,
        settles: Settles,
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
            .map(crate::realm::credentials::pack)
            .transpose()?;
        let key = format!("provider:{}", uuid::Uuid::new_v4());
        let expires = match action {
            ProviderRequestAction::Describe
            | ProviderRequestAction::Refresh => Some(12),
            ProviderRequestAction::Hello => Some(95),
            ProviderRequestAction::Limits
            | ProviderRequestAction::Logout => Some(20),
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
                settles,
                expires,
                forgotten: false,
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
    pub(crate) fn send_providers(&mut self) -> Result<()> {
        let db = self.realm.store().connection();
        let id = crate::runtimes::of_harness(
            db,
            crate::society::agents::HARNESS,
        )?;
        let Some(to) = self.runtimes.find(&id) else {
            return Ok(());
        };
        if !self.runtimes.is_ready(to) {
            return Ok(());
        }
        // A request keeps its own id, which its result answers.
        for request in std::mem::take(&mut self.providers.outgoing) {
            self.leases.grant(to, &request.id, &request.provider);
            self.runtimes.send(to, |_, _| {
                runtime::ServerMessage::ProviderRequest(request)
            });
        }
        for control in std::mem::take(&mut self.providers.controls) {
            let open = self
                .runtimes
                .all()
                .into_iter()
                .filter(|at| self.leases.is_open(*at, &control.scope))
                .collect::<Vec<_>>();
            for at in open {
                if control.cancel {
                    self.leases.finish(at, &control.scope);
                }
                let control = control.clone();
                self.runtimes.send(at, |_, _| {
                    runtime::ServerMessage::ProviderControl(control)
                });
            }
        }
        Ok(())
    }

    /// Answers the provider requests that timed out.
    pub(crate) fn expire_providers(&mut self) -> Result<()> {
        for result in self.providers.expired() {
            self.provider_result(None, result)?;
        }
        Ok(())
    }

    /// Tries the chosen model or the connection, for a client's
    /// check.
    pub(crate) fn check(
        &mut self,
        client: u64,
        check: &Check,
    ) -> Result<()> {
        let (id, name, action) = match check {
            Check::Model { id, model } => {
                (id, model.clone(), ProviderRequestAction::Hello)
            }
            Check::Connection { id } => {
                (id, None, ProviderRequestAction::Refresh)
            }
            Check::Limits { id } => {
                return self.check_limits(client, id);
            }
        };
        // The faux model needs no trying.
        if self.providers.faux {
            let words = "Every agent runs the faux model here.";
            self.model_link(LinkState::Connected, None, Some(words))?;
            self.publish();
            return self.answer(
                client,
                reply(id, Ok::<_, Rejection>(words.into())),
            );
        }
        let name =
            name.unwrap_or_else(|| self.env.settings.model.clone());
        let (provider, model) = match self.named_model(&name)? {
            Ok(found) => found,
            Err(rejection) => {
                return self
                    .answer(client, reply(id, Err(rejection)));
            }
        };
        self.model_link(LinkState::Testing, None, None)?;
        self.send_provider_request(
            Some((client, id)),
            &provider,
            action,
            Some(model),
            None,
            Settles::Reply,
        )?;
        self.send_providers()
    }

    /// Asks each signed-in provider that keeps limits how much of
    /// them is used, unless it told so within the last minute, and
    /// answers `client` once each answered.
    fn check_limits(
        &mut self,
        client: u64,
        id: &RequestId,
    ) -> Result<()> {
        let now = crate::app::clock::now();
        let asked = self
            .provider_views()?
            .into_iter()
            .filter(|p| {
                self.reads_limits(p)
                    && !self.providers.limits.fresh(&p.id, now)
                    && !self.asking_limits(&p.id)
            })
            .map(|p| p.id)
            .collect::<Vec<_>>();
        if asked.is_empty() {
            return self.answer(
                client,
                reply(
                    id,
                    Ok::<_, Rejection>("Limits checked.".into()),
                ),
            );
        }
        for provider in &asked {
            self.send_provider_request(
                Some((client, id)),
                provider,
                ProviderRequestAction::Limits,
                None,
                None,
                Settles::Limits,
            )?;
        }
        self.send_providers()
    }

    /// Whether the limits of `provider` are read: a signed-in
    /// provider that keeps them, or the faux provider of a faux
    /// realm.
    fn reads_limits(&self, provider: &ProviderView) -> bool {
        if self.providers.faux {
            provider.id == FAUX
        } else {
            provider.configured
                && limits::READ.contains(&&*provider.id)
        }
    }

    /// Whether a read of `provider`'s limits is under way.
    fn asking_limits(&self, provider: &str) -> bool {
        self.providers.pending.values().any(|p| {
            p.provider == provider
                && p.action == ProviderRequestAction::Limits
        })
    }

    /// Takes what a turn learnt of a provider's limits; a new refusal
    /// reads all of them again, where the provider tells them.
    pub(crate) fn limits_seen(
        &mut self,
        seen: runtime::LimitsSeen,
    ) -> Result<()> {
        let provider = seen.provider;
        let refused =
            self.providers.limits.seen(&provider, seen.limits);
        if refused && !self.asking_limits(&provider) {
            self.send_provider_request(
                None,
                &provider,
                ProviderRequestAction::Limits,
                None,
                None,
                Settles::Reply,
            )?;
            self.send_providers()?;
        }
        self.publish();
        Ok(())
    }

    /// Answers a client's read of the models, requesting each
    /// endpoint's models first when `refresh` says so.
    pub(crate) fn read_models(
        &mut self,
        client: u64,
        id: &RequestId,
        refresh: Option<bool>,
    ) -> Result<()> {
        let endpoints =
            if refresh == Some(true) && !self.providers.faux {
                self.env.settings.endpoints.clone()
            } else {
                Vec::new()
            };
        // The catalogue is answered once every endpoint answered.
        if endpoints.is_empty() {
            let read = self.catalogue_read(id)?;
            self.answer(client, read)?;
        }
        for endpoint in &endpoints {
            self.send_provider_request(
                Some((client, id)),
                &endpoint.name,
                ProviderRequestAction::Refresh,
                None,
                None,
                Settles::Catalogue,
            )?;
        }
        self.send_providers()
    }

    /// Takes a step of a client's sign-in: its start, its answer to
    /// what the provider put to it, or its cancel.
    pub(crate) fn login(
        &mut self,
        client: u64,
        login: &Login,
    ) -> Result<()> {
        match login {
            Login::Start { id, start } => {
                self.login_start(client, id, start)?;
            }
            Login::Reply { id, reply: answer } => {
                let words = self.login_reply(client, answer);
                self.answer(client, reply(id, words))?;
            }
            Login::Cancel { id, cancel } => {
                let words = self.login_cancel(client, cancel)?;
                self.answer(client, reply(id, words))?;
            }
            Login::Forget { id, forget } => {
                self.login_forget(client, id, forget)?;
            }
        }
        self.send_providers()
    }

    /// Starts signing in to the provider `start` names, by the method
    /// it chose.
    fn login_start(
        &mut self,
        client: u64,
        id: &RequestId,
        start: &LoginStart,
    ) -> Result<()> {
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
                    "Signing in with {} is not offered. Use an API \
                     key.",
                    start.provider
                )
            }),
            None => {
                Some(format!("Unknown provider: {}", start.provider))
            }
        };
        if let Some(failed) = failure {
            return self.answer(
                client,
                login_step(id, &key, LoginStep::Failed { failed }),
            );
        }
        let scope = self.send_provider_request(
            Some((client, id)),
            &start.provider,
            ProviderRequestAction::Login,
            None,
            Some(key.clone()),
            Settles::Reply,
        )?;
        let request = self
            .providers
            .outgoing
            .last_mut()
            .context("the sign-in's request")?;
        request.method = Some(if start.method == LoginMethod::Key {
            ProviderRequestMethod::Key
        } else {
            ProviderRequestMethod::Subscription
        });
        self.providers.logins.insert(
            key,
            SignIn {
                client,
                id: id.clone(),
                scope,
                prompt: None,
            },
        );
        Ok(())
    }

    /// Passes a client's answer to what its sign-in put to it on to
    /// the provider.
    fn login_reply(
        &mut self,
        client: u64,
        answer: &LoginReply,
    ) -> Result<String, Rejection> {
        let signin = self
            .providers
            .logins
            .get_mut(&answer.login)
            .filter(|s| s.client == client)
            .ok_or_else(no_sign_in)?;
        let prompt = signin
            .prompt
            .take()
            .ok_or(Rejection::SignInNotWaiting)?;
        let scope = signin.scope.clone();
        self.credentials.remember(&answer.value);
        self.providers.controls.push(control(
            &scope,
            Some(prompt),
            Some(answer.value.clone()),
        ));
        Ok("Sign-in answer received.".into())
    }

    /// Cancels a client's sign-in `cancel`, telling its start too.
    fn login_cancel(
        &mut self,
        client: u64,
        cancel: &str,
    ) -> Done<String> {
        let ours = self
            .providers
            .logins
            .get(cancel)
            .is_some_and(|s| s.client == client);
        let Some(signin) = ours
            .then(|| self.providers.logins.remove(cancel))
            .flatten()
        else {
            return Ok(Err(no_sign_in()));
        };
        self.answer(
            client,
            login_step(
                &signin.id,
                cancel,
                LoginStep::Failed {
                    failed: "Sign-in cancelled.".into(),
                },
            ),
        )?;
        self.providers.controls.push(control(
            &signin.scope,
            None,
            None,
        ));
        self.providers
            .pending
            .retain(|_, p| p.login.as_deref() != Some(cancel));
        Ok(Ok("Sign-in cancelled.".into()))
    }

    /// Signs out of `provider`, or removes its key: the credential
    /// and what it told of its limits are forgotten at once, and a
    /// runtime that keeps a login of its own, as Claude's program
    /// does, signs out too before the client is answered.
    fn login_forget(
        &mut self,
        client: u64,
        id: &RequestId,
        provider: &str,
    ) -> Result<()> {
        let name = self
            .provider_views()?
            .into_iter()
            .find(|p| p.id == provider)
            .map_or_else(|| provider.to_owned(), |p| p.name);
        if !self.credentials.is_configured(provider) {
            return self.answer(
                client,
                reply(
                    id,
                    Ok::<_, Rejection>(format!(
                        "Nothing is kept for {name}."
                    )),
                ),
            );
        }
        // A sign-in under way would keep its credential once it ends.
        let ended = self
            .providers
            .pending
            .iter()
            .filter(|(_, p)| p.provider == provider)
            .filter_map(|(scope, p)| {
                Some((scope.clone(), p.login.clone()?))
            })
            .collect::<Vec<_>>();
        for (scope, login) in ended {
            self.providers.pending.remove(&scope);
            self.providers.controls.push(control(&scope, None, None));
            if let Some(signin) = self.providers.logins.remove(&login)
            {
                self.answer(
                    signin.client,
                    login_step(
                        &signin.id,
                        &login,
                        LoginStep::Failed {
                            failed: format!(
                                "Sign-in stopped: {name} was \
                                 removed."
                            ),
                        },
                    ),
                )?;
            }
        }
        self.credentials.save_model(provider, None)?;
        self.providers.forget_limits(provider);
        self.publish();
        if provider == "claude" {
            self.send_provider_request(
                Some((client, id)),
                provider,
                ProviderRequestAction::Logout,
                None,
                None,
                Settles::Reply,
            )?;
            return Ok(());
        }
        self.answer(
            client,
            reply(
                id,
                Ok::<_, Rejection>(format!("{name} is removed.")),
            ),
        )
    }

    /// A step of a sign-in the runtime `from` runs.
    pub(crate) fn provider_event(
        &mut self,
        from: Entity,
        event: ProviderEvent,
    ) -> Result<()> {
        if !self.leases.is_open(from, &event.scope) {
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

    /// The end of a provider request, from the runtime `from` or its
    /// timeout.
    pub(crate) fn provider_result(
        &mut self,
        from: Option<Entity>,
        result: ProviderResult,
    ) -> Result<()> {
        if let Some(from) = from {
            self.leases.finish(from, &result.re);
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
                let value =
                    crate::realm::credentials::unpack(credential)?;
                self.credentials
                    .save_model(&pending.provider, Some(value))?;
                // Another sign-in may be another plan.
                self.providers.forget_limits(&pending.provider);
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
            self.publish();
            return Ok(());
        }
        if pending.action == ProviderRequestAction::Logout {
            let words = match &result.error {
                None => "Claude is removed, and its program signed \
                         out."
                    .to_owned(),
                Some(error) => format!(
                    "Claude is removed from Hamlet, but its program \
                     could not sign out: {}",
                    self.credentials.scrub_text(error)
                ),
            };
            if let Some((client, id)) = &pending.from {
                self.answer(
                    *client,
                    reply(id, Ok::<_, Rejection>(words)),
                )?;
            }
            self.publish();
            return Ok(());
        }
        if pending.action == ProviderRequestAction::Limits {
            // A provider that could not be read keeps what it told
            // before; one that told nothing yet reads as unknown, so
            // the chief sees that it was tried.
            match (&result.error, result.limits) {
                _ if pending.forgotten => {}
                (None, Some(limits)) => {
                    self.providers
                        .limits
                        .read(&pending.provider, limits);
                }
                (Some(_), _) => {
                    self.providers.limits.failed(&pending.provider);
                }
                (None, None) => {}
            }
            let more = self.providers.pending.values().any(|p| {
                pending.from.is_some() && p.from == pending.from
            });
            if let Some((client, id)) = &pending.from
                && !more
            {
                self.answer(
                    *client,
                    reply(
                        id,
                        Ok::<_, Rejection>("Limits checked.".into()),
                    ),
                )?;
            }
            self.publish();
            return Ok(());
        }
        if let Some(providers) = result.providers {
            self.providers.providers = providers;
        }
        if let Some(models) = result.models {
            let mut grouped = BTreeMap::<String, Vec<Value>>::new();
            for model in models {
                let value = serde_json::to_value(model)?;
                let provider = Listing::of(&value)
                    .provider
                    .unwrap_or_else(|| pending.provider.clone());
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
            self.publish();
            return Ok(());
        }
        let count = self
            .providers
            .lists
            .get(&pending.provider)
            .map_or(0, Vec::len);
        let name = self
            .providers
            .providers
            .iter()
            .find(|p| p.id == pending.provider)
            .map_or(pending.provider.as_str(), |p| p.name.as_str());
        let words = match &result.error {
            Some(error) => plainly(error, &pending.action),
            None => {
                result.text.clone().unwrap_or_else(|| match count {
                    1 => format!(
                        "Connected to {name}: 1 model available."
                    ),
                    _ => format!(
                        "Connected to {name}: {count} models \
                         available."
                    ),
                })
            }
        };
        // A request no chief made, such as the refresh at start, does
        // not settle a check under way; the check's own result does.
        let checking =
            self.env.settings.link.state == LinkState::Testing;
        if pending.from.is_some() || !checking {
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
        }
        if let Some((client, id)) = &pending.from {
            if pending.settles == Settles::Catalogue {
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
                    reply(id, Ok::<_, Rejection>(words)),
                )?;
            }
        }
        self.publish();
        Ok(())
    }

    fn named_model(&mut self, name: &str) -> Done<(String, String)> {
        if name.is_empty() {
            return Ok(Err(Rejection::NoDefaultModel));
        }
        let Some((provider, id)) = name
            .split_once("::")
            .filter(|(p, m)| !p.is_empty() && !m.is_empty())
        else {
            return Ok(Err(Rejection::NoEndpointNamed {
                name: name.into(),
            }));
        };
        if !self.provider_views()?.iter().any(|p| p.id == provider) {
            return Ok(Err(Rejection::NoSuchEndpoint {
                name: name.into(),
                endpoint: provider.into(),
            }));
        }
        let listed =
            self.providers.lists.get(provider).is_some_and(|list| {
                list.iter().any(|m| Listing::of(m).id == id)
            });
        if !listed && self.env.settings.model != name {
            return Ok(Err(Rejection::NoModel {
                model: name.into(),
            }));
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
        let mut settings = crate::realm::settings::stored(db)?;
        settings.link = hamlet_protocol::client::Link {
            state,
            models: count,
            words,
        };
        crate::realm::settings::keep(
            db,
            &settings,
            crate::app::clock::now(),
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
                    && ["chatgpt", "openrouter", "claude"]
                        .contains(&&*p.id)
                {
                    methods.push(LoginMethod::Subscription);
                }
                let label = match &*p.id {
                    "chatgpt" => "Continue with ChatGPT".into(),
                    "openrouter" => "Sign in with OpenRouter".into(),
                    "claude" => "Sign in to Claude".into(),
                    _ => p.name.clone(),
                };
                let configured =
                    self.credentials.model(&p.id, &serving).is_some();
                ProviderView {
                    configured,
                    limits: (configured || self.providers.faux)
                        .then(|| self.providers.limits.view(&p.id))
                        .flatten(),
                    methods,
                    label,
                    id: p.id,
                    name: p.name,
                }
            })
            .collect())
    }

    /// Takes the models `provider` lists into the catalogue, keeping
    /// a chief's own prices where it lists none, and lets go of those
    /// it no longer lists.
    fn take_models(
        &mut self,
        provider: &str,
        list: &[Value],
    ) -> Result<()> {
        let db = self.realm.store().connection();
        let mut cards = catalogue(db)?;
        let prefix = format!("{provider}::");
        let list = list.iter().map(Listing::of).collect::<Vec<_>>();
        cards.retain(|name, _| {
            name.strip_prefix(&prefix).is_none_or(|id| {
                list.iter().any(|model| model.id == id)
            })
        });
        for model in &list {
            let name = format!("{provider}::{}", model.id);
            let card = cards.entry(name.clone()).or_insert(Card {
                context: None,
                default_effort: None,
                efforts: Vec::new(),
                fast: false,
                images: None,
                images_yours: false,
                name,
                note: String::new(),
                price: None,
                priced: ListedPriced::Unknown,
                title: String::new(),
            });
            card.take(model);
        }
        keep_catalogue(db, &cards)
    }

    /// The catalogue, as a read of it answers.
    pub(crate) fn catalogue_read(
        &mut self,
        id: &RequestId,
    ) -> Result<ServerMessage> {
        Ok(ServerMessage::Read(ReadResult::Models {
            to: id.clone(),
            models: self.models_listed()?,
        }))
    }

    /// The models of the catalogue the chief can reach.
    pub(crate) fn models_listed(&mut self) -> Result<Vec<Listed>> {
        let db = self.realm.store().connection();
        // Only what the chief can reach: a provider signed in to or
        // given a key, an endpoint of the settings, and the default
        // model whatever serves it.
        let settings = &self.env.settings;
        let serving = settings.provider().unwrap_or_default();
        let reached = |provider: &str| {
            self.providers.faux
                || settings
                    .endpoints
                    .iter()
                    .any(|e| e.name == provider)
                || self.credentials.model(provider, serving).is_some()
        };
        Ok(catalogue(db)?
            .into_values()
            .filter(|model| {
                model.name == settings.model
                    || model.name.split_once("::").is_some_and(
                        |(provider, _)| reached(provider),
                    )
            })
            .map(|card| {
                let (provider, id) =
                    card.name.split_once("::").unwrap_or_default();
                let takes =
                    card.takes(&self.providers.listed(provider, id));
                card.listed(takes)
            })
            .collect())
    }

    /// Sets what a chief says of the model `name` in the catalogue.
    pub(crate) fn edit_catalogue(
        &mut self,
        name: &str,
        set: EditCatalogueSet,
    ) -> Done<String> {
        let bad =
            set.price.as_ref().and_then(|p| p.as_ref()).is_some_and(
                |p| {
                    [p.input, p.output]
                        .iter()
                        .any(|n| !n.is_finite() || *n < 0.0)
                },
            );
        if bad {
            return Ok(Err(Rejection::BadCost));
        }
        let empty = set
            .images
            .as_ref()
            .and_then(|images| images.as_ref())
            .is_some_and(|takes| {
                [takes.max_width, takes.max_height, takes.per_line]
                    .contains(&Some(0))
                    || takes.max_bytes == Some(0)
            });
        if empty {
            return Ok(Err(Rejection::BadImageLimit));
        }
        let db = self.realm.store().connection();
        let mut cards = catalogue(db)?;
        let Some(card) = cards.get_mut(name) else {
            return Ok(Err(Rejection::NoModel {
                model: name.into(),
            }));
        };
        if !card.priced.settable()
            && (set.price.is_some() || set.images.is_some())
        {
            return Ok(Err(Rejection::ModelListed {
                model: name.into(),
            }));
        }
        if let Some(note) = set.note {
            card.note = self.credentials.scrub_text(&note);
        }
        if let Some(images) = set.images {
            card.images_yours = images.is_some();
            card.images = images.map(Some);
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

/// A price of `rate` dollars per million tokens of every kind.
fn flat_price(rate: f64) -> Price {
    Price {
        input: rate,
        output: rate,
        cache_read: rate,
        cache_write: rate,
    }
}

/// The rejection of a sign-in reply or cancel that is not the
/// connection's own open one.
fn no_sign_in() -> Rejection {
    Rejection::NoSignIn
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

/// A model's entry in the catalogue.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Card {
    context: Option<i64>,
    #[serde(default)]
    default_effort: Option<Effort>,
    #[serde(default)]
    efforts: Vec<Effort>,
    #[serde(default)]
    fast: bool,
    /// The images it takes, none when it takes none; unset in a card
    /// kept before models were queried about images, which then
    /// takes the images its provider lists.
    #[serde(
        default,
        deserialize_with = "set",
        skip_serializing_if = "Option::is_none"
    )]
    images: Option<Option<ImageInput>>,
    /// Set when a chief said what it takes of images, which a
    /// listing leaves alone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    images_yours: bool,
    name: String,
    note: String,
    price: Option<ListedPrice>,
    priced: ListedPriced,
    /// Its provider's name for it.
    #[serde(default)]
    title: String,
}

impl Card {
    /// Takes in what pi says of `model`: what it is called, the
    /// efforts it can be set to, its default among them, whether
    /// its API has a fast tier, and what it takes of images.
    fn learn(&mut self, model: &Listing) {
        model.name.trim().clone_into(&mut self.title);
        if !self.images_yours {
            self.images = Some(model.images());
        }
        self.efforts = if model.reasoning {
            let levels = model.thinking_level_map.as_ref();
            Effort::ALL
                .into_iter()
                .filter(|effort| {
                    match levels.and_then(|map| {
                        map.get(<&'static str>::from(effort))
                    }) {
                        Some(Value::Null) => false,
                        Some(_) => true,
                        None => !matches!(
                            effort,
                            Effort::XHigh | Effort::Max
                        ),
                    }
                })
                .collect()
        } else {
            vec![Effort::Off]
        };
        self.default_effort =
            Some(Effort::Medium.clamped(&self.efforts));
        self.fast = matches!(
            model.api.as_deref(),
            Some("openai-responses" | "openai-codex-responses")
        );
    }

    /// Takes in its provider's listing `model`. A price it lists
    /// replaces a chief's, with the images it lists (S30); with none,
    /// a chief's price stays.
    fn take(&mut self, model: &Listing) {
        self.learn(model);
        let context = model.listed_context.or(model.context_window);
        if let Some(context) =
            context.and_then(|context| i64::try_from(context).ok())
        {
            self.context = Some(context);
        }
        let price = model.shown_price();
        if price.is_some() {
            self.price = price;
            self.priced = ListedPriced::Listed;
            self.images_yours = false;
            self.images = Some(model.images());
        } else if matches!(
            self.priced,
            ListedPriced::Unknown | ListedPriced::Assigned
        ) {
            (self.price, self.priced) =
                if is_local(model.base_url.as_deref()) {
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

    /// The images the model takes: as the card says, else as
    /// `listed`, its provider's listing of it, does.
    fn takes(&self, listed: &Listing) -> Option<ImageInput> {
        match &self.images {
            Some(images) => images.clone(),
            None => listed.images(),
        }
    }

    fn listed(self, takes: Option<ImageInput>) -> Listed {
        Listed {
            context: self.context,
            default_effort: self.default_effort.map(Effort::wire),
            efforts: self
                .efforts
                .into_iter()
                .map(Effort::wire)
                .collect(),
            fast: self.fast,
            images: takes,
            name: self.name,
            note: self.note,
            price: self.price,
            priced: self.priced,
            title: self.title,
        }
    }
}

/// A value that is there, even when it is null.
fn set<'de, D, T>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// The model catalogue: each model's card, by its name.
type Catalogue = BTreeMap<String, Card>;

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
            document::at.eq(crate::app::clock::now()),
        ))
        .execute(db)?;
    Ok(())
}

/// Tells whether `base_url` is served on this machine.
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
fn plainly(error: &str, action: &ProviderRequestAction) -> String {
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
        return "That key was not accepted. Check it was copied \
                whole and is still active."
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
    if *action == ProviderRequestAction::Hello {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_kept_before_images_takes_what_its_provider_lists() {
        let old = r#"{"context":null,"name":"orchard::wren","note":"",
            "price":null,"priced":"unknown"}"#;
        let card = serde_json::from_str::<Card>(old).unwrap();
        assert_eq!(card.images, None);
        let sees = Listing::of(
            &serde_json::json!({"input": ["text", "image"]}),
        );
        let blind =
            Listing::of(&serde_json::json!({"input": ["text"]}));
        assert!(card.takes(&sees).is_some());
        assert_eq!(card.takes(&blind), None);

        // A card that says it takes none keeps saying it.
        let none = r#"{"context":null,"images":null,"name":"x::y",
            "note":"","price":null,"priced":"unknown"}"#;
        let card = serde_json::from_str::<Card>(none).unwrap();
        assert_eq!(card.images, Some(None));
        assert_eq!(card.takes(&sees), None);
    }

    #[test]
    fn a_listed_price_replaces_a_chiefs_price_and_images() {
        let yours = r#"{"context":null,"images":{"maxWidth":10},
            "images_yours":true,"name":"x::y","note":"",
            "price":{"input":1.0,"output":2.0},"priced":"yours"}"#;
        let mut card = serde_json::from_str::<Card>(yours).unwrap();
        card.take(&Listing::of(
            &serde_json::json!({"input": ["text"]}),
        ));
        assert_eq!(card.priced, ListedPriced::Yours);
        assert!(card.images_yours);

        card.take(&Listing::of(&serde_json::json!({
            "input": ["text", "image"],
            "cost": {"input": 3.0, "output": 4.0},
        })));
        assert_eq!(card.priced, ListedPriced::Listed);
        assert_eq!(
            card.price,
            Some(ListedPrice {
                input: 3.0,
                output: 4.0
            })
        );
        assert!(!card.images_yours);
        assert_eq!(card.images, Some(Some(ImageInput::default())));
    }
}
