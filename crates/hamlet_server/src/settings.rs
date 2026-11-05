//! The realm's settings, as the chief saved them, and what the
//! process puts over them.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_protocol::client::{Link, LinkState, Newcomers};
use hamlet_protocol::runtime::ModelRef;
use hamlet_store::Db;
use hamlet_store::ids::ChiefId;
use hamlet_store::schema::document;
use serde::{Deserialize, Serialize};

use crate::society::hires::Effort;

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
    /// The realm's model, as `endpoint::model`.
    pub model: String,
    pub endpoints: Vec<Endpoint>,
    /// The answered turns after which an agent reflects; never for 0
    /// (C5).
    pub reflect_every: u64,
    /// Whether every agent's commands may reach the network (C19).
    pub allow_net: bool,
    /// How hard models think unless a hire's own effort says
    /// otherwise (H29a).
    pub effort: Option<Effort>,
    pub dreams: Dreams,
    /// Who lets a scholar an agent made into the city.
    pub newcomers: Newcomers,
    /// The character a scholar made blank starts from.
    pub character: String,
    /// How the last check of the model's provider went.
    #[serde(default = "untested")]
    pub link: Link,
    /// Whether the chief saved them.
    pub saved: bool,
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
            model: "local::qwen3.6:35b-a3b".into(),
            endpoints: vec![Endpoint {
                name: "local".into(),
                base_url: "http://localhost:11434/v1".into(),
            }],
            reflect_every: 10,
            allow_net: false,
            effort: None,
            dreams: Dreams::default(),
            newcomers: Newcomers::default(),
            character: CHARACTER.into(),
            link: untested(),
            saved: false,
        }
    }
}

/// The character a scholar made blank starts from, until a chief
/// gives the realm its own.
pub(crate) const CHARACTER: &str =
    "You are a scholar new to this realm. You know nothing of your \
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

/// What the process puts over the saved settings, read once at
/// start: the environment's overrides, and faux models in place of
/// every model.
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
            for kept in &mut settings.endpoints {
                if kept.name == endpoint {
                    kept.base_url.clone_from(url);
                }
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
