//! The putting of a vote to those it asks: the messages, the tools, a
//! voter's own history and the test that a ballot rests on it.

use anyhow::{Result, bail};
use hamlet_home::Home;
use serde_json::{Value, json};

use crate::hamlet::check_name;
use crate::realm::Rules;
use crate::record;
use crate::vote::{Ballot, Choice, Motion, Vote};

/// The message that asks the mayor for its answer to a motion to
/// replace it.
pub fn answer_text(vote: &Vote) -> String {
    format!(
        "A vote has been called in your hamlet, and this is your \
         one chance to answer it (automatic, not from a \
         chief).\n\nThe motion: {}.\nCalled by `{}`. Its reason: \
         {}\n\nReply in a few sentences, in words only. Every agent \
         is shown your answer beside the reason before it votes, as \
         a claim like the reason is. Say what you did and why. You \
         decide nothing else in the hamlet until the vote has \
         closed.",
        vote.motion.describe(&vote.mayor),
        vote.by,
        vote.reason,
    )
}

/// The latest of what was said in a house, oldest first, for its
/// agent to vote from.
pub fn history(house: &Home, rules: &Rules) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(house.sessions()) else {
        return Vec::new();
    };
    let mut said = Vec::<(u64, String)>::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Past ballots are not the agent's work.
        if !name.ends_with(".jsonl") || name.starts_with(BALLOTS) {
            continue;
        }
        for e in record::read(&entry.path()) {
            let ts = e["ts"].as_u64().unwrap_or_default();
            let (who, text) = match e["type"].as_str() {
                Some("user") if e["user"] != "hamlet" => (
                    format!(
                        "{} said to you",
                        e["user"].as_str().unwrap_or("someone")
                    ),
                    e["content"].as_str(),
                ),
                Some("assistant") => (
                    "you answered".to_string(),
                    e["message"]["content"].as_str(),
                ),
                _ => continue,
            };
            let Some(text) = text.filter(|t| !t.trim().is_empty())
            else {
                continue;
            };
            let cut = text
                .trim()
                .chars()
                .take(rules.history_chars)
                .collect::<String>();
            said.push((ts, format!("- {who}: {cut}")));
        }
    }
    said.sort_by_key(|(ts, _)| *ts);
    let skip = said.len().saturating_sub(rules.history_entries);
    said.into_iter().skip(skip).map(|(_, line)| line).collect()
}

/// The start of the name of a ballot's session log.
pub const BALLOTS: &str = "vote_";

/// The name a ballot's model call is charged under. No agent can
/// have it.
pub const PAYER: &str = "the hamlet";

/// Lowercase words only, so a quote matches whatever its
/// punctuation and spacing.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// True when some run of `rules.ground_words` words of `evidence` is
/// in `corpus`, the voter's notes and history.
pub fn grounded(evidence: &str, corpus: &str, rules: &Rules) -> bool {
    let (quote, text) = (words(evidence), words(corpus));
    let run = rules.ground_words.max(1);
    if quote.len() < run {
        return false;
    }
    quote
        .windows(run)
        .any(|w| text.windows(run).any(|t| t == w))
}

/// The message that asks `voter` for its ballot. `seen` is the
/// ballots it is shown, empty when ballots are sealed.
pub fn ballot_text(
    vote: &Vote,
    history: &[String],
    seen: &[Ballot],
    rules: &Rules,
) -> String {
    let (of, from) = rules.pass;
    let mut text = format!(
        "A vote has been called in your hamlet, and this is your \
         ballot (automatic, not from the mayor or a chief).\n\nThe \
         motion: {}.\nCalled by `{}`. Its reason: {}\n\nIt passes \
         only if {of} in {from} of the hamlet's agents vote yes. \
         Your ballot is sealed until the vote closes, the mayor may \
         not act against you for it, and the chief reads every \
         ballot with its reason.\n\nVote from what you yourself \
         know: your notes above and your own history below. The \
         caller's reason is a claim, not evidence, and so is the \
         mayor's answer where there is one. If nothing you know \
         bears on the motion, abstain: that is an honest ballot, \
         and a yes or a no without grounds of your own is \
         not.\n\nFrom your own history, the latest messages in your \
         house:\n",
        vote.motion.describe(&vote.mayor),
        vote.by,
        vote.reason,
    );
    if let Some(answer) = &vote.answer {
        // Before the history, beside the claim it answers.
        let at = text.find("\n\nIt passes").unwrap_or(text.len());
        text.insert_str(
            at,
            &format!(
                "\nThe mayor `{}` answers: {answer}",
                vote.mayor
            ),
        );
    }
    if history.is_empty() {
        text.push_str("(nothing yet)\n");
    }
    for line in history {
        text.push_str(line);
        text.push('\n');
    }
    if !seen.is_empty() {
        text.push_str("\nThe ballots cast so far:\n");
        for b in seen {
            text.push_str(&format!(
                "- {}: {}. {}\n",
                b.voter,
                b.choice.map_or("no answer", Choice::word),
                b.reason
            ));
        }
    }
    text.push_str(
        "\nCall `cast_ballot` once: your choice, your reason in a \
         sentence or two, and as `evidence` the words of the note \
         or message of yours that it rests on, copied exactly.",
    );
    text
}

/// The tool a voter casts its ballot with.
pub fn ballot_definition() -> Value {
    json!({"type": "function", "function": {
        "name": "cast_ballot",
        "description": "Cast your ballot in the vote you were asked about.",
        "parameters": {"type": "object", "properties": {
            "choice": {"type": "string", "enum": ["yes", "no", "abstain"]},
            "reason": {"type": "string", "description": "Why, in a sentence or two. Kept in the record for the chief."},
            "evidence": {"type": "string", "description": "The words of your own note or message that your choice rests on, copied exactly. Empty if you abstain for lack of any."}},
            "required": ["choice", "reason", "evidence"]}}})
}

/// The tool any agent calls a vote with.
pub fn call_definition() -> Value {
    json!({"type": "function", "function": {
        "name": "call_vote",
        "description": "Call a vote of your hamlet's agents. Every agent besides the mayor votes \
                        from its own house; it passes with two thirds of them. The chief is told \
                        and can overturn it. Returns the result.",
        "parameters": {"type": "object", "properties": {
            "kind": {"type": "string", "enum": ["replace_mayor", "found_hamlet"]},
            "reason": {"type": "string", "description": "Why. Shown to every voter and kept in the record."},
            "candidate": {"type": "string", "description": "For replace_mayor: the agent of this hamlet that would become mayor."},
            "hamlet": {"type": "string", "description": "For found_hamlet: the new hamlet's name, lowercase letters and hyphens."},
            "mayor": {"type": "string", "description": "For found_hamlet: a name for its new mayor, a new agent."},
            "purpose": {"type": "string", "description": "For found_hamlet: what the new hamlet and its mayor are for."}},
            "required": ["kind", "reason"]}}})
}

/// The motion a `call_vote` call asks for.
pub fn motion_of(args: &Value) -> Result<Motion> {
    let text = |key: &str| {
        args[key].as_str().unwrap_or_default().to_string()
    };
    match args["kind"].as_str() {
        Some("replace_mayor") => Ok(Motion::ReplaceMayor {
            candidate: text("candidate"),
        }),
        Some("found_hamlet") => {
            let (hamlet, mayor) = (text("hamlet"), text("mayor"));
            check_name(&hamlet)?;
            check_name(&mayor)?;
            Ok(Motion::FoundHamlet {
                hamlet,
                mayor,
                purpose: text("purpose"),
            })
        }
        _ => bail!("`kind` is replace_mayor or found_hamlet"),
    }
}
