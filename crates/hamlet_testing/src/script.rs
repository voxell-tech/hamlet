//! Faux scripts: the answers a server gives in a model's place.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

/// A faux script, written as the JSON file a server reads with
/// `--faux`. The format is in `hamlet_acceptance`'s README.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    models: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    responses: Vec<Step>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    agents: BTreeMap<String, Vec<Step>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    errands: Option<Vec<Step>>,
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

impl Script {
    /// A script that answers nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The faux provider's model ids; `faux-1` when absent.
    pub fn models(mut self, ids: &[&str]) -> Self {
        self.models = ids.iter().map(|id| (*id).into()).collect();
        self
    }

    /// Answers for any conversation without a list of its own, in
    /// order.
    pub fn responses(
        mut self,
        steps: impl IntoIterator<Item = Step>,
    ) -> Self {
        self.responses.extend(steps);
        self
    }

    /// Answers for the agent `hamlet:number`, in order.
    pub fn agent(
        mut self,
        agent: &str,
        steps: impl IntoIterator<Item = Step>,
    ) -> Self {
        self.agents.entry(agent.into()).or_default().extend(steps);
        self
    }

    /// Answers for one-off conversations: ballots, reviews, merged
    /// notes and the library's calls, in order.
    pub fn errands(
        mut self,
        steps: impl IntoIterator<Item = Step>,
    ) -> Self {
        self.errands.get_or_insert_default().extend(steps);
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

/// One answer of a faux script.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum Step {
    Words(String),
    Answer(Answer),
}

/// An answer with more than words.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    calls: Vec<Call>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stall_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hold_until: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct Call {
    name: String,
    arguments: Value,
}

/// An answer of words alone.
pub fn words(text: &str) -> Step {
    Step::Words(text.into())
}

/// An answer calling one tool with `arguments`, a JSON object.
pub fn call(name: &str, arguments: Value) -> Step {
    calls([(name, arguments)])
}

/// An answer calling several tools at once.
pub fn calls<'a>(
    each: impl IntoIterator<Item = (&'a str, Value)>,
) -> Step {
    Step::Answer(Answer {
        calls: each
            .into_iter()
            .map(|(name, arguments)| Call {
                name: name.into(),
                arguments,
            })
            .collect(),
        ..Answer::default()
    })
}

/// A model's failure with `message`.
pub fn failure(message: &str) -> Step {
    Step::Answer(Answer {
        error: Some(message.into()),
        ..Answer::default()
    })
}

impl Step {
    /// The same answer, given only once `file` exists. A relative
    /// path is from the script's folder.
    pub fn held_until(self, file: &str) -> Step {
        self.with(|answer| answer.hold_until = Some(file.into()))
    }

    /// The same answer, taking `ms` milliseconds however the request
    /// is stopped.
    pub fn stalled(self, ms: u64) -> Step {
        self.with(|answer| answer.stall_ms = Some(ms))
    }

    /// The same answer, thinking `thought` first.
    pub fn thinking(self, thought: &str) -> Step {
        self.with(|answer| answer.thinking = Some(thought.into()))
    }

    fn with(self, change: impl FnOnce(&mut Answer)) -> Step {
        let mut answer = match self {
            Step::Words(text) => Answer {
                text: Some(text),
                ..Answer::default()
            },
            Step::Answer(answer) => answer,
        };
        change(&mut answer);
        Step::Answer(answer)
    }
}
