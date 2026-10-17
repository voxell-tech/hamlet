//! The models a home may run, in `<home>/models.toml`: those the
//! chief added, where each is served, what it costs and whether the
//! chief allows it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::Config;

/// Price of a model served on this machine for a million tokens until
/// the chief says otherwise, in dollars.
pub(crate) const LOCAL_PRICE: f64 = 0.10;

/// Price a model served elsewhere is taken to have for a million
/// tokens while nobody knows its price and its provider bills no
/// call.
pub(crate) const ASSUMED_PRICE: f64 = 1.00;

/// The wait for an endpoint to list its models.
const LIST_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(10);

/// The mark between an endpoint's name and a model's in the name a
/// model goes by.
const BETWEEN: &str = "::";

/// A model's price for a million tokens, in dollars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
}

impl Price {
    /// The cost of `input` and `output` tokens, in dollars.
    pub fn of(self, input: u64, output: u64) -> f64 {
        (input as f64 * self.input + output as f64 * self.output)
            / 1e6
    }
}

/// The source of a model's price.
#[derive(
    Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default,
)]
#[serde(rename_all = "lowercase")]
pub enum Priced {
    /// Nobody has said.
    #[default]
    Unknown,
    /// The endpoint's own list said so.
    Listed,
    /// Given because the model runs on this machine.
    Assigned,
    /// The chief wrote it.
    Yours,
}

/// One model of the catalogue.
#[derive(
    Serialize, Deserialize, Clone, Debug, PartialEq, Default,
)]
#[serde(default)]
pub struct Model {
    /// The endpoint that serves it, by name.
    pub endpoint: String,
    /// The endpoint's name for it.
    pub id: String,
    /// Dollars for a million tokens in, and out.
    pub input: f64,
    pub output: f64,
    pub priced: Priced,
    /// The most tokens it holds at once, where known.
    pub context: Option<u64>,
    /// True when any hamlet of the home may run it.
    pub allowed: bool,
    /// Its use, in the chief's words. The mayor is shown it when it
    /// chooses.
    pub note: String,
    /// The name it is shown by, where the bazaar told it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    /// What it reads: `text`, `image`, `file`, `audio`, `video`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
}

impl Model {
    /// The name it goes by in the records and in a mayor's choice:
    /// its endpoint's, then its own.
    pub fn name(&self) -> String {
        named(&self.endpoint, &self.id)
    }

    pub fn price(&self) -> Option<Price> {
        (self.priced != Priced::Unknown).then_some(Price {
            input: self.input,
            output: self.output,
        })
    }

    /// One line for whoever chooses a model: what it costs, how
    /// much it holds and what it is for.
    pub fn card(&self) -> String {
        let mut card = format!("`{}`", self.name());
        match self.price() {
            Some(price) => card.push_str(&format!(
                ": ${:.2} in and ${:.2} out for a million tokens",
                price.input, price.output
            )),
            None => card.push_str(": price not known"),
        }
        if let Some(context) = self.context {
            card.push_str(&format!(", holds {context} tokens"));
        }
        if self.inputs.iter().any(|input| input == "image") {
            card.push_str(", reads images");
        }
        if self.tools == Some(false) {
            card.push_str(", cannot call tools");
        }
        if !self.note.trim().is_empty() {
            card.push_str(&format!(". {}", self.note.trim()));
        }
        card
    }
}

/// The name a model `id` of `endpoint` goes by. One of no endpoint
/// has only its own, and is run nowhere.
pub fn named(endpoint: &str, id: &str) -> String {
    if endpoint.is_empty() {
        id.to_string()
    } else {
        format!("{endpoint}{BETWEEN}{id}")
    }
}

/// The endpoint and the model's own name in `name`, or nothing
/// for a name that tells of no endpoint.
pub fn parts(name: &str) -> Option<(&str, &str)> {
    name.split_once(BETWEEN)
}

/// True when models at `base_url` run on this machine.
pub fn is_local(base_url: &str) -> bool {
    let host = crate::config::host(base_url);
    matches!(host, "localhost" | "::1" | "0.0.0.0")
        || host.starts_with("127.")
}

/// A model as an endpoint's list tells of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub id: String,
    pub price: Option<Price>,
    pub context: Option<u64>,
}

