//! The realm's settings, as the chief saved them, and what the
//! process puts over them.

use anyhow::Result;
use bevy_ecs::resource::Resource;
use diesel::prelude::*;
use hamlet_protocol::client::{
    EditSettingsSet, Link, LinkState, Newcomers,
};
use hamlet_protocol::runtime::ModelRef;
use hamlet_store::Db;
use hamlet_store::ids::ChiefId;
use hamlet_store::schema::document;
use serde::{Deserialize, Serialize};

use crate::core::Core;
use crate::realm::change::{Done, reject};
use crate::realm::credentials::WorkspaceAddress;
use crate::society::agents::Effort;
use crate::society::city::CHARACTER_MOST;
use crate::society::rejection::Rejection;

/// The settings after this process's overrides.
#[derive(Resource, Clone, Debug)]
pub(crate) struct EffectiveSettings {
    pub settings: Settings,
    pub overrides: Overrides,
}

/// A model's endpoint by the name models are chosen through.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Endpoint {
    pub name: String,
    pub base_url: String,
}

/// When agents dream, and what a dream may cost.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Dreams {
    pub on: bool,
    /// The local hours in which dreams start, from and to.
    pub hours: (u8, u8),
    /// The most one dream may cost, in millionths of a dollar.
    pub cost: i64,
}

