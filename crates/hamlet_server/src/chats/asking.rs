//! The questions an agent puts to the chief with `ask_chief`: read
//! from the call and checked, set out as words, and answered.

use hamlet_protocol::v0::{Offer, Pick, Question, Questions};
use serde_json::{Value, json};

/// The most questions one call may ask.
pub(super) const MOST_QUESTIONS: usize = 10;

/// The most options one question may offer.
pub(super) const MOST_OPTIONS: usize = 6;

/// The head of every answer the chief gives to `ask_chief`.
pub(super) const ANSWERS: &str = "The chief answers:";

/// The words at `key` of `value`, trimmed. Empty when there are none.
fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().trim().to_string()
}

/// The questions in the arguments of an `ask_chief` call. `Err` says
/// what is wrong with them, for the agent to put right.
pub(super) fn read(args: &str) -> Result<Questions, String> {
    let args = serde_json::from_str::<Value>(args)
        .map_err(|_| "the arguments are not JSON".to_string())?;
    let asked = match &args["questions"] {
        Value::Array(asked) => asked.clone(),
        // A lone `question` is a call of one plain question.
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

/// The questions as words, for a chief who reads them as text. One
/// plain question is its own words.
pub(super) fn about(asked: &Questions) -> String {
    if let [only] = asked.questions.as_slice()
        && asked.message.is_empty()
        && only.description.is_empty()
        && only.options.is_empty()
    {
        return only.question.clone();
    }
    let mut lines = Vec::new();
    if !asked.message.is_empty() {
        lines.push(asked.message.clone());
    }
    let numbered = asked.questions.len() > 1;
    for (at, q) in asked.questions.iter().enumerate() {
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
/// of `asked` in order: the chief's reply, each question quoted over
/// its answer. `Err` says why they do not answer them.
pub(super) fn answer(
    asked: &Questions,
    picks: &[Pick],
) -> Result<String, String> {
    if picks.len() != asked.questions.len() {
        return Err("every question needs an answer".into());
    }
    let answers = asked
        .questions
        .iter()
        .zip(picks)
        .enumerate()
        .map(|(at, (q, pick))| match pick {
            Pick::Offer { index } => usize::try_from(*index)
                .ok()
                .and_then(|index| q.options.get(index))
                .map(|o| o.title.clone())
                .ok_or_else(|| {
                    format!("question {} has no such option", at + 1)
                }),
            Pick::Words { .. } if !q.text_input => {
                Err(format!("question {} takes an option", at + 1))
            }
            Pick::Words { text } if text.trim().is_empty() => {
                Err(format!("question {} needs words", at + 1))
            }
            Pick::Words { text } => Ok(text.trim().to_string()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let body = asked
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

/// The chief's own words in a result of `ask_chief`. `None` when the
/// chief did not answer.
pub(super) fn said(result: &str) -> Option<&str> {
    result
        .strip_prefix(ANSWERS)
        .map(str::trim)
        .filter(|words| !words.is_empty())
}

#[cfg(test)]
mod tests {
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
    fn questions_are_read_with_their_options_and_words() {
        let asked = read(&grill()).unwrap();
        assert_eq!(asked.message, "Before I plan the shelf.");
        let [paper, hardback, budget] = asked.questions.as_slice()
        else {
            panic!("three questions");
        };
        assert_eq!(paper.question, "Paper or ebook?");
        assert_eq!(paper.options[0].description, "Three days");
        assert_eq!(paper.options[1].title, "Ebook");
        assert!(paper.text_input);
        assert!(!hardback.text_input);
        // Nothing to pick, so it is answered in words all the same.
        assert!(budget.text_input);
        assert_eq!(
            about(&asked),
            "Before I plan the shelf.\n1. Paper or ebook?\n   \
             Shipping depends on it.\n   a) Paper - Three days\n   \
             b) Ebook\n   or in your own words\n2. Hardback?\n   a) \
             Yes\n   b) No\n3. Budget?"
        );
    }

    #[test]
    fn what_is_wrong_with_a_call_is_named() {
        let fails =
            |args: Value| read(&args.to_string()).unwrap_err();
        assert!(fails(json!({})).contains("`questions`"));
        assert!(
            fails(json!({"questions": []})).contains("`questions`")
        );
        let many = vec![json!({"question": "?"}); MOST_QUESTIONS + 1];
        assert!(
            fails(json!({"questions": many})).contains("at most")
        );
        assert!(
            fails(json!({"questions": [{"question": "a"}, {"question": " "}]}))
                .contains("question 2")
        );
        let twice = json!({"questions": [{"question": "a", "options": ["x", " x"]}]});
        assert!(fails(twice).contains("`x` twice"));
        let untitled = json!({"questions": [{"question": "a", "options": [{"description": "d"}]}]});
        assert!(fails(untitled).contains("no `title`"));
        let options = (0..=MOST_OPTIONS)
            .map(|at| format!("o{at}"))
            .collect::<Vec<_>>();
        let crowded = json!({"questions": [{"question": "a", "options": options}]});
        assert!(fails(crowded).contains("more than"));
    }

    #[test]
    fn every_question_is_answered_as_it_allows() {
        let asked = read(&grill()).unwrap();
        let answered = answer(
            &asked,
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
        let wrong =
            |picks: &[Pick]| answer(&asked, picks).unwrap_err();
        let first = Pick::Offer { index: 0 };
        assert!(wrong(&[first.clone()]).contains("every question"));
        let no_words = [first.clone(), words("maybe"), words("x")];
        assert!(wrong(&no_words).contains("takes an option"));
        let blank = [first.clone(), first.clone(), words(" ")];
        assert!(wrong(&blank).contains("needs words"));
        let missing = [Pick::Offer { index: 2 }, first, words("x")];
        assert!(wrong(&missing).contains("no such option"));
    }
}
