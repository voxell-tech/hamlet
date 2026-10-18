//! One skill of the library, as a card.

use bevy_fynix::tokens::Tone;
use hamlet_core::core::Skills;
use hamlet_sdk::AnyView;
use hamlet_sdk::cards::card;
use hamlet_sdk::confirm::Confirm;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::looks::bad_action;

#[derive(Clone, PartialEq)]
pub(crate) struct SkillRow {
    pub name: String,
    pub description: String,
}

pub(crate) fn rows(skills: &Skills) -> Vec<SkillRow> {
    skills
        .0
        .iter()
        .map(|s| SkillRow {
            name: s.name.clone(),
            description: s.description.clone(),
        })
        .collect()
}

/// The skill's card, with the press that takes it out beside its
/// name.
pub(crate) fn skill(row: &SkillRow) -> AnyView {
    let gone = row.name.clone();
    let out = bad_action(Glyph::Delete, "TAKE OUT", move |world| {
        world.trigger(Confirm {
            question: format!(
                "TAKE {} OUT OF THE LIBRARY?",
                gone.to_uppercase()
            ),
            outcome: format!(
                "{gone} is deleted from the library for every \
                 hamlet. This cannot be undone."
            ),
            verb: "TAKE OUT".into(),
            command: format!("/unshelve {gone}"),
        })
    });
    card(
        row.name.clone(),
        row.description.clone(),
        Tone::Body,
        Some(out),
    )
}