/// The models in an endpoint's answer to a request for its list, each
/// with its price and size where the answer gives them.
pub(crate) fn read_list(body: &Value) -> Vec<Listed> {
    let number = |v: &Value| {
        v.as_f64()
            .or_else(|| v.as_str()?.trim().parse::<f64>().ok())
    };
    // Most answer with `data`, a few with `models` or a bare list,
    // and a few name a model by `slug`, `name` or `model`.
    let models = [&body["data"], &body["models"], body]
        .into_iter()
        .find_map(Value::as_array);
    let mut listed = models
        .into_iter()
        .flatten()
        // A ChatGPT plan lists models it keeps from pickers too.
        .filter(|m| {
            m["visibility"].as_str().is_none_or(|v| v == "list")
        })
        .filter_map(|m| {
            let per_token = |key: &str| {
                number(&m["pricing"][key]).filter(|p| *p >= 0.0)
            };
            let price =
                per_token("prompt").zip(per_token("completion"));
            Some(Listed {
                id: ["id", "slug", "name", "model"]
                    .into_iter()
                    .find_map(|key| m[key].as_str())?
                    .to_string(),
                price: price.map(|(input, output)| Price {
                    input: input * 1e6,
                    output: output * 1e6,
                }),
                context: ["context_length", "context_window"]
                    .into_iter()
                    .find_map(|key| m[key].as_u64()),
            })
        })
        .collect::<Vec<_>>();
    listed.sort_by(|a, b| a.id.cmp(&b.id));
    listed
}

/// Asks the endpoint at `base_url` for its models, which also
/// proves the address and the key work.
pub async fn list(
    base_url: &str,
    key: Option<&str>,
) -> Result<Vec<Listed>> {
    let client =
        reqwest::Client::builder().timeout(LIST_TIMEOUT).build()?;
    let mut req = client
        .get(format!("{}/models", base_url.trim_end_matches('/')));
    if let Some(key) = key {
        req = req.bearer_auth(key);
    }
    let resp =
        req.send().await.context("could not reach the endpoint")?;
    let status = resp.status();
    let body = resp.json::<Value>().await.unwrap_or_default();
    anyhow::ensure!(
        status.is_success(),
        "HTTP {status}: {}",
        body["error"]
    );
    Ok(read_list(&body))
}

/// Every model the home knows of.
#[derive(
    Serialize, Deserialize, Clone, Debug, Default, PartialEq,
)]
pub struct Catalogue {
    #[serde(default, rename = "model")]
    pub models: Vec<Model>,
}

/// The path of the catalogue of `home`.
pub fn path(home: &Path) -> PathBuf {
    home.join("models.toml")
}

