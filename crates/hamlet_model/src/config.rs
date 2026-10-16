//! Settings in `<home>/config.toml`, and the tests of a connection.

pub(crate) mod keys;
pub mod providers;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use keys::{load_key, save_key};
use serde::{Deserialize, Serialize};

/// The wait for the model nobody chose to answer a first word. One on
/// this machine may first have to be loaded.
const ANSWER_WAIT: Duration = Duration::from_secs(90);

/// The settings of a home.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    /// The model that runs where nobody chose one, by the name it
    /// goes by: `endpoint::model`.
    pub model: String,
    /// User turns between forced reflections; 0 disables.
    pub reflect_every: usize,
    /// Network access for tools.
    pub allow_net: bool,
    /// The places models are served. None of them stands apart.
    pub endpoints: Vec<Endpoint>,
    /// The bazaar's server, where agents, hamlets and plugins are
    /// bought and sold. `HAMLET_BAZAAR_URL` overrides it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bazaar_url: Option<String>,
}

/// A place models are served.
#[derive(
    Serialize, Deserialize, Clone, Debug, PartialEq, Default,
)]
#[serde(default)]
pub struct Endpoint {
    /// Its name in the names of its models. Lowercase letters,
    /// digits and hyphens.
    pub name: String,
    /// OpenAI-compatible, e.g. `http://localhost:11434/v1`.
    pub base_url: String,
    /// Served on the chief's ChatGPT plan, by a sign-in in place of
    /// a key.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub chatgpt: bool,
    #[serde(skip)]
    pub api_key: Option<String>,
}

impl Endpoint {
    /// The key it is called with. For one on a ChatGPT plan, the
    /// access token of the sign-in, renewed first when near its end.
    pub async fn key(&self, home: &Path) -> Result<Option<String>> {
        if self.chatgpt {
            return crate::chatgpt::access_token(home)
                .await
                .map(Some);
        }
        Ok(self.api_key.clone())
    }
}

/// True when `name` can be an endpoint's.
pub fn names_endpoint(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
        })
}

/// The host in `base_url`, with no scheme, port or path.
pub fn host(base_url: &str) -> &str {
    let rest = base_url.split("://").nth(1).unwrap_or(base_url);
    let host = rest.split(['/', '?']).next().unwrap_or_default();
    match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => host.rsplit_once(':').map_or(host, |(host, _)| host),
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            model: "local::qwen3.6:35b-a3b".into(),
            reflect_every: 10,
            allow_net: false,
            endpoints: vec![Endpoint {
                name: "local".into(),
                base_url: "http://localhost:11434/v1".into(),
                chatgpt: false,
                api_key: None,
            }],
            bazaar_url: None,
        }
    }
}

fn config_path(home: &Path) -> PathBuf {
    home.join("config.toml")
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

impl Config {
    /// The endpoint called `name`.
    pub fn endpoint(&self, name: &str) -> Option<&Endpoint> {
        self.endpoints.iter().find(|endpoint| endpoint.name == name)
    }

    /// The endpoint that serves the model nobody chose.
    pub fn default_endpoint(&self) -> Option<&Endpoint> {
        let (name, _) = crate::catalogue::parts(&self.model)?;
        self.endpoint(name)
    }

    /// Every key the endpoints have, to keep out of what is
    /// written down.
    pub fn keys(&self) -> Vec<String> {
        self.endpoints
            .iter()
            .filter_map(|endpoint| endpoint.api_key.clone())
            .collect()
    }

    /// Lets `HAMLET_MODEL` name the model nobody chose, and
    /// `HAMLET_BASE_URL` say where the endpoint that serves it is.
    fn overridden(mut self) -> Config {
        if let Some(v) = env("HAMLET_MODEL") {
            self.model = v;
        }
        let served = crate::catalogue::parts(&self.model)
            .map(|(name, _)| name);
        let endpoint = self
            .endpoints
            .iter_mut()
            .find(|endpoint| Some(&*endpoint.name) == served);
        if let Some((endpoint, v)) =
            endpoint.zip(env("HAMLET_BASE_URL"))
        {
            endpoint.base_url = v;
        }
        self
    }

    /// Reads the file, then the keys, then lets `HAMLET_*` env vars
    /// override. `HAMLET_BASE_URL` and `HAMLET_API_KEY` are of the
    /// endpoint that serves the model nobody chose.
    pub fn load(home: &Path) -> Result<Config> {
        let path = config_path(home);
        let mut c = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str::<Config>(&text)
                .with_context(|| {
                    format!("parsing {}", path.display())
                })?,
            Err(_) => Config::default(),
        };
        c.endpoints
            .retain(|endpoint| names_endpoint(&endpoint.name));
        for endpoint in &mut c.endpoints {
            endpoint.api_key = load_key(home, &endpoint.name);
        }
        let mut c = c.overridden();
        let served = c
            .default_endpoint()
            .map(|endpoint| endpoint.name.clone());
        if let Some((name, v)) = served.zip(env("HAMLET_API_KEY")) {
            c.endpoints
                .iter_mut()
                .filter(|endpoint| endpoint.name == name)
                .for_each(|endpoint| {
                    endpoint.api_key = Some(v.clone())
                });
        }
        if let Some(v) = env("HAMLET_REFLECT_EVERY") {
            c.reflect_every = v.parse().with_context(|| {
                format!(
                    "HAMLET_REFLECT_EVERY must be a whole number, \
                     not `{v}`"
                )
            })?;
        }
        if let Some(v) = env("HAMLET_NET") {
            c.allow_net = v == "1";
        }
        Ok(c)
    }

    /// True when the home at `home` has settings of its own.
    pub fn saved(home: &Path) -> bool {
        config_path(home).exists()
    }

    /// Writes the settings, and each key where keys are kept.
    pub fn save(&self, home: &Path) -> Result<()> {
        let path = config_path(home);
        std::fs::write(&path, toml::to_string_pretty(self)?)
            .with_context(|| format!("writing {}", path.display()))?;
        for endpoint in &self.endpoints {
            save_key(
                home,
                &endpoint.name,
                endpoint.api_key.as_deref(),
            )?;
        }
        Ok(())
    }

    /// The address the endpoint `name` of the home at `home` is
    /// served at, read from its settings with no key. `None` for one
    /// the settings do not have.
    pub fn place(home: &Path, name: &str) -> Option<String> {
        let text = std::fs::read_to_string(config_path(home));
        let config = text
            .ok()
            .and_then(|text| toml::from_str::<Config>(&text).ok())
            .unwrap_or_default()
            .overridden();
        config
            .endpoints
            .into_iter()
            .find(|endpoint| endpoint.name == name)
            .map(|endpoint| endpoint.base_url)
    }

    /// Forgets the key of an endpoint that was taken out.
    pub fn forget_key(home: &Path, endpoint: &str) {
        let _ = save_key(home, endpoint, None);
    }

    /// Puts `endpoint` among the endpoints and returns the name it
    /// has there. One at the same address, signed in to the same
    /// way, is given the key and keeps its name, and a name another
    /// has is numbered.
    pub fn settle(&mut self, endpoint: Endpoint) -> String {
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
            .expect("the numbers do not end");
        self.endpoints.push(Endpoint {
            name: name.clone(),
            ..endpoint
        });
        name
    }
}

