//! The questions an agent puts to the chief with `request_chief`:
//! read from the call and checked, set out as words, and answered.

use hamlet_protocol::client::{Offer, Pick, Question, Questions};
use serde::Deserialize;
use serde_json::Value;

/// The most questions one call may put.
pub(crate) const MOST_QUESTIONS: usize = 10;

/// The most options one question may offer.
pub(crate) const MOST_OPTIONS: usize = 6;

/// The head of every answer the chief gives to `request_chief`.
pub(crate) const ANSWERS: &str = "The chief answers:";

/// The arguments of `request_chief`, as the tool's schema has them.
#[derive(Deserialize)]
struct RequestChief {
    #[serde(default)]
    message: String,
    questions: Vec<RequestChiefQuestion>,
}

/// One question as the agent puts it.
#[derive(Deserialize)]
struct RequestChiefQuestion {
    question: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    options: Vec<RequestChiefOption>,
    /// Whether the chief may answer in words; pi's default is yes.
    text_input: Option<bool>,
}

/// One option: its title alone, or a title and a description.
#[derive(Deserialize)]
#[serde(
    untagged,
    expecting = "an option is a title, or an object with a `title` \
                 and a `description`"
)]
enum RequestChiefOption {
    Title(String),
    Described {
        title: String,
        #[serde(default)]
        description: String,
    },
}

/// The fault in a `request_chief` call's arguments, for the agent to
/// put right.
#[derive(Debug, thiserror::Error)]
pub(crate) enum InvalidQuestions {
    #[error("the arguments are not JSON")]
    NotJson,
    #[error("the arguments are not valid: {0}")]
    Invalid(serde_json::Error),
    #[error("say what in `questions`")]
    NoQuestions,
    #[error("put at most {most} questions at once", most = MOST_QUESTIONS)]
    TooMany,
    #[error("question {0} has no `question`")]
    Blank(usize),
    #[error(
        "question {0} offers more than {most} options",
        most = MOST_OPTIONS
    )]
    Crowded(usize),
    #[error("an option of question {0} has no `title`")]
    Untitled(usize),
    #[error("question {question} offers `{title}` twice")]
    Twice { question: usize, title: String },
}

/// The fault in picks that do not answer the questions they are for.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Unanswered {
    #[error("every question needs an answer")]
    Unequal,
    #[error("question {0} has no such option")]
    NoOption(usize),
    #[error("question {0} takes an option")]
    TakesOption(usize),
    #[error("question {0} needs words")]
    NoWords(usize),
}

/// The questions in the arguments of a `request_chief` call.
pub(crate) fn read(
    args: &str,
) -> Result<Questions, InvalidQuestions> {
    let args = serde_json::from_str::<Value>(args)
        .map_err(|_| InvalidQuestions::NotJson)?;
    let args = serde_json::from_value::<RequestChief>(args)
        .map_err(InvalidQuestions::Invalid)?;
    if args.questions.is_empty() {
        return Err(InvalidQuestions::NoQuestions);
    }
    if args.questions.len() > MOST_QUESTIONS {
        return Err(InvalidQuestions::TooMany);
    }
    let questions = args
        .questions
        .into_iter()
        .enumerate()
        .map(|(at, raw)| question(at + 1, raw))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Questions {
        message: args.message.trim().into(),
        questions,
    })
}

/// The question `n`, counted from one.
fn question(
    n: usize,
    raw: RequestChiefQuestion,
) -> Result<Question, InvalidQuestions> {
    let question = raw.question.trim().to_string();
    if question.is_empty() {
        return Err(InvalidQuestions::Blank(n));
    }
    if raw.options.len() > MOST_OPTIONS {
        return Err(InvalidQuestions::Crowded(n));
    }
    let mut options = Vec::<Offer>::new();
    for offer in raw.options {
        let (title, description) = match offer {
            RequestChiefOption::Title(title) => {
                (title, String::new())
            }
            RequestChiefOption::Described { title, description } => {
                (title, description)
            }
        };
        let (title, description) = (
            title.trim().to_string(),
            description.trim().to_string(),
        );
        if title.is_empty() {
            return Err(InvalidQuestions::Untitled(n));
        }
        if options.iter().any(|o| o.title == title) {
            return Err(InvalidQuestions::Twice {
                question: n,
                title,
            });
        }
        options.push(Offer { title, description });
    }
    // A question with nothing to pick is answered in words.
    let text_input =
        options.is_empty() || raw.text_input.unwrap_or(true);
    Ok(Question {
        question,
        description: raw.description.trim().into(),
        options,
        text_input,
    })
}

