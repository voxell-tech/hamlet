//! The librarian, an errand that answers an agent from the shelves
//! or judges a skill it offered: the `librarian` table, the call each
//! such errand answers, and the step that brings a verdict to the
//! library.

use anyhow::{Context as _, Result};
use hamlet_actor::BindParam;
use hamlet_messages::address::{Agent, Library};
use hamlet_messages::agent::{Input, InputId, Submit};
use hamlet_messages::library::{OfferId, ReadShelves, Verdict};
use hamlet_messages::realm::profiles::{AskLibrarian, SubmitSkill};
use hamlet_messages::realm::workers::Ended;
use hamlet_messages::realm::{Done, Standing};
use hamlet_messages::workspace::{
    AgentNumber, CallId, ConversationId, ErrandNumber,
};
use serde::{Deserialize, Serialize};

use super::act::{Act, Called, Decided, Decision, ToolAct};
use super::calls::{self, Delivery};
use super::change::{self, Affected, Rows, Writer, integer, number};
use super::peer::{self, When};
use super::steps::{self, Cx, Kind, StepId, Steps};
use super::tasks::{self, TaskId};
use super::workers::errands::{self, ErrandInput, Purpose};
use super::{documents, profiles, record, tree};

/// The librarian's part before every errand, in today's words.
const ROLE: &str =
    "You are the realm's librarian. You keep the library of skills \
     every agent shares: you know what is on its shelves, you take \
     a skill offered only when it is useful to others, distinct \
     from the shelves, clear enough to run and free of private \
     names, keys and paths, and you never invent a skill.";

const LOOKING: &str = "Name the skills on the shelves that do what \
                       is asked, with their exact commands. If \
                       nothing fits, say so.";

const JUDGING: &str = "Answer with ACCEPT or REFUSE as your first \
                       word, then say why in a sentence or two.";

/// A skill an agent offered the library, to be judged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Offered {
    offer: OfferId,
    skill: String,
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// Errand `errand` answers `asker`'s call `call`, made in
    /// `conversation`, or judges what it `offered`.
    Begin {
        errand: ErrandNumber,
        asker: AgentNumber,
        call: CallId,
        conversation: Option<ConversationId>,
        offered: Option<Offered>,
    },
    /// Errand `errand` ended as `ended`: its asker is told, or its
    /// verdict is brought to the library first.
    Ended { errand: ErrandNumber, ended: Ended },
    /// The library took the verdict of errand `errand`: its offerer
    /// is told.
    Judged { errand: ErrandNumber },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Begin {
            errand,
            asker,
            call,
            conversation,
            offered,
        } => {
            let (offer, skill) = match offered {
                Some(Offered { offer, skill }) => (
                    BindParam::Integer(offer.0.into()),
                    BindParam::Text(skill),
                ),
                None => (BindParam::Null, BindParam::Null),
            };
            writer
                .execute(
                    "INSERT INTO librarian (errand, asker, call, \
                     conversation, offer, skill) VALUES (?1, ?2, \
                     ?3, ?4, ?5, ?6)",
                    vec![
                        integer(errand.get())?,
                        number(asker),
                        BindParam::Text(call.0),
                        conversation.map_or(BindParam::Null, |c| {
                            BindParam::Text(c.0)
                        }),
                        offer,
                        skill,
                    ],
                )
                .await?;
            Ok(Affected::default())
        }
        Change::Ended { errand, ended } => {
            end(writer, errand, &ended).await
        }
        Change::Judged { errand } => {
            let words = writer
                .rows()
                .value::<String>(
                    "SELECT words FROM librarian WHERE errand = ?1",
                    vec![integer(errand.get())?],
                )
                .await?
                .with_context(|| {
                    format!("errand {} judged nothing", errand.get())
                })?;
            notice(&writer.rows(), errand, words).await
        }
    }
}

/// Tells the asker how errand `errand` ended, or keeps its verdict
/// and begins the step that brings it to the library.
async fn end(
    writer: &Writer<'_>,
    errand: ErrandNumber,
    ended: &Ended,
) -> Result<Affected> {
    let skill = writer
        .rows()
        .value::<Option<String>>(
            "SELECT skill FROM librarian WHERE errand = ?1",
            vec![integer(errand.get())?],
        )
        .await?
        .with_context(|| {
            format!("errand {} answers no call", errand.get())
        })?;
    let Some(skill) = skill else {
        let text = match ended {
            Ended::Answered { text } => {
                format!("The librarian says: {text}")
            }
            Ended::Failed { why } => {
                format!("The librarian could not answer: {why}")
            }
        };
        return notice(&writer.rows(), errand, text).await;
    };
    let (accepted, words) = verdict(&skill, ended);
    writer
        .execute(
            "UPDATE librarian SET accepted = ?2, words = ?3 WHERE \
             errand = ?1",
            vec![
                integer(errand.get())?,
                BindParam::Integer(accepted.into()),
                BindParam::Text(words),
            ],
        )
        .await?;
    steps::apply(writer, steps::Change::Begin { row: row(errand) })
        .await
}