impl Catalogue {
    /// The catalogue of `home`, empty when it is missing or cannot be
    /// read.
    pub fn load(home: &Path) -> Catalogue {
        std::fs::read_to_string(path(home))
            .ok()
            .and_then(|text| toml::from_str::<Catalogue>(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, home: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path(home), text).with_context(|| {
            format!("writing {}", path(home).display())
        })
    }

    pub fn get(&self, name: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.name() == name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Model> {
        self.models.iter_mut().find(|model| model.name() == name)
    }

    /// The models any hamlet may run, by name.
    pub fn allowed(&self) -> impl Iterator<Item = &Model> {
        self.models.iter().filter(|model| model.allowed)
    }

    /// Adds the model `found` of `endpoint`, allowed, or allows it
    /// again, with the price and size it is listed with. One with no
    /// price that runs on this machine (`local`) is given
    /// `LOCAL_PRICE`.
    pub fn add(
        &mut self,
        endpoint: &str,
        found: &Listed,
        local: bool,
    ) -> &mut Model {
        let name = named(endpoint, &found.id);
        if self.get(&name).is_none() {
            self.models.push(Model {
                endpoint: endpoint.to_string(),
                id: found.id.clone(),
                ..Model::default()
            });
            self.models.sort_by(|a, b| {
                (&a.endpoint, &a.id).cmp(&(&b.endpoint, &b.id))
            });
        }
        let model = self.get_mut(&name).expect("it is there now");
        model.allowed = true;
        model.refresh(found, local);
        model
    }

    /// Takes in what the endpoint `endpoint` lists. Each model of it
    /// that was added is given the price and size it is listed with,
    /// and one the endpoint no longer lists is dropped. So is one
    /// the chief neither allowed, priced nor described.
    pub fn take_in(
        &mut self,
        endpoint: &str,
        listed: &[Listed],
        local: bool,
    ) {
        let found =
            |id: &str| listed.iter().find(|found| found.id == id);
        for model in &mut self.models {
            if model.endpoint != endpoint {
                continue;
            }
            if let Some(found) = found(&model.id) {
                model.refresh(found, local);
            }
        }
        self.models.retain(|model| {
            let kept = model.allowed
                || model.priced == Priced::Yours
                || !model.note.trim().is_empty();
            model.endpoint != endpoint
                || (kept && found(&model.id).is_some())
        });
    }
}

impl Model {
    /// Takes the price and size `found` is listed with. A price the
    /// chief wrote stays.
    fn refresh(&mut self, found: &Listed, local: bool) {
        if found.context.is_some() {
            self.context = found.context;
        }
        if self.priced == Priced::Yours {
            return;
        }
        match found.price {
            Some(price) => {
                self.input = price.input;
                self.output = price.output;
                self.priced = Priced::Listed;
            }
            None if local && self.priced == Priced::Unknown => {
                self.input = LOCAL_PRICE;
                self.output = LOCAL_PRICE;
                self.priced = Priced::Assigned;
            }
            None => {}
        }
    }
}

/// The price of the model `name` in the home at `home`: its
/// catalogue's, or `LOCAL_PRICE` for one it does not tell of that
/// runs on this machine, or `ASSUMED_PRICE` for any other.
pub fn price(home: &Path, name: &str) -> Option<Price> {
    let known =
        Catalogue::load(home).get(name).and_then(Model::price);
    let local = || {
        let (endpoint, _) = parts(name)?;
        let at = Config::place(home, endpoint)?;
        Some(is_local(&at))
    };
    known.or_else(|| {
        let each = if local().unwrap_or(false) {
            LOCAL_PRICE
        } else {
            ASSUMED_PRICE
        };
        Some(Price {
            input: each,
            output: each,
        })
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_list_names_its_models_and_prices_those_it_can() {
        let body = json!({"data": [
            {"id": "vendor/large", "context_length": 200000,
             "pricing": {"prompt": "0.000003", "completion": "0.000015"}},
            {"id": "plain"},
            {"no": "id"},
        ]});
        let listed = read_list(&body);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "plain");
        assert_eq!(listed[0].price, None);
        let large = &listed[1];
        assert_eq!(large.context, Some(200_000));
        let price = large.price.unwrap();
        assert!((price.input - 3.0).abs() < 1e-9);
        assert!((price.output - 15.0).abs() < 1e-9);
        // A thousand tokens in and a thousand out.
        assert!((price.of(1000, 1000) - 0.018).abs() < 1e-9);
        // Not every server wraps its list the same way.
        let bare = json!([{"name": "tiny"}]);
        assert_eq!(read_list(&bare)[0].id, "tiny");
        let other = json!({"models": [{"model": "tiny"}]});
        assert_eq!(read_list(&other)[0].id, "tiny");
        // A ChatGPT plan's, with a model kept from pickers.
        let plan = json!({"models": [{"slug": "shown", "visibility": "list"},
            {"slug": "kept", "visibility": "hide"}]});
        let shown = read_list(&plan);
        assert_eq!((shown.len(), &*shown[0].id), (1, "shown"));
    }

    #[test]
    fn a_catalogue_keeps_what_the_chief_wrote_when_it_is_listed_again()
     {
        let mut all = Catalogue::default();
        let cheap = Price {
            input: 1.0,
            output: 2.0,
        };
        let listed = |price| {
            vec![
                Listed {
                    id: "large".into(),
                    price,
                    context: Some(8000),
                },
                Listed {
                    id: "small".into(),
                    price: None,
                    context: None,
                },
            ]
        };
        all.add("far", &listed(Some(cheap))[0], false);
        all.add("far", &listed(Some(cheap))[1], false);
        // Added, they are allowed and named by where they are
        // served.
        let large = all.get("far::large").unwrap();
        assert!(large.allowed);
        assert_eq!(large.price(), Some(cheap));
        assert_eq!(all.get("far::small").unwrap().price(), None);
        assert_eq!(parts("far::large"), Some(("far", "large")));
        assert_eq!(parts("qwen3:8b"), None);

        let large = all.get_mut("far::large").unwrap();
        large.note = "For hard questions.".into();
        large.input = 9.0;
        large.priced = Priced::Yours;
        all.take_in("far", &listed(Some(cheap)), false);
        let large = all.get("far::large").unwrap();
        assert!(large.allowed);
        assert_eq!(large.input, 9.0);
        assert!(large.card().contains("For hard questions."));
        assert_eq!(all.allowed().count(), 2);
        // One the endpoint no longer serves goes.
        all.take_in("far", &listed(Some(cheap))[..1], false);
        assert!(all.get("far::small").is_none());
        // Listing never adds one.
        all.take_in("near", &listed(None), true);
        assert!(all.get("near::small").is_none());

        // One on this machine costs a little, so its work counts.
        all.add("near", &listed(None)[1], true);
        let small = all.get("near::small").unwrap();
        assert_eq!(small.priced, Priced::Assigned);
        assert_eq!(small.input, LOCAL_PRICE);
        // A model a listing once put in, never allowed, priced or
        // described, goes at the next listing.
        all.models.push(Model {
            endpoint: "near".into(),
            id: "large".into(),
            ..Model::default()
        });
        all.take_in("near", &listed(None), true);
        assert!(all.get("near::large").is_none());
        assert!(all.get("near::small").is_some());
        // It comes back from its file as it was.
        let text = toml::to_string_pretty(&all).unwrap();
        assert_eq!(toml::from_str::<Catalogue>(&text).unwrap(), all);
    }

    #[test]
    fn a_model_on_this_machine_is_told_by_its_address() {
        for local in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:8080",
            "http://[::1]:1234/v1",
        ] {
            assert!(is_local(local), "{local}");
        }
        for far in
            ["https://openrouter.ai/api/v1", "http://10.0.0.2:11434"]
        {
            assert!(!is_local(far), "{far}");
        }
    }
}