impl Default for Dreams {
    fn default() -> Self {
        Self {
            on: false,
            hours: (1, 6),
            cost: 200_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Settings {
    /// The realm's model, as `endpoint::model`; empty until the
    /// chief picks one.
    pub model: String,
    pub endpoints: Vec<Endpoint>,
    /// The answered turns after which an agent reflects; never for 0
    /// (C5).
    pub reflect_every: u64,
    /// Whether every agent's commands may reach the network (C19).
    pub allow_net: bool,
    /// How hard models think unless an agent's own effort says
    /// otherwise (H29a).
    pub effort: Option<Effort>,
    pub dreams: Dreams,
    /// Who lets a profile an agent made into the city.
    pub newcomers: Newcomers,
    /// The character a profile made blank starts from.
    pub character: String,
    /// How the last check of the model's provider went.
    #[serde(default = "untested")]
    pub link: Link,
    /// Whether the chief picked the realm's model, which ends setup.
    #[serde(alias = "saved")]
    pub picked: bool,
}

fn untested() -> Link {
    Link {
        models: None,
        state: LinkState::Untested,
        words: None,
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: String::new(),
            endpoints: Vec::new(),
            reflect_every: 10,
            allow_net: false,
            effort: None,
            dreams: Dreams::default(),
            newcomers: Newcomers::default(),
            character: CHARACTER.into(),
            link: untested(),
            picked: false,
        }
    }
}

/// The character a profile made blank starts from, until a chief
/// gives the realm its own.
pub(crate) const CHARACTER: &str =
    "You are a profile new to this realm. You know nothing of your \
     own yet: use the library's skills, learn from the work you are \
     given, and keep what is worth keeping in your memory so that \
     it lasts.";

impl Settings {
    /// The endpoint the realm's model is served by.
    pub fn provider(&self) -> Option<&str> {
        self.model.split_once("::").map(|(provider, _)| provider)
    }

    /// The address of the endpoint `provider`, when the settings name
    /// one.
    pub fn base_url(&self, provider: &str) -> Option<&str> {
        self.endpoints
            .iter()
            .find(|e| e.name == provider)
            .map(|e| e.base_url.as_str())
    }
}

/// Keeps `settings` as the realm's, saved by `chief`.
pub(crate) fn keep(
    db: &mut Db,
    settings: &Settings,
    at: i64,
    chief: Option<ChiefId>,
) -> Result<()> {
    diesel::replace_into(document::table)
        .values((
            document::kind.eq("settings"),
            document::value.eq(serde_json::to_string(settings)?),
            document::at.eq(at),
            document::chief.eq(chief),
        ))
        .execute(db)?;
    Ok(())
}

/// The settings the chief saved, else the defaults.
pub(crate) fn stored(db: &mut Db) -> Result<Settings> {
    let text = document::table
        .find("settings")
        .select(document::value)
        .first::<String>(db)
        .optional()?;
    Ok(match text {
        Some(text) => serde_json::from_str(&text)?,
        None => Settings::default(),
    })
}

/// What the process puts over the saved settings, read once at start.
#[derive(Clone, Debug, Default)]
pub(crate) struct Overrides {
    pub faux: bool,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub reflect_every: Option<u64>,
    pub allow_net: Option<bool>,
}

impl Overrides {
    /// `HAMLET_MODEL`, `HAMLET_BASE_URL`, `HAMLET_REFLECT_EVERY` and
    /// `HAMLET_NET`.
    pub fn from_env(faux: bool) -> Self {
        let set =
            |name| std::env::var(name).ok().filter(|v| !v.is_empty());
        Self {
            faux,
            model: set("HAMLET_MODEL"),
            base_url: set("HAMLET_BASE_URL"),
            reflect_every: set("HAMLET_REFLECT_EVERY")
                .and_then(|v| v.parse().ok()),
            allow_net: set("HAMLET_NET")
                .map(|v| v == "1" || v == "true"),
        }
    }

    /// `settings` as this process runs them.
    pub fn apply(&self, mut settings: Settings) -> Settings {
        if let Some(model) = &self.model {
            settings.model.clone_from(model);
        }
        if let Some(every) = self.reflect_every {
            settings.reflect_every = every;
        }
        if let Some(net) = self.allow_net {
            settings.allow_net = net;
        }
        if let (Some(url), Some((endpoint, _))) =
            (&self.base_url, settings.model.clone().split_once("::"))
        {
            match settings
                .endpoints
                .iter_mut()
                .find(|kept| kept.name == endpoint)
            {
                Some(kept) => kept.base_url.clone_from(url),
                // A realm starts with no endpoint to point.
                None => settings.endpoints.push(Endpoint {
                    name: endpoint.into(),
                    base_url: url.clone(),
                }),
            }
        }
        settings
    }
}

/// The faux model every agent runs in a faux realm.
pub(crate) fn faux_model() -> ModelRef {
    ModelRef {
        base_url: None,
        id: "faux-1".into(),
        provider: "faux".into(),
    }
}

impl Core<'_, '_> {
    /// Keeps the settings `set` changes.
    pub(crate) fn edit_settings(
        &mut self,
        set: EditSettingsSet,
    ) -> Done<String> {
        let db = self.realm.store().connection();
        let mut settings = stored(db)?;
        if let Some(model) = set.model {
            settings.model = model;
            settings.picked = true;
        }
        if let Some(every) = set.reflect_every {
            let Ok(every) = u64::try_from(every) else {
                reject!(Rejection::BadReflectEvery);
            };
            settings.reflect_every = every;
        }
        if let Some(net) = set.allow_net {
            settings.allow_net = net;
        }
        if let Some(effort) = set.effort {
            settings.effort = effort.map(Effort::of);
        }
        if let Some(newcomers) = set.newcomers {
            settings.newcomers = newcomers;
        }
        if let Some(character) = set.character {
            if character.trim().is_empty() {
                reject!(Rejection::NoCharacter);
            }
            if character.chars().count() > CHARACTER_MOST {
                reject!(Rejection::LongCharacter);
            }
            settings.character = character;
        }
        let mut removed = Vec::new();
        if let Some(endpoints) = set.endpoints {
            let endpoints = endpoints
                .into_iter()
                .map(|e| Endpoint {
                    name: e.name.trim().to_owned(),
                    base_url: e.base_url.trim().to_owned(),
                })
                .collect::<Vec<_>>();
            if let Some(why) = endpoint_fault(&endpoints) {
                reject!(Rejection::BadEndpoint { why });
            }
            removed = settings
                .endpoints
                .iter()
                .filter(|old| {
                    !endpoints.iter().any(|e| e.name == old.name)
                })
                .map(|old| old.name.clone())
                .collect();
            settings.endpoints = endpoints;
        }
        let chosen = match set.workspace {
            None => None,
            Some(None) => Some(None),
            Some(Some(workspace)) => {
                let address = Endpoint {
                    name: "workspace".into(),
                    base_url: workspace.url.trim().to_owned(),
                };
                if let Some(why) =
                    endpoint_fault(std::slice::from_ref(&address))
                {
                    reject!(Rejection::BadEndpoint { why });
                }
                Some(Some(WorkspaceAddress {
                    url: address.base_url,
                    token: workspace.token,
                }))
            }
        };
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        keep(db, &settings, crate::app::clock::now(), Some(chief))?;
        for name in removed {
            self.credentials.save_model(&name, None)?;
        }
        if let Some(chosen) = chosen {
            self.credentials.choose_workspace(chosen)?;
        }
        self.env.settings = self.env.overrides.apply(settings);
        self.publish();
        Ok(Ok("The settings are saved.".into()))
    }
}

/// The fault of `endpoints`, in words.
fn endpoint_fault(endpoints: &[Endpoint]) -> Option<String> {
    for (at, endpoint) in endpoints.iter().enumerate() {
        if endpoint.name.is_empty() || endpoint.name.contains("::") {
            return Some(
                "An endpoint has a name without `::`.".into(),
            );
        }
        if endpoints[..at].iter().any(|e| e.name == endpoint.name) {
            return Some(format!(
                "Two endpoints are named `{}`.",
                endpoint.name
            ));
        }
        let Ok(url) = url::Url::parse(&endpoint.base_url) else {
            return Some(format!(
                "`{}` is not an address.",
                endpoint.base_url
            ));
        };
        if !matches!(url.scheme(), "http" | "https") {
            return Some(format!(
                "`{}` is not an http or https address.",
                endpoint.base_url
            ));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Some(
                "An address may not hold a name or a key; sign in \
                 to the endpoint with its key instead."
                    .into(),
            );
        }
    }
    None
}

/// The model `endpoint::model` names, with its endpoint's address.
pub(crate) fn model_ref(
    settings: &Settings,
    model: &str,
) -> ModelRef {
    let (endpoint, id) =
        model.split_once("::").unwrap_or(("", model));
    ModelRef {
        base_url: settings
            .endpoints
            .iter()
            .find(|e| e.name == endpoint)
            .map(|e| e.base_url.clone()),
        id: id.into(),
        provider: endpoint.into(),
    }
}
