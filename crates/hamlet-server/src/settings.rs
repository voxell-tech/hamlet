//! Saved settings and process-only model overrides.

use anyhow::{Result, bail};
use hamlet_protocol::v0::EditSettingsSet;
use serde_json::{Value, json};
use url::Url;

use crate::credentials::Workspace;
use crate::protocol::Refusal;

pub(crate) fn defaults() -> Value {
    json!({"model":"local::qwen3.6:35b-a3b", "reflectEvery":10, "allowNet":false,
        "endpoints":[{"name":"local", "baseUrl":"http://localhost:11434/v1"}],
        "link":{"state":"untested", "models":null, "words":null}, "saved":false})
}

pub(crate) struct Overrides {
    model: Option<String>,
    base_url: Option<String>,
    reflect_every: Option<u64>,
    allow_net: Option<bool>,
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
        Ok(Self {
            model: if faux {
                Some("faux::faux-1".into())
            } else {
                get("HAMLET_MODEL")
            },
            base_url: get("HAMLET_BASE_URL"),
            reflect_every,
            allow_net: get("HAMLET_NET").map(|v| v == "1"),
        })
    }

    pub fn apply(&self, mut settings: Value) -> Value {
        if let Some(model) = &self.model {
            settings["model"] = model.clone().into();
        }
        if let Some(every) = self.reflect_every {
            settings["reflectEvery"] = every.into();
        }
        if let Some(net) = self.allow_net {
            settings["allowNet"] = net.into();
        }
        let provider = settings["model"]
            .as_str()
            .and_then(|m| m.split_once("::"))
            .map(|(p, _)| p.to_owned());
        if let Some(url) = &self.base_url {
            for endpoint in settings["endpoints"]
                .as_array_mut()
                .into_iter()
                .flatten()
            {
                if endpoint["name"].as_str() == provider.as_deref() {
                    endpoint["baseUrl"] = url.clone().into();
                }
            }
        }
        settings
    }
}

pub(crate) fn validate(
    set: &EditSettingsSet,
) -> std::result::Result<Option<Option<Workspace>>, Refusal> {
    let invalid = |text: &str| Refusal::Invalid(text.into());
    let workspace = match &set.workspace {
        None => None,
        Some(None) => Some(None),
        Some(Some(workspace)) => {
            let address =
                Url::parse(&workspace.url).ok().filter(|u| {
                    (u.scheme() == "https"
                        || (u.scheme() == "http"
                            && matches!(
                                u.host_str(),
                                Some(
                                    "localhost"
                                        | "127.0.0.1"
                                        | "[::1]"
                                )
                            )))
                        && u.username().is_empty()
                        && u.password().is_none()
                        && u.query().is_none()
                        && u.fragment().is_none()
                        && u.port() != Some(0)
                });
            if address.is_none() {
                return Err(invalid("The workspace address must start with https://, or http:// for localhost, and have no password or query."));
            }
            if workspace.token.encode_utf16().count() < 16 {
                return Err(invalid(
                    "The workspace token needs at least 16 \
                     characters.",
                ));
            }
            Some(Some(Workspace {
                url: workspace.url.clone(),
                token: workspace.token.clone(),
            }))
        }
    };
    if set
        .reflect_every
        .is_some_and(|v| !(0..=9_007_199_254_740_991).contains(&v))
    {
        return Err(invalid("reflectEvery must be a whole number"));
    }
    let mut names = Vec::new();
    let builtins = serde_json::from_str::<Vec<String>>(include_str!(
        "../assets/provider-names.json"
    ))
    .expect("bundled provider names");
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