/// The questions as words, for a chief who reads them as text. One
/// plain question is its own words.
pub(crate) fn about(put: &Questions) -> String {
    if let [only] = put.questions.as_slice()
        && put.message.is_empty()
        && only.description.is_empty()
        && only.options.is_empty()
    {
        return only.question.clone();
    }
    let mut lines = Vec::new();
    if !put.message.is_empty() {
        lines.push(put.message.clone());
    }
    let numbered = put.questions.len() > 1;
    for (at, q) in put.questions.iter().enumerate() {
        lines.push(match numbered {
            true => format!("{}. {}", at + 1, q.question),
            false => q.question.clone(),
        });
        if !q.description.is_empty() {
            lines.push(format!("   {}", q.description));
        }
        for (at, offer) in q.options.iter().enumerate() {
            let letter = char::from(b'a' + at as u8);
            lines.push(match offer.description.is_empty() {
                true => format!("   {letter}) {}", offer.title),
                false => format!(
                    "   {letter}) {} - {}",
                    offer.title, offer.description
                ),
            });
        }
        if q.text_input && !q.options.is_empty() {
            lines.push("   or in your own words".into());
        }
    }
    lines.join("\n")
}

/// The answer the agent is given for `picks`, one for each question
/// of `put` in order: the chief's reply, each question quoted over
/// its answer.
pub(crate) fn answer(
    put: &Questions,
    picks: &[Pick],
) -> Result<String, Unanswered> {
    if picks.len() != put.questions.len() {
        return Err(Unanswered::Unequal);
    }
    let answers = put
        .questions
        .iter()
        .zip(picks)
        .enumerate()
        .map(|(at, (q, pick))| match pick {
            Pick::Offer { index } => usize::try_from(*index)
                .ok()
                .and_then(|index| q.options.get(index))
                .map(|o| o.title.clone())
                .ok_or(Unanswered::NoOption(at + 1)),
            Pick::Words { .. } if !q.text_input => {
                Err(Unanswered::TakesOption(at + 1))
            }
            Pick::Words { text } if text.trim().is_empty() => {
                Err(Unanswered::NoWords(at + 1))
            }
            Pick::Words { text } => Ok(text.trim().to_string()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let body = put
        .questions
        .iter()
        .zip(&answers)
        .map(|(q, answer)| {
            let words =
                q.question.split_whitespace().collect::<Vec<_>>();
            format!("> {}\n{answer}", words.join(" "))
        })
        .collect::<Vec<_>>();
    Ok(format!("{ANSWERS}\n{}", body.join("\n\n")))
}

/// The chief's own words in a result of `request_chief`. `None` when
/// the chief did not answer.
pub(crate) fn said(result: &str) -> Option<&str> {
    result
        .strip_prefix(ANSWERS)
        .map(str::trim)
        .filter(|words| !words.is_empty())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn grill() -> String {
        json!({
            "message": "Before I plan the shelf.",
            "questions": [
                {
                    "question": " Paper or ebook? ",
                    "description": "Shipping depends on it.",
                    "options": [
                        {"title": "Paper", "description": "Three days"},
                        "Ebook"
                    ]
                },
                {
                    "question": "Hardback?",
                    "options": ["Yes", "No"],
                    "text_input": false
                },
                {"question": "Budget?", "text_input": false}
            ]
        })
        .to_string()
    }

    fn words(text: &str) -> Pick {
        Pick::Words { text: text.into() }
    }

    #[test]
    fn every_question_is_answered_as_it_allows() {
        let put = read(&grill()).unwrap();
        let answered = answer(
            &put,
            &[
                words("both"),
                Pick::Offer { index: 1 },
                words("under 30"),
            ],
        )
        .unwrap();
        assert_eq!(
            answered,
            "The chief answers:\n> Paper or ebook?\nboth\n\n> \
             Hardback?\nNo\n\n> Budget?\nunder 30"
        );
        assert_eq!(
            said(&answered),
            Some(
                "> Paper or ebook?\nboth\n\n> Hardback?\nNo\n\n> \
                 Budget?\nunder 30"
            )
        );
        assert_eq!(said("The chief chose not to answer."), None);
        let wrong = |picks: &[Pick]| {
            answer(&put, picks).unwrap_err().to_string()
        };
        let first = Pick::Offer { index: 0 };
        assert!(
            wrong(std::slice::from_ref(&first))
                .contains("every question")
        );
        let no_words = [first.clone(), words("maybe"), words("x")];
        assert!(wrong(&no_words).contains("takes an option"));
        let blank = [first.clone(), first.clone(), words(" ")];
        assert!(wrong(&blank).contains("needs words"));
        let missing = [Pick::Offer { index: 2 }, first, words("x")];
        assert!(wrong(&missing).contains("no such option"));
    }
}
