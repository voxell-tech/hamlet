//! Letters, questions and answers along roads: the `letter` table,
//! and `Tell`, `Ask` and `Reply` (H30).

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use hamlet_actor::{BindParam, Ctx};
use hamlet_messages::address::Agent;
use hamlet_messages::agent::{Author, Input, InputId, Submit};
use hamlet_messages::realm::{
    AgentRefusal, Ask, Done, Enclosed, LetterId, LetterRefusal,
    Named, Refusable, Reply, ReplyRefusal, Tell,
};
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, RealmTask,
};
use serde::{Deserialize, Serialize};

use super::act::{Act, Decided, Decision, Paid, ToolAct};
use super::calls::{Calls, Delivery, Made};
use super::change::{self, Affected, Rows, Writer, number};
use super::oversight::asking;
use super::peer::When;
use super::roads::{self, End, Ends, Way};
use super::tasks::TaskId;
use super::tree::{self, Status};
use super::{Realm, hire, laid, parcels};

type Sent = Done<AgentRefusal<LetterRefusal>>;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
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
    /// The letter is given to its receiver, which may name its task
    /// from now on.
    Given { letter: LetterId },
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
        Change::Given { letter } => {
            writer
                .execute(
                    "UPDATE letter SET given_at = ?2 WHERE id = ?1 \
                     AND given_at IS NULL",
                    vec![
                        change::integer(letter.0)?,
                        BindParam::Integer(writer.at().0),
                    ],
                )
                .await?;
            Ok(Affected::default())
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
    writer
        .execute(
            "INSERT INTO letter (id, kind, sender, receiver, way, \
             text, task, conversation, at) VALUES (?1, ?2, ?3, ?4, \
             ?5, ?6, ?7, ?8, ?9)",
            vec![
                change::integer(letter.0)?,
                BindParam::Text(change::case_name(&kind)?),
                number(from),
                number(to),
                BindParam::Text(change::case_name(&way)?),
                BindParam::Text(text),
                task.map(TaskId::column)
                    .transpose()?
                    .unwrap_or(BindParam::Null),
                conversation
                    .map_or(BindParam::Null, |conversation| {
                        BindParam::Text(conversation.0)
                    }),
                BindParam::Integer(writer.at().0),
            ],
        )
        .await?;
    Ok(Affected::default())
}

async fn keep_answer(
    writer: &Writer<'_>,
    question: LetterId,
    text: String,
) -> Result<Affected> {
    writer
        .execute(
            "UPDATE letter SET answer = ?2 WHERE id = ?1 AND answer \
             IS NULL",
            vec![change::integer(question.0)?, BindParam::Text(text)],
        )
        .await?;
    Ok(Affected::default())
}

/// The next letter's id.
pub(super) async fn fresh(rows: &Rows<'_>) -> Result<LetterId> {
    Ok(LetterId(u32::try_from(rows.next_id("letter").await?)?))
}

/// Makes `delivery`, a letter or an answer, from its row as it is
/// now: `Submit` to the receiver under the letter's id, or to the
/// asker's conversation under the answer's. A letter is marked given
/// before its receiver is called.
pub(super) async fn deliver(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    delivery: Delivery,
) -> Result<()> {
    let rows = Rows::outside(ctx)?;
    let (to, submit) = match delivery {
        Delivery::Letter(letter) => {
            change::commit(
                realm,
                ctx,
                vec![Change::Given { letter }.into()],
            )
            .await?;
            letter_input(&rows, letter).await?
        }
        Delivery::Answer(question) => {
            answer_input(&rows, question).await?
        }
        other => bail!("{other:?} is not a letter's"),
    };
    realm
        .peers
        .call(&Agent(to), &submit, When::AfterAnswer)
        .await
}

async fn letter_input(
    rows: &Rows<'_>,
    letter: LetterId,
) -> Result<(AgentNumber, Submit)> {
    let (kind, sender, receiver, text, task) = rows
        .row::<(String, u32, u32, String, Option<TaskId>)>(
            "SELECT kind, sender, receiver, text, task FROM letter \
             WHERE id = ?1",
            vec![change::integer(letter.0)?],
        )
        .await?
        .with_context(|| format!("no letter {}", letter.0))?;
    let from = author(sender);
    let files = parcels::files(rows, letter).await?;
    let input = match change::from_case_name::<Kind>(kind)? {
        Kind::Letter => Input::Letter { from, text, files },
        Kind::Question => Input::Question {
            letter,
            from,
            text,
            files,
        },
    };
    let submit = Submit {
        id: InputId(format!("letter/{}", letter.0)),
        task: task.map(TaskId::on_the_wire),
        input,
    };
    Ok((AgentNumber(receiver), submit))
}

