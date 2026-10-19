//! The cues of what is not seen happening: the hand, the turn of the
//! day and the chief's answers.

use bevy::prelude::*;
use hamlet_client::messages::{Answer, Veto};
use hamlet_client::waits::Choice;

use super::{Cue, Sound};
use crate::DayPart;
use crate::play::Played;

/// A sound for each thing the chief does to the world by hand,
/// where it was done.
pub(super) fn hear_hand(
    mut played: MessageReader<Played>,
    mut cues: MessageWriter<Cue>,
    mut turn: Local<u32>,
) {
    for played in played.read() {
        *turn = turn.wrapping_add(1);
        cues.write(Cue {
            sound: Sound::played(played.what),
            at: played.at,
            turn: *turn,
        });
    }
}

/// A few small bars as the day breaks and as night falls.
pub(super) fn turn_of_day(
    part: Option<Res<State<DayPart>>>,
    mut was: Local<Option<DayPart>>,
    mut cues: MessageWriter<Cue>,
) {
    let Some(now) = part.map(|part| *part.get()) else {
        return;
    };
    let sound = match (was.replace(now), now) {
        (Some(DayPart::Night), DayPart::Dawn | DayPart::Day) => {
            Sound::Dawn
        }
        (Some(DayPart::Day), DayPart::Dusk | DayPart::Night) => {
            Sound::Dusk
        }
        _ => return,
    };
    cues.write(Cue {
        sound,
        at: Vec3::ZERO,
        turn: 0,
    });
}

/// The chief's answer to something that waited: yes or no. A reply
/// in words and a dismissal keep the sound of their button.
pub(super) fn hear_answer(
    answer: On<Answer>,
    mut cues: MessageWriter<Cue>,
) {
    let sound = match answer.choice {
        Choice::Allow | Choice::Always | Choice::Go => Sound::Allowed,
        Choice::Deny | Choice::Withdraw => Sound::Denied,
        Choice::Reply | Choice::Dismiss => return,
    };
    cues.write(Cue {
        sound,
        at: Vec3::ZERO,
        turn: 0,
    });
}

/// The chief stopped an agent, or everything.
pub(super) fn hear_veto(_: On<Veto>, mut cues: MessageWriter<Cue>) {
    cues.write(Cue {
        sound: Sound::Stopped,
        at: Vec3::ZERO,
        turn: 0,
    });
}
