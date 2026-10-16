//! The bazaar: the models OpenRouter offers and those every other
//! endpoint serves, with what is told of each, kept in
//! `<home>/bazaar.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::catalogue::{Listed, Model, Price};

/// The address OpenRouter lists its models at.
const OPENROUTER: &str = "https://openrouter.ai/api/v1";

/// The host of OpenRouter's endpoints.
const OPENROUTER_HOST: &str = "openrouter.ai";

/// The time the wares stay fresh before OpenRouter is asked again,
/// in milliseconds.
pub const FRESH: u64 = 24 * 60 * 60 * 1000;

/// The wait for OpenRouter to answer.
const ASK_TIMEOUT: Duration = Duration::from_secs(20);

/// One model on offer.
#[derive(
    Serialize, Deserialize, Clone, Debug, PartialEq, Default,
)]
#[serde(default)]
pub struct Ware {
    /// OpenRouter's id, as `maker/model`, or an endpoint's own.
    pub id: String,
    /// The name it is shown by.
    pub title: String,
    pub description: String,
    /// Its release, in seconds since 1970.
    pub created: Option<u64>,
    /// Dollars for a million tokens in, and out.
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub context: Option<u64>,
    pub max_output: Option<u64>,
    /// What it reads: `text`, `image`, `file`, `audio`, `video`.
    pub inputs: Vec<String>,
    pub tools: Option<bool>,
    pub reasoning: Option<bool>,
    /// Artificial Analysis's indexes of it.
    pub intelligence: Option<f64>,
    pub coding: Option<f64>,
    pub agentic: Option<f64>,
    /// The day it stops being served, as `YYYY-MM-DD`.
    pub expires: Option<String>,
}

impl Ware {
    /// The maker in its id, `anthropic` of `anthropic/claude`.
    pub fn maker(&self) -> &str {
        self.id.split_once('/').map_or("", |(maker, _)| maker)
    }

    pub fn price(&self) -> Option<Price> {
        Some(Price {
            input: self.input?,
            output: self.output?,
        })
    }

    /// True when it reads pictures.
    pub fn sees(&self) -> bool {
        self.inputs.iter().any(|input| input == "image")
    }

    /// It as an endpoint's list tells of a model.
    pub fn listed(&self) -> Listed {
        Listed {
            id: self.id.clone(),
            price: self.price(),
            context: self.context,
        }
    }

    /// Writes what it tells of itself into `model` of the catalogue.
    pub fn describe(&self, model: &mut Model) {
        model.title.clone_from(&self.title);
        model.tools = self.tools;
        model.inputs.clone_from(&self.inputs);
    }
}

impl From<&Listed> for Ware {
    fn from(listed: &Listed) -> Self {
        Ware {
            id: listed.id.clone(),
            title: listed.id.clone(),
            input: listed.price.map(|price| price.input),
            output: listed.price.map(|price| price.output),
            context: listed.context,
            ..Ware::default()
        }
    }
}

/// The wares as their file keeps them.
#[derive(
    Serialize, Deserialize, Clone, Debug, PartialEq, Default,
)]
#[serde(default)]
pub struct Wares {
    /// The time OpenRouter last answered, in milliseconds since
    /// 1970.
    pub fetched: u64,
    pub openrouter: Vec<Ware>,
    /// What each other endpoint serves, by the endpoint's name.
    pub served: BTreeMap<String, Vec<Ware>>,
}

/// The path of the wares of `home`.
pub fn path(home: &Path) -> PathBuf {
    home.join("bazaar.json")
}

impl Wares {
    /// The wares of `home`, none when the file is missing or cannot
    /// be read.
    pub fn load(home: &Path) -> Wares {
        std::fs::read_to_string(path(home))
            .ok()
            .and_then(|text| {
                serde_json::from_str::<Wares>(&text).ok()
            })
            .unwrap_or_default()
    }

    pub fn save(&self, home: &Path) -> Result<()> {
        let text = serde_json::to_string(self)?;
        std::fs::write(path(home), text).with_context(|| {
            format!("writing {}", path(home).display())
        })
    }

    /// True when OpenRouter should be asked again at `now`, in
    /// milliseconds since 1970.
    pub fn stale(&self, now: u64) -> bool {
        now.saturating_sub(self.fetched) >= FRESH
    }

    /// The ware `id` of OpenRouter, or of the endpoint `source`.
    pub fn get(
        &self,
        source: Option<&str>,
        id: &str,
    ) -> Option<&Ware> {
        let wares = match source {
            None => &self.openrouter,
            Some(endpoint) => self.served.get(endpoint)?,
        };
        wares.iter().find(|ware| ware.id == id)
    }
}

