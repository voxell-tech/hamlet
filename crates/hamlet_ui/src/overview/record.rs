//! A hamlet's record: its budget, its votes and who came and went.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::views::{FrameProps, column};
use bevy_fynix::{ViewExt, derived, keyed};
use hamlet_core::agents::Settlement;
use hamlet_core::core::Settings;
use hamlet_rules::realm::Realm;
use hamlet_rules::vote::{self, Standing};

use super::{heading, tell};
use crate::Ui;
use crate::icons::Glyph;
use crate::looks::{bad_action, bare, note};
use crate::me::Me;
use crate::roll::hamlets_moved;

/// The spending, the votes and the comings and goings of a hamlet.
#[derive(Clone, PartialEq, Default)]
struct Annals {
    budget: String,
    /// Each vote by its number, and that a chief can still overturn
    /// it.
    votes: Vec<(u64, String, bool)>,
    record: String,
}

fn annals(world: &World, hamlet: Entity) -> Annals {
    let Some(settlement) = world.get::<Settlement>(hamlet) else {
        return Annals::default();
    };
    let place = settlement.record();
    let model = world
        .get_resource::<Settings>()
        .map(|settings| settings.config.model.clone())
        .unwrap_or_default();
    let rules = Realm::open(place.home())
        .map(|realm| realm.rules)
        .unwrap_or_default();
    let votes = vote::votes(&place)
        .iter()
        .rev()
        .map(|vote| {
            let open = matches!(
                vote.standing,
                Standing::Open | Standing::Passed
            );
            (vote.id, vote.report(&rules), open)
        })
        .collect();
    let user = world
        .get_resource::<Me>()
        .map(|me| me.user.clone())
        .unwrap_or_default();
    let record = match place.decree(&user, "/record") {
        Some(Ok(decree)) => decree.lines.join("\n"),
        Some(Err(e)) => format!("{e:#}"),
        None => String::new(),
    };
    Annals {
        budget: place.report(&model).lines(),
        votes,
        record,
    }
}

/// A hamlet's record: its budget, its votes, each to be overturned
/// while it stands, and who came and went.
pub(super) fn record_of(hamlet: Entity) -> Ui {
    let told = derived(move |world: &World| annals(world, hamlet))
        .when(hamlets_moved());
    let words = |text: &str| note(text.to_string()).boxed();
    keyed(told, move |annals| {
        let mut parts = vec![
            heading(Glyph::Meter, "THE BUDGET".to_string()),
            words(&annals.budget),
            heading(
                Glyph::Ballot,
                format!("VOTES ({})", annals.votes.len()),
            ),
        ];
        if annals.votes.is_empty() {
            parts.push(words("No votes yet."));
        }
        for (id, report, open) in &annals.votes {
            let id = *id;
            let mut vote = vec![words(report.trim_end())];
            if *open {
                vote.push(bad_action(
                    Glyph::Restore,
                    "OVERTURN",
                    move |world| {
                        tell(world, hamlet, format!("/overturn {id}"))
                    },
                ));
            }
            parts.push(
                column(vote)
                    .gap(4.0)
                    .align(AlignItems::FlexStart)
                    .boxed(),
            );
        }
        parts.push(heading(
            Glyph::History,
            "WHO CAME AND WENT".to_string(),
        ));
        parts.push(words(&annals.record));
        column(parts).gap(8.0).padding(UiRect::top(px(4.0))).boxed()
    })
    .within(bare())
    .boxed()
}
