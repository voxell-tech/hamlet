//! The commands that wait for an answer.

use bevy::prelude::*;
use hamlet_agents_api::tools::plugged::Answer;

/// Who ran a command.
#[derive(Clone, PartialEq, Debug)]
pub enum Caller {
    Plugin {
        entity: Entity,
        /// The plugin's own number for the call.
        id: u64,
    },
    /// The chief, by the command of this name.
    Chief(String),
    /// An agent, calling a tool. The answer goes to the sender kept
    /// in [`Calls`] under the call's number.
    Agent,
}

/// A command run by one plugin or the chief that its target has not
/// answered.
#[derive(Clone, PartialEq, Debug)]
pub struct Call {
    /// The host's number for it, which the target answers with.
    pub id: u64,
    pub caller: Caller,
    pub target: Entity,
    pub target_id: String,
    /// The plugins it has crossed, this hop included.
    pub depth: usize,
    /// The frame it is given up at.
    pub due: u64,
}

/// Every command waiting for an answer, and the frame count the
/// waits are measured by.
#[derive(Resource, Default, Debug)]
pub struct Calls {
    next: u64,
    pub(super) frame: u64,
    pub(super) open: Vec<Call>,
    /// Where the answer to each call of an agent goes.
    agents: Vec<(u64, Answer)>,
}

impl Call {
    /// The plugin's own number for the call, if `plugin` ran it.
    fn numbered_by(&self, plugin: Entity) -> Option<u64> {
        match self.caller {
            Caller::Plugin { entity, id } if entity == plugin => {
                Some(id)
            }
            _ => None,
        }
    }
}

impl Calls {
    /// Whether any call waits.
    pub fn any(calls: Res<Calls>) -> bool {
        !calls.open.is_empty()
    }

    /// The calls `caller` has waiting.
    pub fn in_flight(&self, caller: Entity) -> usize {
        self.open
            .iter()
            .filter(|call| call.numbered_by(caller).is_some())
            .count()
    }

    /// Whether `caller` has a call waiting that it numbered `id`.
    pub fn waiting(&self, caller: Entity, id: u64) -> bool {
        self.open
            .iter()
            .any(|call| call.numbered_by(caller) == Some(id))
    }

    /// The depth of a call `caller` runs now: one more than the
    /// deepest call it is answering.
    pub fn depth_of(&self, caller: Entity) -> usize {
        1 + self
            .open
            .iter()
            .filter(|call| call.target == caller)
            .map(|call| call.depth)
            .max()
            .unwrap_or(0)
    }

    /// Notes a call, to be given up `wait` frames from now, and
    /// returns its number.
    pub fn open(
        &mut self,
        caller: (Entity, u64),
        target: (Entity, &str),
        depth: usize,
        wait: u64,
    ) -> u64 {
        let caller = Caller::Plugin {
            entity: caller.0,
            id: caller.1,
        };
        self.push(caller, target, depth, wait)
    }

    /// Notes a command the chief ran, to be given up `wait` frames
    /// from now, and returns its number.
    pub fn open_for_chief(
        &mut self,
        command: &str,
        target: (Entity, &str),
        wait: u64,
    ) -> u64 {
        let caller = Caller::Chief(command.into());
        self.push(caller, target, 1, wait)
    }

    /// The calls of agents that `target` has waiting.
    pub fn agents_waiting(&self, target: Entity) -> usize {
        self.open
            .iter()
            .filter(|call| {
                call.target == target && call.caller == Caller::Agent
            })
            .count()
    }

    /// Notes a call of an agent, to be given up `wait` frames from
    /// now, and returns its number. `answer` is told how it ends.
    pub fn open_for_agent(
        &mut self,
        target: (Entity, &str),
        wait: u64,
        answer: Answer,
    ) -> u64 {
        let id = self.push(Caller::Agent, target, 1, wait);
        self.agents.push((id, answer));
        id
    }

    /// Tells the agent whose call is numbered `id` how it ended.
    pub(super) fn tell_agent(
        &mut self,
        id: u64,
        outcome: Result<String, String>,
    ) {
        if let Some(at) =
            self.agents.iter().position(|(held, _)| *held == id)
        {
            self.agents.remove(at).1.send(outcome);
        }
    }

    fn push(
        &mut self,
        caller: Caller,
        target: (Entity, &str),
        depth: usize,
        wait: u64,
    ) -> u64 {
        self.next += 1;
        self.open.push(Call {
            id: self.next,
            caller,
            target: target.0,
            target_id: target.1.into(),
            depth,
            due: self.frame + wait,
        });
        self.next
    }

    /// Whether no call waits.
    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// Takes the call numbered `id` that `target` is to answer.
    pub fn answer(
        &mut self,
        id: u64,
        target: Entity,
    ) -> Option<Call> {
        let at = self.open.iter().position(|call| {
            call.id == id && call.target == target
        })?;
        Some(self.open.remove(at))
    }

    /// Takes the call numbered `id`.
    pub(super) fn close(&mut self, id: u64) -> Option<Call> {
        let at = self.open.iter().position(|call| call.id == id)?;
        Some(self.open.remove(at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_that_answers_a_call_is_one_deeper() {
        let mut world = World::new();
        let [a, b, c] = [(); 3].map(|()| world.spawn_empty().id());
        let mut calls = Calls::default();

        assert_eq!(calls.depth_of(a), 1);
        let first =
            calls.open((a, 1), (b, "b"), calls.depth_of(a), 9);
        assert_eq!(calls.depth_of(b), 2);
        calls.open((b, 1), (c, "c"), calls.depth_of(b), 9);
        assert_eq!(calls.depth_of(c), 3);
        assert_eq!(calls.in_flight(b), 1);
        assert!(calls.waiting(a, 1) && !calls.waiting(a, 2));

        // Only the target answers, and once.
        assert!(calls.answer(first, c).is_none());
        assert!(calls.answer(first, b).is_some());
        assert!(calls.answer(first, b).is_none());
        assert_eq!(calls.depth_of(b), 1);
    }
}
