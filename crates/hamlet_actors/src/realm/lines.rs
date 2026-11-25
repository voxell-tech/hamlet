//! The chief's lines the realm keeps for their agents until they
//! take them, the stops that withdraw them, and each agent's report
//! of its open inputs. Memory only.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use hamlet_actor::{Ctx, Handler};
use hamlet_messages::agent::chat::Withdrawn;
use hamlet_messages::agent::{Input, InputId, Submit};
use hamlet_messages::realm::lines::{LineIs, ReadLine, Working};
use hamlet_messages::sender::{Client, sender};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;
use tokio::sync::Notify;
use tokio::sync::futures::Notified;

use super::Realm;
use super::act::Look;
use super::calls::{Calls, Delivery};

/// How long the realm keeps what became of a line no one read.
const KEPT: Duration = Duration::from_secs(600);

pub(super) struct Lines {
    held: Mutex<Held>,
    /// Wakes the deliveries waiting between their tries at each
    /// stop.
    stopped: Notify,
}

struct Held {
    /// The number the last stop took.
    number: u64,
    /// The number the last line entered took, which orders them.
    entered: u64,
    lines: HashMap<InputId, Line>,
    /// The stops not yet done, for which the realm keeps the lines
    /// they withdrew.
    owed: HashSet<u64>,
    reports: HashMap<AgentNumber, Report>,
}

struct Line {
    agent: AgentNumber,
    entered: u64,
    input: Input,
    is: Is,
    /// The stop that withdrew it, unless the try under way when the
    /// stop came gives it.
    stop: Option<u64>,
    /// Whether a delivery carries it.
    carried: bool,
}

#[derive(Clone, Copy)]
enum Is {
    /// Kept for its hire's lead to review.
    Reviewed,
    Waiting,
    /// A try to give it is under way.
    Trying,
    Given(Instant),
    Withdrawn(Instant),
}

/// What an agent last told of its open inputs.
#[derive(Default)]
struct Report {
    open: bool,
    /// Whether a line was given to it since.
    given_since: bool,
}

impl Default for Lines {
    /// Stop numbers count up from the wall clock in microseconds, so
    /// each start's are above an earlier start's, and below 2^53, the
    /// largest an agent reads whole.
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_micros());
        Self {
            held: Mutex::new(Held {
                number: u64::try_from(now).unwrap_or(0),
                entered: 0,
                lines: HashMap::new(),
                owed: HashSet::new(),
                reports: HashMap::new(),
            }),
            stopped: Notify::new(),
        }
    }
}

