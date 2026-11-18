//! The order of the deliveries to one agent: each waits for the
//! deliveries to the same agent that were lined up before it, so an
//! agent takes them in the order their acts committed. Deliveries to
//! other agents do not wait on each other.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use anyhow::Result;
use hamlet_messages::workspace::AgentNumber;
use tokio::sync::oneshot;

use super::calls::{Calls, Delivery};
use super::change::{Rows, integer, number};

/// The last turn taken in each agent's lane.
#[derive(Default)]
pub(super) struct Lanes(
    Mutex<HashMap<AgentNumber, oneshot::Receiver<()>>>,
);

/// A delivery's place in the lanes of the agents it reaches. Dropping
/// it, delivered or not, lets the next delivery to those agents go.
pub(super) struct Turn {
    before: Vec<oneshot::Receiver<()>>,
    _done: Vec<oneshot::Sender<()>>,
}

impl Turn {
    /// Waits until the deliveries before this one are done.
    pub async fn come(&mut self) {
        for before in self.before.drain(..) {
            // An error is a turn dropped: that delivery is done too.
            let _ = before.await;
        }
    }
}

/// A delivery with its turn.
pub(super) struct Queued {
    pub delivery: Delivery,
    pub turn: Turn,
}

/// An act's deliveries with their turns.
pub(super) struct Lined {
    pub before_answer: Vec<Queued>,
    pub after_answer: Vec<Queued>,
}

impl Lanes {
    /// Lines up `calls` behind every delivery lined up before them,
    /// those before the answer first. A commit lines up its own
    /// while its transaction is open, so the lanes keep the order of
    /// the commits.
    pub async fn line(
        &self,
        rows: &Rows<'_>,
        calls: Calls,
    ) -> Result<Lined> {
        let mut reaching = Vec::new();
        for delivery in
            calls.before_answer.iter().chain(&calls.after_answer)
        {
            reaching.push(receivers(rows, delivery).await?);
        }
        let mut lanes =
            self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut turns = reaching
            .into_iter()
            .map(|agents| take(&mut lanes, agents));
        let mut queue = |deliveries: Vec<Delivery>| {
            deliveries
                .into_iter()
                .zip(turns.by_ref())
                .map(|(delivery, turn)| Queued { delivery, turn })
                .collect()
        };
        Ok(Lined {
            before_answer: queue(calls.before_answer),
            after_answer: queue(calls.after_answer),
        })
    }
}

fn take(
    lanes: &mut HashMap<AgentNumber, oneshot::Receiver<()>>,
    agents: Vec<AgentNumber>,
) -> Turn {
    let mut turn = Turn {
        before: Vec::new(),
        _done: Vec::new(),
    };
    for agent in agents {
        let (done, next) = oneshot::channel();
        turn.before.extend(lanes.insert(agent, next));
        turn._done.push(done);
    }
    turn
}

/// The agents `delivery` reaches. A chief's answer goes to the
/// workspace and keeps no lane.
async fn receivers(
    rows: &Rows<'_>,
    delivery: &Delivery,
) -> Result<Vec<AgentNumber>> {
    let (sql, key) = match delivery {
        Delivery::Line { to, .. } => return Ok(vec![*to]),
        Delivery::ChiefAnswer(_) => return Ok(Vec::new()),
        Delivery::Letter(letter) => (
            "SELECT receiver FROM letter WHERE id = ?1",
            integer(letter.0)?,
        ),
        Delivery::Answer(question) => (
            "SELECT sender FROM letter WHERE id = ?1",
            integer(question.0)?,
        ),
        Delivery::Notice(wait) => {
            ("SELECT agent FROM wait WHERE id = ?1", integer(wait.0)?)
        }
        Delivery::WorkerAnswer(worker) => (
            "SELECT starter FROM worker WHERE agent = ?1 AND \
             starter IS NOT NULL",
            number(*worker),
        ),
        Delivery::Kept(task) => (
            "SELECT hire FROM review WHERE task = ?1",
            task.column()?,
        ),
        Delivery::Held(task) => (
            "SELECT hire FROM review WHERE task = ?1 UNION SELECT \
             CASE held.what WHEN 'answer' THEN letter.sender ELSE \
             letter.receiver END FROM held JOIN letter ON letter.id \
             = held.letter WHERE held.task = ?1",
            task.column()?,
        ),
    };
    Ok(rows
        .values::<u32>(sql, vec![key])
        .await?
        .into_iter()
        .map(AgentNumber)
        .collect())
}
