//! The petitions of plugins in the chief's queue, and the chief's
//! answers back to the SDK.

use bevy::ecs::lifecycle::Add;
use bevy::prelude::*;
use hamlet_core::oversight::{self, Settled};
use hamlet_rules::chief::rulings::Choice;
use hamlet_sdk::capability::{Answer, Answered, Petition};

/// Puts each [`Petition`] in the queue and passes the chief's answer
/// back as [`Answered`].
pub fn plugin(app: &mut App) {
    app.add_observer(raise).add_observer(settled);
}

fn raise(
    raised: On<Add, Petition>,
    petitions: Query<&Petition>,
    mut commands: Commands,
) {
    let entity = raised.entity;
    let Ok(petition) = petitions.get(entity) else {
        return;
    };
    let by = petition.by.clone();
    let text = petition.text.clone();
    let answers =
        petition.answers.iter().map(|a| choice(*a)).collect();
    commands.queue(move |world: &mut World| {
        oversight::petition(world, entity, &by, text, answers)
    });
}

fn settled(
    settled: On<Settled>,
    petitions: Query<(), With<Petition>>,
    mut commands: Commands,
) {
    if petitions.contains(settled.entity) {
        commands.trigger(Answered {
            petition: settled.entity,
            answer: answer(settled.choice),
        });
    }
}

fn choice(answer: Answer) -> Choice {
    match answer {
        Answer::Allow => Choice::Allow,
        Answer::AskNextTime => Choice::AskNextTime,
        Answer::Deny => Choice::Deny,
        Answer::Dismiss => Choice::Dismiss,
        Answer::KeepData => Choice::KeepData,
        Answer::EraseData => Choice::EraseData,
    }
}

/// The chief's `choice` as an [`Answer`]. A choice no petition of a
/// plugin offers is a dismissal.
fn answer(choice: Choice) -> Answer {
    match choice {
        Choice::Allow => Answer::Allow,
        Choice::AskNextTime => Answer::AskNextTime,
        Choice::Deny => Answer::Deny,
        Choice::KeepData => Answer::KeepData,
        Choice::EraseData => Answer::EraseData,
        _ => Answer::Dismiss,
    }
}