impl Lines {
    fn held(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A stop's number, which no stop took before.
    pub fn number(&self) -> u64 {
        let mut held = self.held();
        held.number += 1;
        held.number
    }

    /// Enters the chief's lines `calls` lines up and the lines `kept`
    /// for a review, by their hires, and withdraws for each stop
    /// `calls` lines up its agent's lines not yet given. A commit
    /// enters its own while its transaction is open, so a stop finds
    /// every line committed before it.
    pub fn enter(
        &self,
        calls: &Calls,
        kept: &[(AgentNumber, Submit)],
    ) {
        let mut held = self.held();
        let now = Instant::now();
        let Held { lines, owed, .. } = &mut *held;
        lines.retain(|_, line| match line.is {
            Is::Given(at) | Is::Withdrawn(at) => {
                line.owed(owed) || now.duration_since(at) < KEPT
            }
            Is::Reviewed | Is::Waiting | Is::Trying => true,
        });
        let mut stopped = false;
        for delivery in
            calls.before_answer.iter().chain(&calls.after_answer)
        {
            if let Some((to, submit)) = delivery.said()
                && !held.lines.contains_key(&submit.id)
            {
                held.enter(to, submit, Is::Waiting);
            }
            if let Delivery::Stop { to, number } = delivery {
                held.withdraw(*to, *number);
                held.owed.insert(*number);
                stopped = true;
            }
        }
        for (hire, line) in kept {
            held.enter(*hire, line, Is::Reviewed);
        }
        drop(held);
        if stopped {
            self.stopped.notify_waiters();
        }
    }

    /// Takes the line `submit` to `to` for the one delivery that
    /// carries it: false when another carries it or carried it. A
    /// line the realm does not hold, as once it started again, is
    /// entered.
    pub fn carry(&self, to: AgentNumber, submit: &Submit) -> bool {
        let mut held = self.held();
        if !held.lines.contains_key(&submit.id) {
            held.enter(to, submit, Is::Waiting);
        }
        held.lines.get_mut(&submit.id).is_some_and(|line| {
            !std::mem::replace(&mut line.carried, true)
        })
    }

    /// Begins a try to give the line `id`: false, and the line is
    /// withdrawn, when a stop withdrew it. A line the realm does not
    /// hold, as once it started again, is tried.
    pub fn attempt(&self, id: &InputId) -> bool {
        let mut held = self.held();
        let Some(line) = held.lines.get_mut(id) else {
            return true;
        };
        if line.stop.is_some() {
            line.is = Is::Withdrawn(Instant::now());
            return false;
        }
        line.is = Is::Trying;
        true
    }

    /// The try to give the line `id` failed in passing.
    pub fn failed(&self, id: &InputId) {
        self.set(id, Is::Waiting);
    }

    /// The line `id` will not reach its agent: the agent refused it
    /// or left.
    pub fn lost(&self, id: &InputId) {
        self.set(id, Is::Withdrawn(Instant::now()));
    }

    /// The line `id` reached its agent.
    pub fn given(&self, id: &InputId) {
        let mut held = self.held();
        let Held { lines, reports, .. } = &mut *held;
        if let Some(line) = lines.get_mut(id) {
            line.is = Is::Given(Instant::now());
            reports.entry(line.agent).or_default().given_since = true;
        }
    }

    fn set(&self, id: &InputId, is: Is) {
        if let Some(line) = self.held().lines.get_mut(id) {
            line.is = is;
        }
    }

    /// The lines the stop `number` of `agent` withdrew, in the order
    /// they were said, as its `Stop` names them when it is sent.
    pub fn withdrawn(
        &self,
        agent: AgentNumber,
        number: u64,
    ) -> Vec<Withdrawn> {
        let held = self.held();
        let mut withdrawn = held
            .lines
            .iter()
            .filter(|(_, line)| {
                line.agent == agent
                    && line.stop == Some(number)
                    && matches!(
                        line.is,
                        Is::Reviewed | Is::Waiting | Is::Withdrawn(_)
                    )
            })
            .map(|(id, line)| (line.entered, id, line))
            .collect::<Vec<_>>();
        withdrawn.sort_by_key(|(entered, ..)| *entered);
        withdrawn
            .into_iter()
            .map(|(_, id, line)| Withdrawn {
                id: id.clone(),
                line: line.input.clone(),
            })
            .collect()
    }

    /// The stop `number` is done: the lines it withdrew are kept for
    /// it no longer.
    pub fn done(&self, number: u64) {
        self.held().owed.remove(&number);
    }

    /// What became of the line said under `id`. Once it was given or
    /// withdrawn, the realm forgets it as it answers, unless a stop
    /// not yet done names it.
    pub fn read(&self, id: &RequestId) -> LineIs {
        let id = InputId(id.as_str().to_owned());
        let mut held = self.held();
        let Some(line) = held.lines.get(&id) else {
            return LineIs::Withdrawn;
        };
        let owed = line.owed(&held.owed);
        match (line.is, line.stop) {
            (Is::Given(_), _) => {
                held.lines.remove(&id);
                LineIs::Given
            }
            (Is::Withdrawn(_), _) => {
                if !owed {
                    held.lines.remove(&id);
                }
                LineIs::Withdrawn
            }
            (Is::Reviewed | Is::Waiting, Some(_)) => {
                LineIs::Withdrawn
            }
            (Is::Reviewed | Is::Waiting | Is::Trying, _) => {
                LineIs::Waiting
            }
        }
    }

    pub fn working(&self, agent: AgentNumber, open: bool) {
        self.held().reports.insert(
            agent,
            Report {
                open,
                given_since: false,
            },
        );
    }

    /// The realm's count of the turns the stop `number` of `agent`
    /// stopped, when its `Stop` was not answered in time: one when
    /// the agent last told it was working, a line was given to it
    /// since, or the stop withdrew a line; else none.
    pub fn counted(&self, agent: AgentNumber, number: u64) -> u32 {
        let held = self.held();
        let working = held
            .reports
            .get(&agent)
            .is_some_and(|report| report.open || report.given_since);
        let withdrew = held.lines.values().any(|line| {
            line.agent == agent && line.stop == Some(number)
        });
        u32::from(working || withdrew)
    }

    /// Wakes at the next stop.
    pub fn stopping(&self) -> Notified<'_> {
        self.stopped.notified()
    }
}

impl Line {
    /// Whether a stop not yet done withdrew it.
    fn owed(&self, owed: &HashSet<u64>) -> bool {
        self.stop.is_some_and(|number| owed.contains(&number))
    }
}

impl Held {
    fn enter(&mut self, agent: AgentNumber, submit: &Submit, is: Is) {
        self.entered += 1;
        self.lines.insert(
            submit.id.clone(),
            Line {
                agent,
                entered: self.entered,
                input: submit.input.clone(),
                is,
                stop: None,
                carried: false,
            },
        );
    }

    fn withdraw(&mut self, agent: AgentNumber, number: u64) {
        for line in self.lines.values_mut() {
            if line.agent == agent
                && line.stop.is_none()
                && matches!(
                    line.is,
                    Is::Reviewed | Is::Waiting | Is::Trying
                )
            {
                line.stop = Some(number);
            }
        }
    }
}

impl Look<Client> for ReadLine {
    type Output = LineIs;

    fn look(&self, _by: &Client, realm: &Realm) -> LineIs {
        realm.lines.read(&self.0)
    }
}

impl Handler<Working> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        working: Working,
    ) -> Result<()> {
        let agent = sender(&ctx, &working)?;
        self.lines.working(agent.0, working.open);
        Ok(())
    }
}
