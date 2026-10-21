//! Saved settings and process-only model overrides.

use anyhow::{Result, bail};
use hamlet_protocol::v0::{
    EditSettingsSet, Endpoint, Link, LinkState,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::credentials::Workspace;
use crate::protocol::{Refusable, Refusal};
use crate::service::{document, save};

/// The home's settings, as the chief saved them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    /// The model the home runs, as `endpoint::model`.
    pub model: String,
    /// The answered turns after which an agent reflects; none for 0.
    pub reflect_every: u64,
    pub allow_net: bool,
    pub endpoints: Vec<Endpoint>,
    /// How the last check of the model went.
    pub link: Link,
    /// Whether the chief saved them, rather than the defaults
    /// standing.
    pub saved: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: "local::qwen3.6:35b-a3b".into(),
            reflect_every: 10,
            allow_net: false,
            endpoints: vec![Endpoint {
                name: "local".into(),
                base_url: "http://localhost:11434/v1".into(),
            }],
            link: Link {
                state: LinkState::Untested,
                models: None,
                words: None,
            },
            saved: false,
        }
    }
}

impl Settings {
    /// The endpoint of the home's model.
    pub fn provider(&self) -> Option<&str> {
        self.model.split_once("::").map(|(provider, _)| provider)
    }

    /// The address of the endpoint `name`.
    pub fn base_url(&self, name: &str) -> Option<&str> {
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.name == name)
            .map(|endpoint| endpoint.base_url.as_str())
    }
}

/// The settings the chief saved, else the defaults.
pub(crate) fn stored(db: &Connection) -> Result<Settings> {
    Ok(document(db, "settings", "home")?
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default())
}

/// Keeps `settings` as the chief's.
pub(crate) fn keep(
    db: &Connection,
    settings: &Settings,
) -> Result<()> {
    save(db, "settings", "home", &serde_json::to_value(settings)?)
}

pub(crate) struct Overrides {
    model: Option<String>,
    base_url: Option<String>,
    reflect_every: Option<u64>,
    allow_net: Option<bool>,
    pub workspace: Option<Workspace>,
}

impl Overrides {
    pub fn from_env(faux: bool) -> Result<Self> {
        let get =
            |key| std::env::var(key).ok().filter(|v| !v.is_empty());
        let reflect_every = match get("HAMLET_REFLECT_EVERY") {
            Some(text) => match text.parse::<u64>() {
                Ok(n) => Some(n),
                Err(_) => bail!(
                    "HAMLET_REFLECT_EVERY must be a whole number, \
                     not `{text}`"
                ),
            },
            None => None,
        };
        let model = get("HAMLET_MODEL");
        if let Some(name) = &model
            && name
                .split_once("::")
                .is_none_or(|(p, id)| p.is_empty() || id.is_empty())
        {
            bail!("HAMLET_MODEL is `endpoint::id`, not `{name}`");
        }
        Ok(Self {
            model: if faux {
                Some("faux::faux-1".into())
            } else {
                model
            },
            base_url: get("HAMLET_BASE_URL"),
            reflect_every,
            allow_net: get("HAMLET_NET").map(|v| v == "1"),
            workspace: match (
                get("HAMLET_WORKSPACE_URL"),
                get("HAMLET_WORKSPACE_TOKEN"),
            ) {
                (None, None) => None,
                (Some(url), Some(token)) => {
                    let workspace = Workspace { url, token };
                    check_workspace(&workspace)
                        .map_err(anyhow::Error::msg)?;
                    Some(workspace)
                }
                _ => bail!(
                    "HAMLET_WORKSPACE_URL and \
                     HAMLET_WORKSPACE_TOKEN are set together, or \
                     neither is."
                ),
            },
        })
    }

    /// `settings` with the overrides, refused when `HAMLET_BASE_URL`
    /// is for an endpoint they do not list.
    pub fn apply(&self, mut settings: Settings) -> Result<Settings> {
        if let Some(model) = &self.model {
            settings.model.clone_from(model);
        }
        if let Some(every) = self.reflect_every {
            settings.reflect_every = every;
        }
        if let Some(net) = self.allow_net {
            settings.allow_net = net;
        }
        if let Some(url) = &self.base_url {
            let model = settings.model.clone();
            let Some(provider) =
                settings.provider().map(String::from)
            else {
                bail!(
                    "HAMLET_BASE_URL needs a model: set \
                     HAMLET_MODEL or choose one in Settings."
                );
            };
            let Some(endpoint) = settings
                .endpoints
                .iter_mut()
                .find(|endpoint| endpoint.name == provider)
            else {
                if builtins().contains(&provider) {
                    bail!(
                        "HAMLET_BASE_URL is for the endpoint of the \
                         model `{model}`, a provider pi knows the \
                         address of. Unset HAMLET_BASE_URL."
                    );
                }
                bail!(
                    "HAMLET_BASE_URL is for the endpoint of the \
                     model `{model}`, which the settings do not \
                     list. Add the endpoint in Settings, or unset \
                     HAMLET_BASE_URL."
                );
            };
            endpoint.base_url.clone_from(url);
        }
        Ok(settings)
    }
}

/// The names of pi's own providers.
fn builtins() -> Vec<String> {
    serde_json::from_str(include_str!(
        "../assets/provider-names.json"
    ))
    .expect("bundled provider names")
}

pub(crate) fn validate(
    set: &EditSettingsSet,
) -> Refusable<Option<Option<Workspace>>> {
    let invalid = |text: &str| Refusal::Invalid(text.into());
    let workspace = match &set.workspace {
        None => None,
        Some(None) => Some(None),
        Some(Some(workspace)) => {
            let connection = Workspace {
                url: workspace.url.clone(),
                token: workspace.token.clone(),
            };
            check_workspace(&connection).map_err(invalid)?;
            Some(Some(connection))
        }
    };
    if set.reflect_every.is_some_and(|every| every < 0) {
        return Err(invalid("reflectEvery must be a whole number"));
    }
    let mut names = Vec::new();
    let builtins = builtins();
    for endpoint in set.endpoints.iter().flatten() {
        let name = &endpoint.name;
        if name.is_empty()
            || !name.bytes().all(|b| {
                b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'-'
            })
            || builtins.contains(name)
            || names.contains(&name)
        {
            return Err(Refusal::Invalid(format!(
                "`{name}` is not a unique custom endpoint name."
            )));
        }
        names.push(name);
        let url = Url::parse(&endpoint.base_url)
            .map_err(|_| invalid("Invalid URL"))?;
        if !["http", "https"].contains(&url.scheme()) {
            return Err(invalid(
                "The address should start with https:// or http://.",
            ));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(invalid(
                "Keep credentials out of an endpoint's address.",
            ));
        }
    }
    Ok(workspace)
}

pub(crate) fn check_workspace(
    workspace: &Workspace,
) -> Result<(), &'static str> {
    let valid = Url::parse(&workspace.url).ok().is_some_and(|url| {
        (url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(
                    url.host_str(),
                    Some("localhost" | "127.0.0.1" | "[::1]")
                )))
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.port() != Some(0)
    });
    if !valid {
        return Err("The workspace address must start with https://, or http:// for localhost, and have no password or query.");
    }
    if workspace.token.chars().count() < 16 {
        return Err(
            "The workspace token needs at least 16 characters."
        );
    }
    Ok(())
}
