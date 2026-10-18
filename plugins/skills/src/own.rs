//! The skills the selected agent keeps in its own house.

use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_sdk::AnyView;
use hamlet_sdk::cards::item;
use hamlet_sdk::convo::AgentSelected;
use hamlet_sdk::looks::{bare, note};
use hamlet_sdk::papers::Papers;

/// The skills the selected agent wrote, or a word that none is
/// selected.
pub(crate) fn own() -> AnyView {
    keyed(resource::<AgentSelected, _>(|s| s.0), |selected| {
        if !*selected {
            return note("Select an agent to see its own skills.")
                .boxed();
        }
        keyed(resource::<Papers, _>(|p| p.skills.clone()), |skills| {
            if skills.is_empty() {
                return note("None of its own yet.").boxed();
            }
            let rows = skills
                .iter()
                .map(|(name, does)| {
                    item(name.clone(), does.clone(), Tone::Body)
                })
                .collect::<Vec<_>>();
            column(rows).gap(8.0).boxed()
        })
        .within(bare())
        .boxed()
    })
    .within(bare())
    .boxed()
}