/// Whether the librarian took `skill`, and the words its offerer is
/// told, as today reads its answer: `ACCEPT` as the first word takes
/// it.
fn verdict(skill: &str, ended: &Ended) -> (bool, String) {
    let answer = match ended {
        Ended::Answered { text } => text,
        Ended::Failed { why } => {
            return (
                false,
                format!(
                    "The librarian could not judge `{skill}`: {why}"
                ),
            );
        }
    };
    let accepted =
        answer.split_whitespace().next().is_some_and(|first| {
            first
                .trim_matches(|c: char| !c.is_alphabetic())
                .eq_ignore_ascii_case("ACCEPT")
        });
    let words = if accepted {
        format!(
            "The librarian took `{skill}`: {answer} It goes on the \
             shelves for every agent; use the library's copy."
        )
    } else {
        format!(
            "The librarian did not take `{skill}`: {answer} It \
             stays in your lodge."
        )
    };
    (accepted, words)
}

/// The notice that gives errand `errand`'s asker `text`, when it has
/// not left.
async fn notice(
    rows: &Rows<'_>,
    errand: ErrandNumber,
    text: String,
) -> Result<Affected> {
    let (asker, conversation, task) = rows
        .row::<(u32, Option<String>, TaskId)>(
            "SELECT librarian.asker, librarian.conversation, \
             errand.task FROM librarian JOIN errand ON errand.n = \
             librarian.errand WHERE librarian.errand = ?1",
            vec![integer(errand.get())?],
        )
        .await?
        .with_context(|| {
            format!("errand {} answers no call", errand.get())
        })?;
    let asker = AgentNumber(asker);
    if tree::left(rows, asker).await?.is_some() {
        return Ok(Affected::default());
    }
    Ok(Affected {
        deliveries: vec![Delivery::Line {
            to: asker,
            submit: Submit {
                id: InputId(format!("librarian/{}", errand.get())),
                task: Some(task.on_the_wire()),
                input: Input::Notice {
                    text,
                    conversation: conversation.map(ConversationId),
                },
            },
        }],
        ..Affected::default()
    })
}

/// The changes the end of errand `errand` makes.
pub(super) fn ended(
    errand: ErrandNumber,
    ended: &Ended,
) -> Vec<change::Change> {
    vec![
        Change::Ended {
            errand,
            ended: ended.clone(),
        }
        .into(),
    ]
}

