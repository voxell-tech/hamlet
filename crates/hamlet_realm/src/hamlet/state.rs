//! Each agent's state and work, shown on its robot.

use std::collections::HashSet;

use bevy::prelude::*;
use hamlet_client::realm::{
    Activity, Busy, Doing, Faulted, Left, Paper, Voted,
};
use hamlet_client::waits::{Needs, RaisedBy, Waiting};
use hamlet_protocol::client as protocol;

use super::Housed;
use crate::Clock;
use crate::generator::society::Society;
use crate::generator::{Objecting, Voting};
use crate::keeper::KeeperState;
use crate::keeper::work::{self, Task, Work};

fn work(activity: &Activity) -> Work {
    let paper = |p: Paper| match p {
        Paper::File => work::Paper::File,
        Paper::Notes => work::Paper::Notes,
        Paper::Skill => work::Paper::Skill,
    };
    Work {
        task: match activity.doing {
            Doing::Unknown => Task::Unknown,
            Doing::Think => Task::Think,
            Doing::Read(p) => Task::Read(paper(p)),
            Doing::Write(p) => Task::Write(paper(p)),
            Doing::Run => Task::Run,
            Doing::Fetch => Task::Fetch,
            Doing::CustomTool => Task::CustomTool,
            Doing::Await => Task::Await,
        },
        step: u32::try_from(activity.step).unwrap_or(u32::MAX),
        failures: u32::try_from(activity.failures)
            .unwrap_or(u32::MAX),
    }
}

/// The caster of a ballot is seen casting it.
pub(super) fn vote(
    voted: On<Voted>,
    agents: Query<&Housed>,
    clock: Res<Clock>,
    mut voting: ResMut<Voting>,
) {
    if let Ok(agent) = agents.get(voted.agent) {
        voting.0.insert(agent.0, clock.t);
    }
}

/// Shows what each agent is doing, and who is arguing with the
/// chief.
pub(super) fn show(
    mut society: ResMut<Society>,
    mut objecting: ResMut<Objecting>,
    agents: Query<
        (&Housed, Has<Busy>, Has<Faulted>, Has<Needs>),
        Without<Left>,
    >,
    working: Query<
        (&Housed, &Activity),
        Or<(Changed<Activity>, Added<Housed>)>,
    >,
    waiting: Query<(&Waiting, &RaisedBy)>,
    housed: Query<&Housed>,
) {
    for (agent, busy, faulted, needs) in &agents {
        // Waiting for the chief shows over whatever else an agent
        // does.
        let state = if needs {
            KeeperState::Waiting
        } else if faulted {
            KeeperState::Fault
        } else if busy {
            KeeperState::Working
        } else {
            KeeperState::Rest
        };
        if society.agent(agent.0).is_some_and(|a| a.state != state) {
            society.set_state(agent.0, state);
        }
    }
    for (agent, activity) in &working {
        let work = work(activity);
        if society.agent(agent.0).is_some_and(|a| a.work != work) {
            society.set_work(agent.0, work);
        }
    }
    let arguing = waiting
        .iter()
        .filter(|(waiting, _)| {
            matches!(
                waiting.detail,
                protocol::WaitDetail::Objection {}
            )
        })
        .filter_map(|(_, by)| housed.get(by.0).ok())
        .map(|agent| agent.0)
        .collect::<HashSet<_>>();
    if objecting.0 != arguing {
        objecting.0 = arguing;
    }
}
