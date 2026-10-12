//! The one NOTES.md of two merged agents: rewritten by a model, or
//! the two joined end to end.

use serde_json::{Value, json};

use crate::hamlet::{Census, Hamlet};

impl Hamlet {
    pub(super) fn notes_of(
        &self,
        census: &Census,
        name: &str,
    ) -> String {
        std::fs::read_to_string(
            self.house_in(census, name).join("workspace/NOTES.md"),
        )
        .unwrap_or_default()
    }

    /// The model request that has `keep` write the one NOTES.md of
    /// the merged agent. `None` when either has no notes yet, so
    /// joining them loses nothing and costs nothing.
    pub fn merge_request(
        &self,
        keep: &str,
        absorb: &str,
        purpose: &str,
    ) -> Option<Vec<Value>> {
        let census = self.census();
        let find = |name: &str| {
            census.residents.iter().find(|r| r.name == name)
        };
        let (kept, gone) = (find(keep)?, find(absorb)?);
        let (mine, theirs) = (
            self.notes_of(&census, keep),
            self.notes_of(&census, absorb),
        );
        if substance(&mine).is_empty()
            || substance(&theirs).is_empty()
        {
            return None;
        }
        let limit = self.limits().ok()?.leaving.merged_notes;
        let system = format!(
            "You are `{keep}`, an agent in a hamlet of agents. The \
             mayor is merging the agent `{absorb}` into you: from \
             now on you do the work of both, under your own name. \
             Your purpose was: {} Its purpose was: {} Your purpose \
             becomes: {}\nYou are writing your new NOTES.md, the \
             memory that is shown to you on every turn.",
            kept.purpose,
            gone.purpose,
            purpose.trim()
        );
        let user = format!(
            "Your NOTES.md now:\n<notes \
             of=\"{keep}\">\n{mine}\n</notes>\n\nThe NOTES.md of \
             `{absorb}`:\n<notes \
             of=\"{absorb}\">\n{theirs}\n</notes>\n\nWrite the one \
             NOTES.md you will keep.\n- Keep every fact, preference \
             and lesson from both. Drop nothing because it looks \
             minor.\n- Where both say the same thing, say it \
             once.\n- Where they disagree, keep both and say which \
             of you held which.\n- Speak as yourself: things \
             `{absorb}` wrote as \"I\" are now things you know or \
             do.\n- Group it by subject, not by who wrote it.\n- At \
             most {limit} characters.\n- Start with the line `# \
             Notes`. Reply with the file's text only: no code \
             fence, no comment before or after."
        );
        Some(vec![
            json!({"role": "system", "content": system}),
            json!({"role": "user", "content": user}),
        ])
    }
}

/// The text of notes besides the heading every NOTES.md starts
/// with.
pub(super) fn substance(notes: &str) -> String {
    notes
        .lines()
        .filter(|l| l.trim() != "# Notes")
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Two notes end to end, the absorbed agent's under its own
/// heading.
pub(super) fn join(mine: &str, gone: &str, theirs: &str) -> String {
    let theirs = substance(theirs);
    if theirs.is_empty() {
        return mine.trim_end().to_string();
    }
    format!("{}\n\n## From {gone}\n{theirs}", mine.trim_end())
}

/// The notes in a model's reply, if they can be used as a NOTES.md.
pub(super) fn accept(reply: &str, limit: usize) -> Option<String> {
    let mut text = reply.trim();
    // Reasoning some models leave in their reply.
    if let Some((_, after)) = text.rsplit_once("</think>") {
        text = after.trim();
    }
    if let Some(inner) = text.strip_prefix("```") {
        text = inner
            .split_once('\n')
            .map_or("", |(_, body)| body)
            .trim_end()
            .trim_end_matches("```")
            .trim();
    }
    if let Some(at) = text.find("# Notes") {
        text = &text[at..];
    }
    let fits =
        !substance(text).is_empty() && text.chars().count() <= limit;
    fits.then(|| text.to_string())
}
