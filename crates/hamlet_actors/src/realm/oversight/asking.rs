//! An agent's questions and objections to a chief: `AskChief`,
//! `Object`, and the `objection` and `held` tables (H10).

use std::sync::Arc;

use anyhow::{Context as _, Result, bail, ensure};
use hamlet_actor::{BindParam, Ctx};
use hamlet_messages::address::Agent;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::client::{Answer, AnswerRefusal};
use hamlet_messages::realm::oversight::{
    AskChief, AskChiefRefusal, Hold, MOST_OPTIONS, MOST_QUESTIONS,
    Object, ObjectRefusal, Offer, Question,
};
use hamlet_messages::realm::world::{
    ObjectionSource, WaitId, WaitKind,
};
use hamlet_messages::realm::{
    AgentRefusal, Done, LetterId, Refusable,
};
use hamlet_messages::workspace::{AgentNumber, RealmTask};
use hamlet_protocol::client::{self as protocol, Choice, Pick};
use serde::{Deserialize, Serialize};

use super::super::act::{Act, Decided, Decision};
use super::super::calls::{Delivery, Made};
use super::super::change::{self, Affected, Rows, Writer, number};
use super::super::tasks::{Origin, TaskId};
use super::super::waits::{self, Raised, Ruling};
use super::super::{Realm, letters, record, tree};

#[derive(Debug, PartialEq)]
pub(in crate::realm) enum Change {
    /// `agent` objects, on `wait`, to the chief's line of `task`.
    Object {
        task: TaskId,
        agent: AgentNumber,
        wait: WaitId,
        source: ObjectionSource,
    },
    /// `letter` waits for the ruling on the objection to `task`.
    Hold {
        letter: LetterId,
        kept: Kept,
        task: TaskId,
    },
    Go {
        task: TaskId,
    },
    /// The chief withdraws the line of `task`, which drops the
    /// letters held on it.
    Withdraw {
        task: TaskId,
    },
    /// The objection to `task` lapsed with its wait: the letters held
    /// on it are dropped.
    Lapse {
        task: TaskId,
    },
}

/// What of a letter is held: the letter itself, or its answer.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(in crate::realm) enum Kept {
    Letter,
    Answer,
}

pub(in crate::realm) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    let at = BindParam::Integer(writer.at().0);
    match change {
        Change::Object {
            task,
            agent,
            wait,
            source,
        } => {
            writer
                .execute(
                    "INSERT INTO objection (task, agent, wait, \
                     source, at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    vec![
                        task.column()?,
                        number(agent),
                        change::integer(wait.0)?,
                        BindParam::Text(change::case_name(&source)?),
                        at,
                    ],
                )
                .await?;
        }
        Change::Hold { letter, kept, task } => {
            writer
                .execute(
                    "INSERT INTO held (letter, what, task, at) \
                     VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
                    vec![
                        change::integer(letter.0)?,
                        BindParam::Text(change::case_name(&kept)?),
                        task.column()?,
                        at,
                    ],
                )
                .await?;
        }
        Change::Go { task } => {
            rule(writer, task, "go").await?;
        }
        Change::Withdraw { task } => {
            rule(writer, task, WITHDRAW).await?;
            drop_held(writer, task).await?;
        }
        Change::Lapse { task } => {
            drop_held(writer, task).await?;
        }
    }
    Ok(Affected::default())
}

const WITHDRAW: &str = "withdraw";

async fn drop_held(writer: &Writer<'_>, task: TaskId) -> Result<u64> {
    writer
        .execute(
            "DELETE FROM held WHERE task = ?1",
            vec![task.column()?],
        )
        .await
}

async fn rule(
    writer: &Writer<'_>,
    task: TaskId,
    ruling: &str,
) -> Result<u64> {
    writer
        .execute(
            "UPDATE objection SET ruling = ?1 WHERE task = ?2 AND \
             ruling IS NULL",
            vec![BindParam::Text(ruling.to_owned()), task.column()?],
        )
        .await
}