async fn answer_input(
    rows: &Rows<'_>,
    question: LetterId,
) -> Result<(AgentNumber, Submit)> {
    let found =
        rows.row::<(
            u32,
            u32,
            Option<String>,
            Option<String>,
            Option<TaskId>,
        )>(
            "SELECT sender, receiver, answer, conversation, task \
             FROM letter WHERE id = ?1",
            vec![change::integer(question.0)?],
        )
        .await?;
    let Some((asker, asked, Some(text), Some(conversation), task)) =
        found
    else {
        bail!("question {} has no answer", question.0);
    };
    let submit = Submit {
        id: InputId(format!("answer/{}", question.0)),
        task: task.map(TaskId::on_the_wire),
        input: Input::Answer {
            question,
            from: author(asked),
            text,
            conversation: ConversationId(conversation),
        },
    };
    Ok((AgentNumber(asker), submit))
}

fn author(agent: u32) -> Author {
    Author {
        agent: AgentNumber(agent),
        name: None,
    }
}

/// What becomes of a letter or an answer once it is kept.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub(super) enum Route {
    /// It is delivered at once.
    Now(LetterId),
    /// It is kept while its receiver's task is held.
    Held(LetterId),
    /// Its parcel's steps deliver it once its files are copied.
    Parcel(LetterId),
    /// Its task was withdrawn: it is never given.
    Dropped(LetterId),
}

impl Route {
    /// The kept letter or answer, routed by the changes that hold it
    /// or send its files.
    fn of(
        kept: Decided<LetterId>,
        holding: Option<asking::Holding>,
        parcel: Option<Vec<change::Change>>,
    ) -> Decided<Self> {
        let Decided {
            done: letter,
            mut changes,
            entries,
        } = kept;
        let done = match (holding, parcel) {
            (Some(asking::Holding::Dropped), _) => {
                Self::Dropped(letter)
            }
            (_, Some(parcel)) => {
                changes.extend(parcel);
                Self::Parcel(letter)
            }
            (Some(asking::Holding::Held(held)), None) => {
                changes.push(held.into());
                Self::Held(letter)
            }
            (None, None) => Self::Now(letter),
        };
        Decided {
            done,
            changes,
            entries,
        }
    }

    fn calls(self, delivery: fn(LetterId) -> Delivery) -> Calls {
        match self {
            Self::Now(letter) => {
                Calls::after_answer(delivery(letter))
            }
            Self::Held(_) | Self::Parcel(_) | Self::Dropped(_) => {
                Calls::none()
            }
        }
    }
}

pub(super) struct Letter {
    ends: Ends,
    /// The receiver as the sender named it.
    address: String,
    id: LetterId,
    /// What becomes of it while its task is held or withdrawn.
    held: Option<asking::Holding>,
    /// The changes that send its files.
    parcel: Option<Vec<change::Change>>,
}

impl Letter {
    async fn load(
        rows: &Rows<'_>,
        by: &Agent,
        to: &Named,
        task: Option<&RealmTask>,
        paths: &[Enclosed],
    ) -> Result<Self> {
        let ends = roads::ends(rows, by, to).await?;
        let id = fresh(rows).await?;
        let held = match &ends.to {
            Some(to) => {
                let task = task.and_then(TaskId::of);
                let letter = Delivery::Letter(id);
                asking::holds(rows, by.0, to.agent, task, letter)
                    .await?
            }
            None => None,
        };
        let parcel = if paths.is_empty()
            || held == Some(asking::Holding::Dropped)
        {
            None
        } else {
            parcels::start(rows, id, paths).await?
        };
        Ok(Self {
            ends,
            address: to.to_string(),
            id,
            held,
            parcel,
        })
    }
}

pub(super) struct Written<'a> {
    pub kind: Kind,
    pub text: &'a str,
    pub task: Option<&'a RealmTask>,
    pub conversation: Option<&'a ConversationId>,
}