/// Lists the models of the endpoint that serves the model nobody
/// chose, by name, which also proves its URL and key work.
pub async fn test_connection(
    c: &Config,
    home: &Path,
) -> Result<Vec<String>> {
    let endpoint = c.default_endpoint().with_context(|| {
        format!(
            "the default model `{}` names no endpoint the settings \
             have",
            c.model
        )
    })?;
    let key = endpoint.key(home).await?;
    let listed =
        crate::catalogue::list(&endpoint.base_url, key.as_deref())
            .await;
    let listed = match endpoint.chatgpt {
        true => listed.context("ChatGPT did not list its models")?,
        false => listed?,
    };
    Ok(listed.into_iter().map(|model| model.id).collect())
}

/// Asks the model nobody chose for a word, which proves that it
/// answers a chat.
pub async fn try_model(c: &Config, home: &Path) -> Result<()> {
    use crate::provider::Provider;
    let hello = [serde_json::json!({
        "role": "user",
        "content": "Say hello in one word.",
    })];
    let endpoints = crate::provider::Endpoints::new(c, home);
    let mut unheard = |_: crate::provider::Delta<'_>| {};
    let asked = endpoints.complete(&hello, &[], None, &mut unheard);
    tokio::time::timeout(ANSWER_WAIT, asked)
        .await
        .context("the model did not answer in time")??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Only the file side: save/load also touch the real OS keychain.
    #[test]
    fn toml_never_contains_the_key() {
        let mut c = Config {
            model: "local::m".into(),
            reflect_every: 3,
            ..Config::default()
        };
        c.endpoints[0].api_key = Some("sk-secret".into());
        let text = toml::to_string_pretty(&c).unwrap();
        assert!(!text.contains("sk-secret"));
        let loaded = toml::from_str::<Config>(&text).unwrap();
        c.endpoints[0].api_key = None;
        assert_eq!(loaded, c);
        assert_eq!(
            toml::from_str::<Config>("model = \"x\"")
                .unwrap()
                .endpoints,
            Config::default().endpoints
        );
    }

    #[test]
    fn a_settled_endpoint_keeps_the_name_of_one_at_its_address() {
        let mut config = Config::default();
        let ollama = providers::served(&providers::LOCAL[0]);
        assert_eq!(config.settle(ollama), "local");
        assert_eq!(config.endpoints.len(), 1);

        // The same name at another address is another endpoint.
        let far = |key: &str| Endpoint {
            name: "local".into(),
            base_url: "https://far.example/v1/".into(),
            chatgpt: false,
            api_key: Some(key.into()),
        };
        assert_eq!(config.settle(far("first")), "local-2");
        // Settled again, it is the same one with the new key.
        assert_eq!(config.settle(far("second")), "local-2");
        assert_eq!(config.endpoints.len(), 2);
        assert_eq!(
            config.endpoints[1].api_key.as_deref(),
            Some("second")
        );
    }
}
