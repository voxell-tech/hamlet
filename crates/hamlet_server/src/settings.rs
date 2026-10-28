//! Saved settings and process-only model overrides.

use anyhow::{Result, bail};
use hamlet_protocol::client::{
    EditSettingsSet, Endpoint, Link, LinkState,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::credentials::WorkspaceAddress;
use crate::protocol::{Refusable, Refusal};
use crate::sandbox::Isolation;
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
    /// How hard agents' models think unless their own effort says
    /// otherwise; the model's own default when absent.
    #[serde(default)]
    pub effort: Option<hamlet_protocol::Effort>,
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
            effort: None,
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

/// The model a faux home runs, whatever its settings say.
const FAUX_MODEL: &str = "faux::faux-1";

/// What the process puts over the saved settings: the environment's
/// variables, read once when the server starts, the faux model in
/// place of the home's when its models are faux, and the isolation
/// the chief started it with.
#[derive(Clone)]
pub struct Overrides {
    /// Whether the home's models are faux.
    pub(crate) faux: bool,
    model: Option<String>,
    base_url: Option<String>,
    reflect_every: Option<u64>,
    allow_net: Option<bool>,
    pub(crate) workspace: Option<WorkspaceAddress>,
    pub(crate) isolation: Isolation,
}

impl Overrides {
    /// None from the environment, so a test's server is the same
    /// whatever its shell carries. `faux` still puts the faux model
    /// over the settings' one.
    #[cfg(test)]
    pub fn none(faux: bool) -> Self {
        Self {
            faux,
            model: faux.then(|| FAUX_MODEL.into()),
            base_url: None,
            reflect_every: None,
            allow_net: None,
            workspace: None,
            isolation: Isolation::TESTING,
        }
    }

    /// The overrides the environment gives, with the faux model over
    /// `HAMLET_MODEL` when `faux`.
    pub fn from_env(
        faux: bool,
        isolation: Isolation,
    ) -> Result<Self> {
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
            faux,
            model: faux.then(|| FAUX_MODEL.into()).or(model),
            base_url: get("HAMLET_BASE_URL"),
            reflect_every,
            allow_net: get("HAMLET_NET").map(|v| v == "1"),
            workspace: match (
                get("HAMLET_WORKSPACE_URL"),
                get("HAMLET_WORKSPACE_TOKEN"),
            ) {
                (None, None) => None,
                (Some(url), Some(token)) => {
                    let workspace = WorkspaceAddress { url, token };
                    check_workspace(&workspace)?;
                    Some(workspace)
                }
                _ => bail!(
                    "HAMLET_WORKSPACE_URL and \
                     HAMLET_WORKSPACE_TOKEN are set together, or \
                     neither is."
                ),
            },
            isolation,
        })
    }

    /// `settings` with the overrides, refused when `HAMLET_BASE_URL`
    /// is for an endpoint they do not list.
    pub(crate) fn apply(
        &self,
        mut settings: Settings,
    ) -> Result<Settings> {
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
) -> Refusable<Option<Option<WorkspaceAddress>>> {
    let invalid = |text: &str| Refusal::Invalid(text.into());
    let workspace = match &set.workspace {
        None => None,
        Some(None) => Some(None),
        Some(Some(workspace)) => {
            let connection = WorkspaceAddress {
                url: workspace.url.clone(),
                token: workspace.token.clone(),
            };
            check_workspace(&connection)
                .map_err(|bad| invalid(&bad.to_string()))?;
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

/// The fault in a workspace's address.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BadAddress {
    #[error(
        "The workspace address must start with https://, or http:// \
         for localhost, and have no password or query."
    )]
    Url,
    #[error("The workspace token needs at least 16 characters.")]
    ShortToken,
}

pub(crate) fn check_workspace(
    workspace: &WorkspaceAddress,
) -> Result<(), BadAddress> {
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
        return Err(BadAddress::Url);
    }
    if workspace.token.chars().count() < 16 {
        return Err(BadAddress::ShortToken);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_is_refused_for_its_address_or_its_token() {
        let check = |url: &str, token: &str| {
            check_workspace(&WorkspaceAddress {
                url: url.into(),
                token: token.into(),
            })
            .map_err(|bad| bad.to_string())
        };
        let token = "0123456789abcdef";
        assert_eq!(
            check("http://workspace.example", token),
            Err("The workspace address must start with https://, or \
                 http:// for localhost, and have no password or query."
                .into())
        );
        assert_eq!(
            check("http://localhost:8080", "short"),
            Err("The workspace token needs at least 16 characters."
                .into())
        );
        assert_eq!(check("https://workspace.example", token), Ok(()));
    }
}