/// Whether `agent`'s objection to `task` waits for a chief's ruling:
/// its wait is open.
async fn objecting(
    rows: &Rows<'_>,
    agent: AgentNumber,
    task: TaskId,
) -> Result<bool> {
    Ok(rows
        .value::<bool>(
            "SELECT EXISTS (SELECT 1 FROM objection JOIN wait ON \
             wait.id = objection.wait WHERE objection.task = ?1 AND \
             objection.agent = ?2 AND wait.closed_at IS NULL)",
            vec![task.column()?, number(agent)],
        )
        .await?
        .unwrap_or(false))
}

/// Whether a chief withdrew the line of `task`, which ends it.
async fn withdrawn(rows: &Rows<'_>, task: TaskId) -> Result<bool> {
    Ok(rows
        .value::<bool>(
            "SELECT EXISTS (SELECT 1 FROM objection WHERE task = ?1 \
             AND ruling = ?2)",
            vec![
                task.column()?,
                BindParam::Text(WITHDRAW.to_owned()),
            ],
        )
        .await?
        .unwrap_or(false))
}

/// The hold on `agent`'s spending for `task`.
pub(in crate::realm) async fn held(
    rows: &Rows<'_>,
    agent: AgentNumber,
    task: &RealmTask,
) -> Result<Option<Hold>> {
    let Some(id) = TaskId::of(task) else {
        return Ok(None);
    };
    let task = task.clone();
    Ok(if withdrawn(rows, id).await? {
        Some(Hold::Withdrawn { task })
    } else if objecting(rows, agent, id).await? {
        Some(Hold::Objection { task })
    } else {
        None
    })
}

/// What becomes of a letter or an answer on a held or withdrawn task.
#[derive(Debug, PartialEq)]
pub(in crate::realm) enum Holding {
    /// It is kept until the chief rules on the objection.
    Held(Change),
    /// Its task was withdrawn, so it is never given.
    Dropped,
}

/// What becomes of `delivery`, a letter or an answer from `from` to
/// `to` on `task`: held while either objects to that task, dropped
/// once a chief withdrew it, else `None`.
pub(in crate::realm) async fn holds(
    rows: &Rows<'_>,
    from: AgentNumber,
    to: AgentNumber,
    task: Option<TaskId>,
    delivery: Delivery,
) -> Result<Option<Holding>> {
    let Some(task) = task else {
        return Ok(None);
    };
    if withdrawn(rows, task).await? {
        return Ok(Some(Holding::Dropped));
    }
    if !objecting(rows, from, task).await?
        && !objecting(rows, to, task).await?
    {
        return Ok(None);
    }
    let (letter, kept) = match delivery {
        Delivery::Letter(letter) => (letter, Kept::Letter),
        Delivery::Answer(letter) => (letter, Kept::Answer),
        other => bail!("{other:?} is not held"),
    };
    Ok(Some(Holding::Held(Change::Hold { letter, kept, task })))
}

/// What an objection adds to the chief's `answer` on wait `wait`:
/// the ruling on its task, and for a withdraw the task's end and its
/// dropped letters.
pub(in crate::realm) async fn ruling(
    rows: &Rows<'_>,
    wait: WaitId,
    answer: &Answer,
) -> Result<Ruling> {
    let task = rows
        .value::<TaskId>(
            "SELECT task FROM objection WHERE wait = ?1",
            vec![change::integer(wait.0)?],
        )
        .await?
        .with_context(|| {
            format!("objection wait {} has no row", wait.0)
        })?;
    let ruled = match answer.0.choice {
        Choice::Go => Some(Change::Go { task }),
        Choice::Withdraw => Some(Change::Withdraw { task }),
        _ => None,
    };
    Ok(Ruling {
        changes: ruled.into_iter().map(Into::into).collect(),
        ..Ruling::default()
    })
}