/// True when `base_url` is OpenRouter's.
pub fn is_openrouter(base_url: &str) -> bool {
    crate::config::host(base_url) == OPENROUTER_HOST
}

/// Dollars for a million tokens, from OpenRouter's price of one
/// token. A price below nothing is one that varies by the call.
fn per_million(v: &Value) -> Option<f64> {
    let each = v
        .as_f64()
        .or_else(|| v.as_str()?.trim().parse::<f64>().ok())?;
    // To a millionth of a dollar, past which a product of floats has
    // only noise.
    (each >= 0.0).then_some((each * 1e12).round() / 1e6)
}

/// The wares in OpenRouter's answer to a request for its models.
pub(crate) fn read(body: &Value) -> Vec<Ware> {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let index = |m: &Value, key: &str| {
        m["benchmarks"]["artificial_analysis"][key].as_f64()
    };
    body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m["id"].as_str()?.to_string();
            let params = m["supported_parameters"].as_array();
            let can = |what: &str| {
                params.map(|all| all.iter().any(|p| p == what))
            };
            Some(Ware {
                title: m["name"].as_str().unwrap_or(&id).to_string(),
                id,
                description: text(&m["description"]),
                created: m["created"].as_u64(),
                input: per_million(&m["pricing"]["prompt"]),
                output: per_million(&m["pricing"]["completion"]),
                context: m["context_length"].as_u64(),
                max_output: m["top_provider"]
                    ["max_completion_tokens"]
                    .as_u64(),
                inputs: m["architecture"]["input_modalities"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
                tools: can("tools"),
                reasoning: can("reasoning"),
                intelligence: index(m, "intelligence_index"),
                coding: index(m, "coding_index"),
                agentic: index(m, "agentic_index"),
                expires: m["expiration_date"]
                    .as_str()
                    .map(str::to_string),
            })
        })
        .collect()
}

/// Asks OpenRouter for every model it offers. It needs no key.
pub async fn fetch() -> Result<Vec<Ware>> {
    let body = ask(&format!("{OPENROUTER}/models")).await?;
    Ok(read(&body))
}

async fn ask(url: &str) -> Result<Value> {
    let client =
        reqwest::Client::builder().timeout(ASK_TIMEOUT).build()?;
    let resp = client
        .get(url)
        .send()
        .await
        .context("could not reach OpenRouter")?;
    let status = resp.status();
    let body = resp.json::<Value>().await.unwrap_or_default();
    anyhow::ensure!(
        status.is_success(),
        "HTTP {status}: {}",
        body["error"]
    );
    Ok(body)
}

/// One provider that serves a ware through OpenRouter.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Stall {
    pub provider: String,
    /// The precision it runs the model at, as `fp8`, where told.
    pub quantization: Option<String>,
    /// The share of calls it answered over the last day, out of 100.
    pub uptime: Option<f64>,
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub context: Option<u64>,
    pub max_output: Option<u64>,
}

/// The stalls in OpenRouter's answer to a request for a model's
/// endpoints.
pub(crate) fn read_stalls(body: &Value) -> Vec<Stall> {
    body["data"]["endpoints"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| Stall {
            provider: e["provider_name"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            quantization: e["quantization"]
                .as_str()
                .filter(|q| *q != "unknown")
                .map(str::to_string),
            uptime: e["uptime_last_1d"].as_f64(),
            input: per_million(&e["pricing"]["prompt"]),
            output: per_million(&e["pricing"]["completion"]),
            context: e["context_length"].as_u64(),
            max_output: e["max_completion_tokens"].as_u64(),
        })
        .collect()
}

/// Asks OpenRouter which providers serve the ware `id`, and how
/// well.
pub async fn stalls(id: &str) -> Result<Vec<Stall>> {
    let body =
        ask(&format!("{OPENROUTER}/models/{id}/endpoints")).await?;
    Ok(read_stalls(&body))
}

/// The order wares are shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Newest,
    Cheapest,
    Roomiest,
    Smartest,
    Name,
}

impl Sort {
    pub const ALL: [Sort; 5] = [
        Sort::Newest,
        Sort::Cheapest,
        Sort::Roomiest,
        Sort::Smartest,
        Sort::Name,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Sort::Newest => "Newest",
            Sort::Cheapest => "Cheapest",
            Sort::Roomiest => "Largest context",
            Sort::Smartest => "Smartest",
            Sort::Name => "Name",
        }
    }
}

