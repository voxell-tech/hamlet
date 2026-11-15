//! Letters, questions and answers along roads: the `letter` table,
//! and `Tell`, `Ask` and `Reply` (H30).

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U6 fills this file"
)]

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::address::Agent;
use hamlet_messages::realm::{
    AgentRefusal, Ask, Done, LetterId, LetterRefusal, Reply,
    ReplyRefusal, Tell,
};
use hamlet_messages::workspace::{AgentNumber, ConversationId};

use super::act::{Act, Decision};
use super::calls::{Calls, Delivery, Made};
use super::change::{Affected, Rows, Writer};
use super::roads::{Ends, Way};
use super::tasks::TaskId;
use super::{Realm, not_built};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Letter,
    Question,
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    Keep {
        letter: LetterId,
        kind: Kind,
        from: AgentNumber,
        to: AgentNumber,
        way: Way,
        text: String,
        task: Option<TaskId>,
        /// The conversation a question was asked from.
        conversation: Option<ConversationId>,
    },
    /// The answer to a question, kept once.
    Answer { question: LetterId, text: String },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Keep {
            letter,
            kind,
            from,
            to,
            way,
            text,
            task,
            conversation,
        } => {
            keep(
                writer,
                letter,
                kind,
                from,
                to,
                way,
                text,
                task,
                conversation,
            )
            .await
        }
        Change::Answer { question, text } => {
            keep_answer(writer, question, text).await
        }
    }
}

async fn keep(
    writer: &Writer<'_>,
    letter: LetterId,
    kind: Kind,
    from: AgentNumber,
    to: AgentNumber,
    way: Way,
    text: String,
    task: Option<TaskId>,
    conversation: Option<ConversationId>,
) -> Result<Affected> {
    todo!()
}

async fn keep_answer(
    writer: &Writer<'_>,
    question: LetterId,
    text: String,
) -> Result<Affected> {
    todo!()
}

/// The next letter's id.
pub(super) async fn fresh(rows: &Rows<'_>) -> Result<LetterId> {
    todo!()
}

/// Makes `delivery`, a letter or an answer, from its row as it is
/// now: `Submit` to the receiver under the letter's id, or to the
/// asker's conversation under the answer's.
pub(super) async fn deliver(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    delivery: Delivery,
) -> Result<()> {
    Err(not_built("U6"))
}

pub(super) struct Letter {
    ends: Ends,
    id: LetterId,
}

impl Act<Agent> for Tell {
    type Output = Done<AgentRefusal<LetterRefusal>>;
    type Loaded = Letter;
    type Done = LetterId;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Letter> {
        todo!()
    }

    fn decide(
        &self,
        by: &Agent,
        letter: Letter,
    ) -> Decision<LetterId, Self::Output> {
        todo!()
    }

    fn deliveries(&self, by: &Agent, letter: &LetterId) -> Calls {
        todo!()
    }

    fn answer(&self, letter: LetterId, made: &Made) -> Self::Output {
        todo!()
    }
}

impl Act<Agent> for Ask {
    type Output = Done<AgentRefusal<LetterRefusal>>;
    type Loaded = Letter;
    type Done = LetterId;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Letter> {
        todo!()
    }

    fn decide(
        &self,
        by: &Agent,
        letter: Letter,
    ) -> Decision<LetterId, Self::Output> {
        todo!()
    }

    fn deliveries(&self, by: &Agent, letter: &LetterId) -> Calls {
        todo!()
    }

    fn answer(&self, letter: LetterId, made: &Made) -> Self::Output {
        todo!()
    }
}

pub(super) struct Question {
    pub asked: AgentNumber,
}

impl Act<Agent> for Reply {
    type Output = Done<ReplyRefusal>;
    type Loaded = Option<Question>;
    type Done = LetterId;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Option<Question>> {
        todo!()
    }

    fn decide(
        &self,
        by: &Agent,
        question: Option<Question>,
    ) -> Decision<LetterId, Self::Output> {
        todo!()
    }

    fn deliveries(&self, by: &Agent, question: &LetterId) -> Calls {
        todo!()
    }

    fn answer(
        &self,
        question: LetterId,
        made: &Made,
    ) -> Self::Output {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use hamlet_messages::realm::{HamletId, Named};

    use super::*;
    use crate::realm::hire;
    use crate::realm::roads::End;
    use crate::realm::tree::Status;

    fn end(agent: u32, lead: Option<u32>) -> End {
        End {
            agent: AgentNumber(agent),
            status: Status::Live,
            lead: lead.map(AgentNumber),
            hamlet: Some(HamletId("h".into())),
            is_mayor: lead.is_none(),
        }
    }

    fn tell(to: u32) -> Tell {
        Tell {
            to: Named::Agent {
                agent: AgentNumber(to),
            },
            text: "hi".into(),
            task: None,
        }
    }

    fn sender() -> Agent {
        Agent(AgentNumber(3))
    }

    #[test]
    #[ignore = "the scaffold: U6 fills it"]
    fn a_tell_with_no_road_is_refused() {
        let letter = Letter {
            ends: Ends {
                from: end(3, Some(2)),
                to: Some(end(4, Some(2))),
                laid: false,
            },
            id: LetterId(1),
        };
        assert_eq!(
            tell(4).decide(&sender(), letter),
            Decision::Answer(Done::Refused {
                why: AgentRefusal::Act(LetterRefusal::NoRoad {
                    to: AgentNumber(4)
                })
            })
        );
    }

    #[test]
    #[ignore = "the scaffold: U6 fills it"]
    fn a_tell_to_a_hire_still_hiring_waits_for_its_steps() {
        let mut to = end(4, Some(3));
        to.status = Status::Hiring;
        let letter = Letter {
            ends: Ends {
                from: end(3, Some(2)),
                to: Some(to),
                laid: false,
            },
            id: LetterId(1),
        };
        assert!(matches!(
            tell(4).decide(&sender(), letter),
            Decision::WaitFor { row, .. } if row == hire::row(AgentNumber(4))
        ));
    }
}
