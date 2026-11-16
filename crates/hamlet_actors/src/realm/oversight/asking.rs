//! An agent's questions and objections to a chief: `AskChief`,
//! `Object`, and the `objection` and `held` tables.

use std::sync::Arc;

use anyhow::{Result, bail};
use hamlet_actor::Ctx;
use hamlet_messages::address::Agent;
use hamlet_messages::realm::client::Answer;
use hamlet_messages::realm::oversight::{
    AskChief, AskChiefRefusal, Hold, Object, ObjectRefusal,
};
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{AgentRefusal, Done};
use hamlet_messages::workspace::{AgentNumber, RealmTask};

use super::super::Realm;
use super::super::act::{Act, Decision, Unbuilt};
use super::super::calls::{Delivery, Made};
use super::super::change::{Affected, Rows, Writer};
use super::super::tasks::TaskId;
use super::super::waits::Ruling;

#[derive(Debug, PartialEq)]
pub(in crate::realm) enum Change {}

pub(in crate::realm) async fn apply(
    _writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {}
}

/// The hold on `agent`'s spending for `task`.
pub(in crate::realm) async fn held(
    _rows: &Rows<'_>,
    _agent: AgentNumber,
    _task: &RealmTask,
) -> Result<Option<Hold>> {
    Ok(None)
}

/// The change that keeps `delivery`, a letter or an answer to `to`,
/// while `to`'s `task` is held.
pub(in crate::realm) async fn holds(
    _rows: &Rows<'_>,
    _to: AgentNumber,
    _task: Option<TaskId>,
    _delivery: Delivery,
) -> Result<Option<Change>> {
    Ok(None)
}

/// What a question or an objection adds to the chief's `answer` on
/// wait `wait`: the words of the chief's picks, and for a withdraw
/// the task's end and its dropped letters.
pub(in crate::realm) async fn ruling(
    _rows: &Rows<'_>,
    _wait: WaitId,
    _answer: &Answer,
) -> Result<Ruling> {
    Ok(Ruling::default())
}

/// Delivers the letters and answers held on `task`.
pub(in crate::realm) async fn deliver_held(
    _realm: &Arc<Realm>,
    _ctx: &Ctx<Realm>,
    task: TaskId,
) -> Result<()> {
    bail!(
        "not built yet: U3, so {:?} is not made",
        Delivery::Held(task)
    )
}

impl Act<Agent> for AskChief {
    type Output = Done<AgentRefusal<AskChiefRefusal>>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Agent,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U3")
    }

    fn decide(
        &self,
        _by: &Agent,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}

impl Act<Agent> for Object {
    type Output = Done<AgentRefusal<ObjectRefusal>>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Agent,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U3")
    }

    fn decide(
        &self,
        _by: &Agent,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}