impl Written<'_> {
    fn decide(self, letter: Letter) -> Decision<Route, Sent> {
        let refuse = |why| {
            Decision::Answer(Done::refused(AgentRefusal::Act(why)))
        };
        let Letter {
            ends,
            address,
            id,
            held,
            parcel,
        } = letter;
        let Some(to) = &ends.to else {
            return refuse(LetterRefusal::NoSuchAgent { address });
        };
        if to.agent == ends.from.agent {
            return refuse(LetterRefusal::Yourself);
        }
        if self.text.trim().is_empty() {
            return refuse(LetterRefusal::Empty);
        }
        if let Status::Ended(left) = &to.status {
            return refuse(LetterRefusal::Left {
                to: to.agent,
                left: left.clone(),
            });
        }
        let Some(way) = ends.way() else {
            return refuse(LetterRefusal::NoRoad { to: to.agent });
        };
        if to.status == Status::Hiring {
            return Decision::WaitFor {
                row: hire::row(to.agent),
                refusal: Done::refused(AgentRefusal::Act(
                    LetterRefusal::StillHiring { to: to.agent },
                )),
            };
        }
        let kept = self.keep(id, ends.from.agent, to, way);
        Decision::Change(Route::of(kept, held, parcel))
    }

    pub fn keep(
        self,
        id: LetterId,
        from: AgentNumber,
        to: &End,
        way: Way,
    ) -> Decided<LetterId> {
        let mut decided = Decided::new(id).change(Change::Keep {
            letter: id,
            kind: self.kind,
            from,
            to: to.agent,
            way,
            text: self.text.to_owned(),
            task: self.task.and_then(TaskId::of),
            conversation: self.conversation.cloned(),
        });
        if to.can_take_purpose
            && let Some(purpose) =
                tree::purpose_from(to.agent, self.text)
        {
            decided = decided.change(purpose);
        }
        if way == Way::Laid {
            decided = decided.change(laid::Change::Used {
                between: (from, to.agent),
            });
        }
        decided
    }
}

impl Act<Agent> for Paid<Tell> {
    type Output = Sent;
    type Loaded = Letter;
    type Done = Route;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Letter> {
        Letter::load(
            rows,
            by,
            &self.act.to,
            self.task.as_ref(),
            &self.act.paths,
        )
        .await
    }

    fn decide(
        &self,
        _by: &Agent,
        letter: Letter,
    ) -> Decision<Route, Sent> {
        let written = Written {
            kind: Kind::Letter,
            text: &self.act.text,
            task: self.task.as_ref(),
            conversation: None,
        };
        written.decide(letter)
    }

    fn deliveries(&self, _by: &Agent, route: &Route) -> Calls {
        route.calls(Delivery::Letter)
    }

    fn answer(&self, _route: Route, _made: &Made) -> Sent {
        Done::Done
    }
}

impl ToolAct for Tell {
    type Run = Paid<Tell>;

    fn run(self, task: Option<RealmTask>) -> Paid<Tell> {
        Paid { task, act: self }
    }
}

impl Act<Agent> for Paid<Ask> {
    type Output = Sent;
    type Loaded = Letter;
    type Done = Route;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Letter> {
        Letter::load(
            rows,
            by,
            &self.act.to,
            self.task.as_ref(),
            &self.act.paths,
        )
        .await
    }

    fn decide(
        &self,
        _by: &Agent,
        letter: Letter,
    ) -> Decision<Route, Sent> {
        let written = Written {
            kind: Kind::Question,
            text: &self.act.text,
            task: self.task.as_ref(),
            conversation: Some(&self.act.conversation),
        };
        written.decide(letter)
    }

    fn deliveries(&self, _by: &Agent, route: &Route) -> Calls {
        route.calls(Delivery::Letter)
    }

    fn answer(&self, _route: Route, _made: &Made) -> Sent {
        Done::Done
    }
}

impl ToolAct for Ask {
    type Run = Paid<Ask>;

    fn run(self, task: Option<RealmTask>) -> Paid<Ask> {
        Paid { task, act: self }
    }
}

pub(super) struct Question {
    pub asked: AgentNumber,
    /// What becomes of the answer while its task is held or
    /// withdrawn.
    pub held: Option<asking::Holding>,
}

impl Act<Agent> for Reply {
    type Output = Done<ReplyRefusal>;
    type Loaded = Option<Question>;
    type Done = Route;

