//! A motion a robot plays once, whatever its work.

use bevy::prelude::*;

use super::Robot;
use crate::Clock;
use crate::keeper::{Act, Keeper};

/// A motion a robot plays once, on its way out of the world or back
/// into it or at a chief's hand. Its work does not move it
/// meanwhile.
#[derive(Component, Clone, Copy, Debug)]
pub struct Passage {
    act: Act,
    /// Clock time it began. None until it has.
    since: Option<f32>,
    /// True when it lasts until it is taken off.
    held: bool,
}

impl Passage {
    pub fn new(act: Act) -> Self {
        Self {
            act,
            since: None,
            held: false,
        }
    }

    /// A motion kept up until the passage is taken off the robot.
    pub fn hold(act: Act) -> Self {
        Self {
            held: true,
            ..Self::new(act)
        }
    }

    pub fn act(&self) -> Act {
        self.act
    }

    /// True when the robot is gone when it has played.
    pub(in crate::generator) fn leaves(&self) -> bool {
        self.act.vanishes()
    }
}

/// Plays each robot's passage, and takes away a robot whose passage
/// was its way out.
pub(in crate::generator) fn pass(
    mut commands: Commands,
    clock: Res<Clock>,
    mut robots: Query<(
        Entity,
        &mut Keeper,
        &mut Passage,
        Option<&mut Robot>,
    )>,
) {
    let now = clock.t;
    for (entity, mut keeper, mut passage, robot) in &mut robots {
        let since = match passage.since {
            Some(since) => since,
            None => {
                keeper.play(passage.act, now);
                passage.since = Some(now);
                now
            }
        };
        // A still shows the moment after.
        let over = clock.frozen
            || passage.act.length().is_none_or(|l| now - since >= l);
        if !over || (passage.held && !clock.frozen) {
            continue;
        }
        if passage.act.vanishes() {
            commands.entity(entity).try_despawn();
        } else {
            commands.entity(entity).try_remove::<Passage>();
            // Its chore went on without it meanwhile. A way it was
            // still on is taken up again at the speed it had.
            if let Some(mut robot) = robot {
                let under_way = robot
                    .job
                    .as_ref()
                    .filter(|job| now < job.arrive())
                    .map(|job| job.leg.speed);
                let haste = robot.haste.or(under_way);
                robot.put_down(haste);
            }
        }
    }
}