/// What a ware must be to be shown.
#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    /// Words each of which its id or title holds.
    pub words: String,
    /// It calls tools, which every agent needs.
    pub tools: bool,
    pub reasoning: bool,
    pub sees: bool,
    /// The most dollars for a million tokens in.
    pub most_input: Option<f64>,
    pub least_context: Option<u64>,
    pub maker: Option<String>,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            words: String::new(),
            tools: true,
            reasoning: false,
            sees: false,
            most_input: None,
            least_context: None,
            maker: None,
        }
    }
}

impl Filter {
    /// True when `ware` is to be shown. What nobody said of a ware
    /// does not keep it out, but a price or a size asked for does.
    pub fn admits(&self, ware: &Ware) -> bool {
        let words = self.words.to_lowercase();
        let named =
            format!("{} {}", ware.id, ware.title).to_lowercase();
        words.split_whitespace().all(|word| named.contains(word))
            && (!self.tools || ware.tools != Some(false))
            && (!self.reasoning || ware.reasoning != Some(false))
            && (!self.sees || ware.inputs.is_empty() || ware.sees())
            && self.most_input.is_none_or(|most| {
                ware.input.is_some_and(|input| input <= most)
            })
            && self.least_context.is_none_or(|least| {
                ware.context.is_some_and(|context| context >= least)
            })
            && self
                .maker
                .as_deref()
                .is_none_or(|maker| ware.maker() == maker)
    }
}

/// The `wares` that `filter` admits, in the order `sort` gives. What
/// nobody said of comes last.
pub fn shown<'a>(
    wares: &'a [Ware],
    filter: &Filter,
    sort: Sort,
) -> Vec<&'a Ware> {
    let mut shown = wares
        .iter()
        .filter(|ware| filter.admits(ware))
        .collect::<Vec<_>>();
    // Lowest first, with nothing after every number.
    let rising = |v: Option<f64>| v.unwrap_or(f64::INFINITY);
    let falling = |v: Option<f64>| -v.unwrap_or(f64::NEG_INFINITY);
    shown.sort_by(|a, b| {
        let by =
            |key: &dyn Fn(&Ware) -> f64| key(a).total_cmp(&key(b));
        match sort {
            Sort::Newest => {
                by(&|w| falling(w.created.map(|c| c as f64)))
            }
            Sort::Cheapest => by(&|w| rising(w.input))
                .then(by(&|w| rising(w.output))),
            Sort::Roomiest => {
                by(&|w| falling(w.context.map(|c| c as f64)))
            }
            Sort::Smartest => by(&|w| falling(w.intelligence)),
            Sort::Name => std::cmp::Ordering::Equal,
        }
        .then_with(|| {
            a.title.to_lowercase().cmp(&b.title.to_lowercase())
        })
    });
    shown
}

