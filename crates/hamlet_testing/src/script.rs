//! The steps of a faux script: the answers a server gives in a
//! model's place.

use serde::Serialize;
use serde_json::Value;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    fill: Option<String>,
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

    /// The same answer, each `{{name}}` in its calls' arguments
    /// replaced by its words in the JSON object of `file` as the
    /// answer is asked for: for what is made only once the server
    /// runs, such as a hamlet's id. A relative path is from the
    /// script's folder.
    pub fn filled_from(self, file: &str) -> Step {
        self.with(|answer| answer.fill = Some(file.into()))
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