    async fn load(
        &self,
        _by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Option<Question>> {
        let found = rows
            .row::<(u32, u32, Option<TaskId>)>(
                "SELECT receiver, sender, task FROM letter WHERE id \
                 = ?1 AND kind = ?2",
                vec![
                    change::integer(self.question.0)?,
                    BindParam::Text(change::case_name(
                        &Kind::Question,
                    )?),
                ],
            )
            .await?;
        let Some((asked, asker, task)) = found else {
            return Ok(None);
        };
        let answer = Delivery::Answer(self.question);
        let (asked, asker) = (AgentNumber(asked), AgentNumber(asker));
        Ok(Some(Question {
            asked,
            held: asking::holds(rows, asked, asker, task, answer)
                .await?,
        }))
    }

    fn decide(
        &self,
        by: &Agent,
        question: Option<Question>,
    ) -> Decision<Route, Self::Output> {
        let Some(question) = question else {
            return Decision::Answer(Done::refused(
                ReplyRefusal::NoSuchQuestion,
            ));
        };
        if question.asked != by.0 {
            return Decision::Answer(Done::refused(
                ReplyRefusal::NotAsked,
            ));
        }
        let kept =
            Decided::new(self.question).change(Change::Answer {
                question: self.question,
                text: self.text.clone(),
            });
        Decision::Change(Route::of(kept, question.held, None))
    }

    fn deliveries(&self, _by: &Agent, route: &Route) -> Calls {
        route.calls(Delivery::Answer)
    }

    fn answer(&self, _route: Route, _made: &Made) -> Self::Output {
        Done::Done
    }
}

#[cfg(test)]
mod tests {
    use hamlet_messages::realm::{HamletId, Named};

    use super::*;
    use crate::realm::roads::End;

    fn end(agent: u32, lead: Option<u32>) -> End {
        End {
            agent: AgentNumber(agent),
            status: Status::Live,
            lead: lead.map(AgentNumber),
            hamlet: Some(HamletId("h".into())),
            is_mayor: lead.is_none(),
            can_take_purpose: false,
        }
    }

    fn tell(to: u32) -> Tell {
        Tell {
            to: Named::Agent {
                agent: AgentNumber(to),
            },
            text: "hi".into(),
            paths: Vec::new(),
        }
    }

    fn sender() -> Agent {
        Agent(AgentNumber(3))
    }

    #[test]
    fn a_tell_with_no_road_is_refused() {
        let letter = Letter {
            ends: Ends {
                from: end(3, Some(2)),
                to: Some(end(4, Some(2))),
                laid: false,
            },
            address: "hire#4".to_owned(),
            id: LetterId(1),
            held: None,
            parcel: None,
        };
        assert_eq!(
            Paid {
                task: None,
                act: tell(4)
            }
            .decide(&sender(), letter),
            Decision::Answer(Done::Refused {
                why: AgentRefusal::Act(LetterRefusal::NoRoad {
                    to: AgentNumber(4)
                })
            })
        );
    }

    #[test]
    fn a_question_to_a_hire_with_no_purpose_gives_it_one() {
        let mut to = end(4, Some(3));
        to.can_take_purpose = true;
        let letter = Letter {
            ends: Ends {
                from: end(3, Some(2)),
                to: Some(to),
                laid: false,
            },
            address: "hire#4".to_owned(),
            id: LetterId(1),
            held: None,
            parcel: None,
        };
        let ask = Ask {
            to: Named::Agent {
                agent: AgentNumber(4),
            },
            text: "Why the well?".into(),
            conversation: ConversationId("c".into()),
            paths: Vec::new(),
        };

        let Decision::Change(decided) = Paid {
            task: None,
            act: ask,
        }
        .decide(&sender(), letter) else {
            panic!("3 asks its hire 4");
        };
        assert!(
            decided.changes.contains(
                &tree::Change::Purpose {
                    hire: AgentNumber(4),
                    text: "Why the well?".to_owned(),
                }
                .into()
            )
        );
    }

    #[test]
    fn a_tell_to_a_hire_still_hiring_waits_for_its_steps() {
        let mut to = end(4, Some(3));
        to.status = Status::Hiring;
        let letter = Letter {
            ends: Ends {
                from: end(3, Some(2)),
                to: Some(to),
                laid: false,
            },
            address: "hire#4".to_owned(),
            id: LetterId(1),
            held: None,
            parcel: None,
        };
        assert!(matches!(
            Paid { task: None, act: tell(4) }.decide(&sender(), letter),
            Decision::WaitFor { row, .. } if row == hire::row(AgentNumber(4))
        ));
    }
}