/// Every maker of `wares`, once each, in order.
pub fn makers(wares: &[Ware]) -> Vec<String> {
    let mut makers = wares
        .iter()
        .map(Ware::maker)
        .filter(|maker| !maker.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    makers.sort();
    makers.dedup();
    makers
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// OpenRouter's answer for three models, cut down from a real
    /// one.
    fn answer() -> Value {
        json!({"data": [
            {
                "id": "anthropic/claude-haiku-5.5",
                "name": "Anthropic: Claude Haiku 5.5",
                "created": 1791397883u64,
                "description": "Small and fast.",
                "context_length": 1000000,
                "architecture": {
                    "input_modalities": ["text", "image", "file"],
                    "output_modalities": ["text"]
                },
                "pricing": {"prompt": "0.0000001", "completion": "0.0000005"},
                "top_provider": {"context_length": 1000000,
                                 "max_completion_tokens": 128000},
                "supported_parameters": ["reasoning", "tools", "max_tokens"],
                "expiration_date": null,
                "benchmarks": {"artificial_analysis":
                    {"intelligence_index": 43.4, "coding_index": null}}
            },
            {
                "id": "someone/chatty",
                "name": "Someone: Chatty",
                "created": 1700000000u64,
                "context_length": 8192,
                "architecture": {"input_modalities": ["text"]},
                "pricing": {"prompt": "0", "completion": "0"},
                "supported_parameters": ["max_tokens"],
                "expiration_date": "2026-12-31"
            },
            {
                "id": "openrouter/auto",
                "name": "Auto Router",
                "pricing": {"prompt": "-1", "completion": "-1"},
                "supported_parameters": ["tools"]
            },
            {"name": "no id"}
        ]})
    }

    #[test]
    fn openrouter_s_answer_is_read_into_wares() {
        let wares = read(&answer());
        assert_eq!(wares.len(), 3);
        let haiku = &wares[0];
        assert_eq!(haiku.maker(), "anthropic");
        assert_eq!(haiku.title, "Anthropic: Claude Haiku 5.5");
        assert_eq!(haiku.max_output, Some(128_000));
        assert_eq!(
            (haiku.tools, haiku.reasoning),
            (Some(true), Some(true))
        );
        assert!(haiku.sees());
        assert_eq!(haiku.intelligence, Some(43.4));
        assert_eq!(haiku.coding, None);
        let price = haiku.price().unwrap();
        assert!((price.input - 0.1).abs() < 1e-9);
        assert!((price.output - 0.5).abs() < 1e-9);
        // Free is a price; one that varies by the call is none.
        let chatty = &wares[1];
        assert_eq!(chatty.input, Some(0.0));
        assert_eq!(chatty.tools, Some(false));
        assert_eq!(chatty.expires.as_deref(), Some("2026-12-31"));
        assert_eq!(wares[2].price(), None);
    }

    #[test]
    fn a_filter_keeps_out_what_an_agent_cannot_use() {
        let wares = read(&answer());
        let ids = |filter: &Filter, sort| {
            shown(&wares, filter, sort)
                .iter()
                .map(|ware| ware.id.as_str())
                .collect::<Vec<_>>()
        };
        // By default, only those that call tools.
        let tools = Filter::default();
        assert_eq!(
            ids(&tools, Sort::Newest),
            ["anthropic/claude-haiku-5.5", "openrouter/auto"]
        );
        let all = Filter {
            tools: false,
            ..Filter::default()
        };
        assert_eq!(ids(&all, Sort::Cheapest)[0], "someone/chatty");
        assert_eq!(ids(&all, Sort::Cheapest)[2], "openrouter/auto");
        assert_eq!(
            ids(&all, Sort::Smartest)[0],
            "anthropic/claude-haiku-5.5"
        );
        let cheap = Filter {
            most_input: Some(0.05),
            ..all.clone()
        };
        assert_eq!(ids(&cheap, Sort::Newest), ["someone/chatty"]);
        let roomy = Filter {
            least_context: Some(100_000),
            ..all.clone()
        };
        assert_eq!(
            ids(&roomy, Sort::Newest),
            ["anthropic/claude-haiku-5.5"]
        );
        let words = Filter {
            words: "HAIKU anthropic".into(),
            ..all.clone()
        };
        assert_eq!(ids(&words, Sort::Name).len(), 1);
        let maker = Filter {
            maker: Some("someone".into()),
            ..all
        };
        assert_eq!(ids(&maker, Sort::Name), ["someone/chatty"]);
        assert_eq!(
            makers(&wares),
            ["anthropic", "openrouter", "someone"]
        );
    }

    #[test]
    fn an_endpoint_s_model_is_shown_though_little_is_told_of_it() {
        let listed = Listed {
            id: "qwen3:8b".into(),
            price: None,
            context: None,
        };
        let ware = Ware::from(&listed);
        let picky = Filter {
            reasoning: true,
            sees: true,
            ..Filter::default()
        };
        assert!(picky.admits(&ware));
        let priced = Filter {
            most_input: Some(1.0),
            ..Filter::default()
        };
        assert!(!priced.admits(&ware));
    }

    #[test]
    fn wares_come_back_from_their_file_as_they_were() {
        let home = hamlet_testing::Scratch::new("bazaar-file");
        let mut wares = Wares {
            fetched: 5,
            openrouter: read(&answer()),
            ..Wares::default()
        };
        wares.served.insert("local".into(), vec![Ware::default()]);
        wares.save(&home).unwrap();
        assert_eq!(Wares::load(&home), wares);
        assert!(wares.stale(5 + FRESH));
        assert!(!wares.stale(4 + FRESH));
    }

    #[test]
    fn a_model_s_stalls_are_read_with_their_uptime() {
        let body = json!({"data": {"endpoints": [
            {"provider_name": "Google", "quantization": "unknown",
             "uptime_last_1d": 99.5,
             "pricing": {"prompt": "0.000001", "completion": "0.000002"},
             "context_length": 1000, "max_completion_tokens": 100},
            {"provider_name": "Small", "quantization": "fp8"}
        ]}});
        let stalls = read_stalls(&body);
        assert_eq!(stalls[0].quantization, None);
        assert_eq!(stalls[0].uptime, Some(99.5));
        assert!((stalls[0].input.unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(stalls[1].quantization.as_deref(), Some("fp8"));
        assert!(is_openrouter("https://openrouter.ai/api/v1"));
        assert!(!is_openrouter("http://localhost:11434/v1"));
    }
}
