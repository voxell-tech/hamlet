//! Faux scripts: the answers a server gives in a model's place, in
//! the format `harnesses/pi/src/faux.ts` reads.

use std::collections::BTreeMap;
use std::path::Path;

use hamlet_testing::Step;
use serde::Serialize;

/// A faux script. An agent takes its answers from its own list, else
/// the conversation's kind (`worker`, `errand`, ...) from its list,
/// else from `responses`.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Faux {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    models: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    responses: Vec<Step>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    agents: BTreeMap<String, Vec<Step>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    kinds: BTreeMap<String, Vec<Step>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    price: Option<Price>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens_per_second: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
struct Price {
    input: f64,
    output: f64,
}

impl Faux {
    /// A script that answers nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The faux provider's model ids; `faux-1` when absent.
    pub fn models(mut self, ids: &[&str]) -> Self {
        self.models = ids.iter().map(|id| (*id).into()).collect();
        self
    }

    /// Answers for any conversation without a list of its own.
    pub fn responses(
        mut self,
        steps: impl IntoIterator<Item = Step>,
    ) -> Self {
        self.responses.extend(steps);
        self
    }

    /// Answers for the agent with id `agent`.
    pub fn agent(
        mut self,
        agent: u64,
        steps: impl IntoIterator<Item = Step>,
    ) -> Self {
        self.agents
            .entry(agent.to_string())
            .or_default()
            .extend(steps);
        self
    }

    /// Answers for the conversations of a kind: `worker`, `errand`,
    /// `letters`, `questions` or `dream`.
    pub fn kind(
        mut self,
        kind: &str,
        steps: impl IntoIterator<Item = Step>,
    ) -> Self {
        self.kinds.entry(kind.into()).or_default().extend(steps);
        self
    }

    /// Dollars per million tokens, input and output alike.
    pub fn price(mut self, dollars: f64) -> Self {
        self.price = Some(Price {
            input: dollars,
            output: dollars,
        });
        self
    }

    /// Streams each answer at `rate` tokens a second.
    pub fn tokens_per_second(mut self, rate: f64) -> Self {
        self.tokens_per_second = Some(rate);
        self
    }

    /// Writes the script to `file`.
    pub fn write(&self, file: &Path) {
        std::fs::write(
            file,
            serde_json::to_vec_pretty(self).unwrap(),
        )
        .unwrap();
    }
}