/// What a question adds to the chief's `answer` on wait `wait`: the
/// words of the chief's picks, or the refusal of a reply that does
/// not answer the questions as they allow.
pub(in crate::realm) async fn reply(
    rows: &Rows<'_>,
    wait: WaitId,
    answer: &Answer,
) -> Result<Result<Ruling, AnswerRefusal>> {
    let detail = rows
        .value::<Option<Vec<u8>>>(
            "SELECT detail FROM wait WHERE id = ?1",
            vec![change::integer(wait.0)?],
        )
        .await?
        .flatten();
    let asked = detail
        .map(|detail| ciborium::from_reader(detail.as_slice()))
        .transpose()?;
    let Some(WaitKind::Question { questions, .. }) = asked else {
        return Ok(Ok(Ruling::default()));
    };
    Ok(replied(&questions, &answer.0).map(|picked| Ruling {
        picked,
        ..Ruling::default()
    }))
}

/// The words of the chief's picks in `answer` to `questions`, `None`
/// for anything but picks alone.
fn replied(
    questions: &[Question],
    answer: &protocol::Answer,
) -> Result<Option<String>, AnswerRefusal> {
    if answer.choice != Choice::Reply {
        return Ok(None);
    }
    let worded = trimmed(answer.text.as_deref()).is_some();
    match answer.picks.as_deref() {
        // Picks with words are no reply.
        Some(_) if worded => Ok(None),
        Some(picks) => picked(questions, picks).map(Some),
        None if worded => in_words(questions).map(|()| None),
        None => Ok(None),
    }
}

/// Refuses a reply in words to anything but one question that takes
/// words.
fn in_words(questions: &[Question]) -> Result<(), AnswerRefusal> {
    match questions {
        [only] if only.text_input => Ok(()),
        [_] => Err(AnswerRefusal::TakesOption { question: 1 }),
        _ => Err(AnswerRefusal::Unequal),
    }
}

