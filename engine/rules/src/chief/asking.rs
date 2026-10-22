//! The questions an agent puts to the chief with `ask_chief`: read
//! from the call and checked, set out as words, and answered.

use serde_json::{Value, json};

/// The most questions one call may ask.
pub const MOST_QUESTIONS: usize = 10;

/// The most options one question may offer.
pub const MOST_OPTIONS: usize = 6;

/// An answer a question offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub title: String,
    /// Empty for none.
    pub description: String,
}

/// One question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    pub question: String,
    /// Empty for none.
    pub description: String,
    pub options: Vec<Offer>,
    /// The chief may answer in words of their own.
    pub text_input: bool,
}

/// What one call asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Questions {
    /// A line over the questions. Empty for none.
    pub message: String,
    pub questions: Vec<Question>,
}

/// The chief's answer to one question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// The option at this place.
    Offer(usize),
    Words(String),
}

/// The words at `key` of `value`, trimmed. Empty when there are none.
fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().trim().to_string()
}

/// The questions in the arguments of an `ask_chief` call. `Err` says
/// what is wrong with them, for the agent to put right.
pub fn read(args: &str) -> Result<Questions, String> {
    let args = serde_json::from_str::<Value>(args)
        .map_err(|_| "the arguments are not JSON".to_string())?;
    let asked = match &args["questions"] {
        Value::Array(asked) => asked.clone(),
        // A call of the form before there were several questions.
        Value::Null if !text(&args, "question").is_empty() => {
            vec![json!({"question": text(&args, "question")})]
        }
        Value::Null => return Err("say what in `questions`".into()),
        _ => return Err("`questions` is a list".into()),
    };
    if asked.is_empty() {
        return Err("say what in `questions`".into());
    }
    if asked.len() > MOST_QUESTIONS {
        return Err(format!(
            "ask at most {MOST_QUESTIONS} questions at once"
        ));
    }
    let questions = asked
        .iter()
        .enumerate()
        .map(|(at, asked)| question(at + 1, asked))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Questions {
        message: text(&args, "message"),
        questions,
    })
}

/// The question `n`, counted from one.
fn question(n: usize, asked: &Value) -> Result<Question, String> {
    let question = text(asked, "question");
    if question.is_empty() {
        return Err(format!("question {n} has no `question`"));
    }
    let offered = match &asked["options"] {
        Value::Null => Vec::new(),
        Value::Array(offered) => offered.clone(),
        _ => {
            return Err(format!(
                "the `options` of question {n} are a list"
            ));
        }
    };
    if offered.len() > MOST_OPTIONS {
        return Err(format!(
            "question {n} offers more than {MOST_OPTIONS} options"
        ));
    }
    let mut options = Vec::<Offer>::new();
    for offer in &offered {
        let (title, description) = match offer {
            Value::String(title) => {
                (title.trim().to_string(), String::new())
            }
            offer => {
                (text(offer, "title"), text(offer, "description"))
            }
        };
        if title.is_empty() {
            return Err(format!(
                "an option of question {n} has no `title`"
            ));
        }
        if options.iter().any(|o| o.title == title) {
            return Err(format!(
                "question {n} offers `{title}` twice"
            ));
        }
        options.push(Offer { title, description });
    }
    // A question with nothing to pick is answered in words.
    let text_input = options.is_empty()
        || asked["text_input"].as_bool().unwrap_or(true);
    Ok(Question {
        question,
        description: text(asked, "description"),
        options,
        text_input,
    })
}

/// The letter of the option at `at`.
pub fn letter(at: usize) -> char {
    (b'a' + at as u8) as char
}

impl Questions {
    /// The questions as words, for a chief who reads them as text.
    /// One plain question is its own words.
    pub fn about(&self) -> String {
        if let [only] = self.questions.as_slice()
            && self.message.is_empty()
            && only.description.is_empty()
            && only.options.is_empty()
        {
            return only.question.clone();
        }
        let mut lines = Vec::new();
        if !self.message.is_empty() {
            lines.push(self.message.clone());
        }
        let numbered = self.questions.len() > 1;
        for (at, q) in self.questions.iter().enumerate() {
            lines.push(match numbered {
                true => format!("{}. {}", at + 1, q.question),
                false => q.question.clone(),
            });
            if !q.description.is_empty() {
                lines.push(format!("   {}", q.description));
            }
            for (at, offer) in q.options.iter().enumerate() {
                lines.push(match offer.description.is_empty() {
                    true => {
                        format!("   {}) {}", letter(at), offer.title)
                    }
                    false => format!(
                        "   {}) {} - {}",
                        letter(at),
                        offer.title,
                        offer.description
                    ),
                });
            }
            if q.text_input && !q.options.is_empty() {
                lines.push("   or in your own words".into());
            }
        }
        lines.join("\n")
    }

    /// The answer the agent is given for `picks`, one for each
    /// question in order: the chief's reply, each question quoted
    /// over its answer. `Err` says why they do not answer them.
    pub fn answer(&self, picks: &[Pick]) -> Result<String, String> {
        if picks.len() != self.questions.len() {
            return Err("every question needs an answer".into());
        }
        let answers = self
            .questions
            .iter()
            .zip(picks)
            .enumerate()
            .map(|(at, (q, pick))| match pick {
                Pick::Offer(offer) => q
                    .options
                    .get(*offer)
                    .map(|o| o.title.clone())
                    .ok_or_else(|| {
                        format!(
                            "question {} has no such option",
                            at + 1
                        )
                    }),
                Pick::Words(_) if !q.text_input => Err(format!(
                    "question {} takes an option",
                    at + 1
                )),
                Pick::Words(words) if words.trim().is_empty() => {
                    Err(format!("question {} needs words", at + 1))
                }
                Pick::Words(words) => Ok(words.trim().to_string()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let body = self
            .questions
            .iter()
            .zip(&answers)
            .map(|(q, answer)| {
                let asked =
                    q.question.split_whitespace().collect::<Vec<_>>();
                format!("> {}\n{answer}", asked.join(" "))
            })
            .collect::<Vec<_>>();
        Ok(format!("{ANSWERS}\n{}", body.join("\n\n")))
    }
}

/// The head of every answer the chief gives to `ask_chief`.
pub const ANSWERS: &str = "The chief answers:";

/// The chief's own words in a result of `ask_chief`. `None` when the
/// chief did not answer.
pub fn said(result: &str) -> Option<&str> {
    result
        .strip_prefix(ANSWERS)
        .map(str::trim)
        .filter(|words| !words.is_empty())
}