/// The librarian's whole instructions: its part, the shelves as the
/// library holds them now, then `asked`.
pub(super) async fn instructions(
    cx: &Cx<'_>,
    asked: Option<String>,
) -> Result<String> {
    let asked = asked
        .context("a librarian's errand keeps what it is asked")?;
    let list = cx
        .peers()
        .call(&Library, &ReadShelves, When::AfterAnswer)
        .await?;
    let shelves = if list.skills.is_empty() {
        "(empty)".to_owned()
    } else {
        list.skills
            .iter()
            .map(|skill| {
                format!("- {}: {}", skill.name, skill.description)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(format!("{ROLE}\n\n# The shelves\n{shelves}\n\n{asked}"))
}

/// The row of the step that brings errand `errand`'s verdict to the
/// library, before its offerer is told.
fn row(errand: ErrandNumber) -> StepId {
    StepId {
        kind: Kind::Verdict,
        n: u64::from(errand.get()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Verdict,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Verdict];

    fn name(self) -> &'static str {
        match self {
            Self::Verdict => "verdict",
        }
    }
}

/// Brings the verdict of the errand row `n` to the library, under an
/// id made from its offerer and its call, and returns the changes
/// committed with its progress.
pub(super) async fn run(
    cx: &Cx<'_>,
    n: u64,
    Step::Verdict: Step,
) -> Result<Vec<change::Change>> {
    let errand = ErrandNumber::new(u32::try_from(n)?)
        .context("a verdict past the last errand")?;
    let (asker, call, offer, accepted, words) = cx
        .rows()?
        .row::<(u32, String, u32, bool, String)>(
            "SELECT asker, call, offer, accepted, words FROM \
             librarian WHERE errand = ?1",
            vec![integer(errand.get())?],
        )
        .await?
        .with_context(|| format!("errand {n} has no verdict"))?;
    let verdict = Verdict {
        id: profiles::request_id(AgentNumber(asker), &CallId(call))?,
        offer: OfferId(offer),
        accepted,
        words,
    };
    match cx
        .peers()
        .call(&Library, &verdict, When::AfterAnswer)
        .await?
    {
        Done::Done => Ok(vec![Change::Judged { errand }.into()]),
        Done::Refused { why } => Err(peer::refusal(why)),
    }
}

/// An agent's call of the librarian.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Librarian<T> {
    pub called: Called,
    pub act: T,
}

impl ToolAct<Agent> for AskLibrarian {
    type Run = Librarian<Self>;

    fn run(self, called: &Called) -> Librarian<Self> {
        Librarian {
            called: called.clone(),
            act: self,
        }
    }
}

impl ToolAct<Agent> for SubmitSkill {
    type Run = Librarian<Self>;

    fn run(self, called: &Called) -> Librarian<Self> {
        Librarian {
            called: called.clone(),
            act: self,
        }
    }
}

/// What a librarian's errand is started with, for its asker.
pub(super) struct Asker {
    n: ErrandNumber,
    task: TaskId,
    model: Option<String>,
    name: String,
}

async fn asker(
    rows: &Rows<'_>,
    by: AgentNumber,
    called: &Called,
) -> Result<Asker> {
    let named = match &called.task {
        Some(task) => tasks::named(rows, by, task).await?,
        None => None,
    };
    let task = match named {
        Some(task) => task,
        None => tasks::standing(rows, by).await?,
    };
    Ok(Asker {
        n: errands::next(rows).await?,
        task,
        model: documents::model(rows, by)
            .await?
            .map(|chosen| chosen.model),
        name: record::name_of(rows, by).await?,
    })
}

impl<T: Send + Sync + 'static> Librarian<T> {
    /// The changes that start the librarian's errand for `by`, asked
    /// to do `asked` with `text`, judging what was `offered`.
    fn start(
        &self,
        by: &Agent,
        asker: Asker,
        (asked, text): (&str, String),
        offered: Option<Offered>,
    ) -> Decision<(), Done<Standing>> {
        let input = ErrandInput {
            instructions: Some(asked.to_owned()),
            text,
            task: asker.task,
            model: asker.model,
            cap: errands::UNCAPPED,
        };
        Decision::Change(
            Decided::new(())
                .change(errands::start(
                    Purpose::Librarian,
                    input,
                    asker.n,
                ))
                .change(Change::Begin {
                    errand: asker.n,
                    asker: by.0,
                    call: self.called.call.clone(),
                    conversation: self.called.conversation.clone(),
                    offered,
                }),
        )
    }
}

impl Act<Agent> for Librarian<AskLibrarian> {
    type Output = Done<Standing>;
    type Loaded = Asker;
    type Done = ();

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Asker> {
        asker(rows, by.0, &self.called).await
    }

    fn decide(
        &self,
        by: &Agent,
        asker: Asker,
    ) -> Decision<(), Self::Output> {
        let text =
            format!("{} asks: {}", asker.name, self.act.question);
        self.start(by, asker, (LOOKING, text), None)
    }

    fn answer(&self, (): (), _made: &calls::Made) -> Self::Output {
        Done::Done
    }
}

impl Act<Agent> for Librarian<SubmitSkill> {
    type Output = Done<Standing>;
    /// The asker, unless an errand judges the offer already.
    type Loaded = Option<Asker>;
    type Done = ();

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Option<Asker>> {
        let judging = rows
            .value::<i64>(
                "SELECT 1 FROM librarian WHERE offer = ?1",
                vec![BindParam::Integer(self.act.offer.0.into())],
            )
            .await?
            .is_some();
        if judging {
            return Ok(None);
        }
        Ok(Some(asker(rows, by.0, &self.called).await?))
    }

    fn decide(
        &self,
        by: &Agent,
        asker: Option<Asker>,
    ) -> Decision<(), Self::Output> {
        let Some(asker) = asker else {
            return Decision::Answer(Done::Done);
        };
        let SubmitSkill {
            offer,
            skill,
            reason,
        } = &self.act;
        let text = format!(
            "{} offers the skill `{skill}`: {}",
            asker.name,
            reason.as_deref().unwrap_or_default()
        );
        let offered = Offered {
            offer: *offer,
            skill: skill.clone(),
        };
        self.start(by, asker, (JUDGING, text), Some(offered))
    }

    fn answer(&self, (): (), _made: &calls::Made) -> Self::Output {
        Done::Done
    }
}