/// The words the chief's `picks` give in a reply to `questions`: each
/// question quoted over its answer.
fn picked(
    questions: &[Question],
    picks: &[Pick],
) -> Result<String, AnswerRefusal> {
    if picks.len() != questions.len() {
        return Err(AnswerRefusal::Unequal);
    }
    let answers = (1..)
        .zip(questions.iter().zip(picks))
        .map(|(question, (asked, pick))| match pick {
            Pick::Offer { index } => usize::try_from(*index)
                .ok()
                .and_then(|index| asked.options.get(index))
                .map(|offer| offer.title.clone())
                .ok_or(AnswerRefusal::NoOption { question }),
            Pick::Words { .. } if !asked.text_input => {
                Err(AnswerRefusal::TakesOption { question })
            }
            Pick::Words { text } => trimmed(Some(text))
                .ok_or(AnswerRefusal::NoWords { question }),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let quoted =
        questions.iter().zip(answers).map(|(asked, answer)| {
            let question =
                asked.question.split_whitespace().collect::<Vec<_>>();
            format!("> {}\n{answer}", question.join(" "))
        });
    Ok(quoted.collect::<Vec<_>>().join("\n\n"))
}

/// Delivers the letters and answers held on `task`.
pub(in crate::realm) async fn deliver_held(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    task: TaskId,
) -> Result<()> {
    super::super::reviews::deliver(realm, ctx, task).await?;
    let rows = Rows::outside(ctx)?;
    let held = rows
        .rows::<(u32, String)>(
            "SELECT letter, what FROM held WHERE task = ?1 ORDER BY \
             rowid",
            vec![task.column()?],
        )
        .await?;
    let mut failed = 0;
    for (letter, kept) in held {
        let letter = LetterId(letter);
        let delivery = match change::from_case_name(kept)? {
            Kept::Letter => Delivery::Letter(letter),
            Kept::Answer => Delivery::Answer(letter),
        };
        if let Err(error) =
            letters::deliver(realm, ctx, delivery).await
        {
            tracing::warn!(
                letter = letter.0,
                "a held letter was not delivered: {error:#}"
            );
            failed += 1;
        }
    }
    ensure!(
        failed == 0,
        "{failed} letters held on task {task:?} were not delivered"
    );
    Ok(())
}

/// `task`, when it is `agent`'s own and of one of `origins`.
async fn own_task(
    rows: &Rows<'_>,
    agent: AgentNumber,
    task: &RealmTask,
    origins: &[Origin],
) -> Result<Option<TaskId>> {
    let Some(task) = TaskId::of(task) else {
        return Ok(None);
    };
    let Some(origin) = rows
        .value::<String>(
            "SELECT origin FROM task WHERE id = ?1 AND agent = ?2",
            vec![task.column()?, number(agent)],
        )
        .await?
    else {
        return Ok(None);
    };
    for of in origins {
        if origin == change::case_name(of)? {
            return Ok(Some(task));
        }
    }
    Ok(None)
}

fn detail(kind: &WaitKind) -> Result<Vec<u8>> {
    let mut detail = Vec::new();
    ciborium::into_writer(kind, &mut detail)?;
    Ok(detail)
}

fn trimmed(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn more_than(len: usize, most: u32) -> bool {
    len > usize::try_from(most).unwrap_or(usize::MAX)
}

/// The questions as asked, trimmed, or the fault in their shape.
fn shaped(
    asked: &[Question],
) -> Result<Vec<Question>, AskChiefRefusal> {
    if asked.is_empty() {
        return Err(AskChiefRefusal::NoQuestions);
    }
    if more_than(asked.len(), MOST_QUESTIONS) {
        return Err(AskChiefRefusal::TooManyQuestions);
    }
    (1..).zip(asked).map(|(n, asked)| shape(n, asked)).collect()
}

/// The question `question`, counted from one.
fn shape(
    question: u32,
    asked: &Question,
) -> Result<Question, AskChiefRefusal> {
    let Some(text) = trimmed(Some(&asked.question)) else {
        return Err(AskChiefRefusal::Blank { question });
    };
    if more_than(asked.options.len(), MOST_OPTIONS) {
        return Err(AskChiefRefusal::Crowded { question });
    }
    let mut options = Vec::<Offer>::new();
    for offer in &asked.options {
        let Some(title) = trimmed(Some(&offer.title)) else {
            return Err(AskChiefRefusal::Untitled { question });
        };
        if options.iter().any(|kept| kept.title == title) {
            return Err(AskChiefRefusal::Twice { question, title });
        }
        options.push(Offer {
            title,
            description: trimmed(offer.description.as_deref()),
        });
    }
    // A question with nothing to pick is answered in words.
    let text_input = options.is_empty() || asked.text_input;
    Ok(Question {
        question: text,
        description: trimmed(asked.description.as_deref()),
        options,
        text_input,
    })
}

/// The questions as words, for a chief who reads them as text. One
/// plain question is its own words.
fn about(message: Option<&str>, questions: &[Question]) -> String {
    if let [only] = questions
        && message.is_none()
        && only.description.is_none()
        && only.options.is_empty()
    {
        return only.question.clone();
    }
    let mut lines =
        message.map(str::to_owned).into_iter().collect::<Vec<_>>();
    let numbered = questions.len() > 1;
    for (n, asked) in (1..).zip(questions) {
        lines.push(if numbered {
            format!("{n}. {}", asked.question)
        } else {
            asked.question.clone()
        });
        if let Some(description) = &asked.description {
            lines.push(format!("   {description}"));
        }
        for (letter, offer) in ('a'..).zip(&asked.options) {
            lines.push(match &offer.description {
                Some(description) => format!(
                    "   {letter}) {} - {description}",
                    offer.title
                ),
                None => format!("   {letter}) {}", offer.title),
            });
        }
        if asked.text_input && !asked.options.is_empty() {
            lines.push("   or in your own words".to_owned());
        }
    }
    lines.join("\n")
}

/// The wait a question or an objection opens.
pub(in crate::realm) struct Opening {
    wait: WaitId,
    raised: Raised,
}

fn refuse<R, D>(why: R) -> Decision<D, Done<AgentRefusal<R>>> {
    Decision::Answer(Done::refused(AgentRefusal::Act(why)))
}

impl Act<Agent> for AskChief {
    type Output = Done<AgentRefusal<AskChiefRefusal>>;
    type Loaded = Result<Opening, AskChiefRefusal>;
    type Done = ();

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Self::Loaded> {
        let chiefs =
            [Origin::Chief, Origin::Retry, Origin::Reflection];
        let Some(task) =
            own_task(rows, by.0, &self.task, &chiefs).await?
        else {
            return Ok(Err(AskChiefRefusal::NoChief));
        };
        let questions = match shaped(&self.questions) {
            Ok(questions) => questions,
            Err(why) => return Ok(Err(why)),
        };
        let asked = rows
            .value::<u32>(
                "SELECT COUNT(*) FROM wait WHERE agent = ?1 AND \
                 task = ?2 AND kind = ?3",
                vec![
                    number(by.0),
                    task.column()?,
                    BindParam::Text(change::case_name(
                        &waits::Kind::Question,
                    )?),
                ],
            )
            .await?
            .unwrap_or(0);
        let most =
            waits::limits_of(rows, by.0).await?.asking.questions;
        if asked >= most {
            return Ok(Err(AskChiefRefusal::TooMany { most }));
        }
        let message = trimmed(self.message.as_deref());
        Ok(Ok(Opening {
            wait: waits::next(rows).await?,
            raised: Raised {
                agent: by.0,
                kind: waits::Kind::Question,
                task: Some(task),
                conversation: Some(self.conversation.clone()),
                subject: about(message.as_deref(), &questions),
                detail: detail(&WaitKind::Question {
                    message,
                    questions,
                })?,
            },
        }))
    }

    fn decide(
        &self,
        _by: &Agent,
        loaded: Self::Loaded,
    ) -> Decision<(), Self::Output> {
        match loaded {
            Ok(Opening { wait, raised }) => Decision::Change(
                Decided::new(())
                    .change(waits::Change::Raise { wait, raised }),
            ),
            Err(why) => refuse(why),
        }
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

impl Act<Agent> for Object {
    type Output = Done<AgentRefusal<ObjectRefusal>>;
    type Loaded = Result<(TaskId, Opening), ObjectRefusal>;
    type Done = ();

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Self::Loaded> {
        let kind = tree::kind_of(rows, by.0).await?;
        let task = match kind {
            AgentKind::Worker | AgentKind::Errand => None,
            AgentKind::Hand | AgentKind::Hire => {
                own_task(rows, by.0, &self.task, &[Origin::Chief])
                    .await?
            }
        };
        let Some(task) = task else {
            return Ok(Err(ObjectRefusal::NotAChiefsLine));
        };
        let objected = rows
            .value::<bool>(
                "SELECT EXISTS (SELECT 1 FROM objection WHERE task \
                 = ?1)",
                vec![task.column()?],
            )
            .await?
            .unwrap_or(false);
        if objected {
            return Ok(Err(ObjectRefusal::AlreadyHeld));
        }
        let Some(reason) = trimmed(Some(&self.reason)) else {
            return Ok(Err(ObjectRefusal::NoReason));
        };
        let name = record::name_of(rows, by.0).await?;
        let opening = Opening {
            wait: waits::next(rows).await?,
            raised: Raised {
                agent: by.0,
                kind: waits::Kind::Objection,
                task: Some(task),
                conversation: Some(self.conversation.clone()),
                subject: format!("`{name}` objects: {reason}"),
                detail: detail(&WaitKind::Objection {
                    reason,
                    source: ObjectionSource::Agent,
                })?,
            },
        };
        Ok(Ok((task, opening)))
    }

    fn decide(
        &self,
        by: &Agent,
        loaded: Self::Loaded,
    ) -> Decision<(), Self::Output> {
        let (task, Opening { wait, raised }) = match loaded {
            Ok(objecting) => objecting,
            Err(why) => return refuse(why),
        };
        Decision::Change(
            Decided::new(())
                .change(waits::Change::Raise { wait, raised })
                .change(Change::Object {
                    task,
                    agent: by.0,
                    wait,
                    source: ObjectionSource::Agent,
                }),
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}
