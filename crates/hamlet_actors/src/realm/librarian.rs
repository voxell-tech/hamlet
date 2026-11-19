//! The librarian, an errand that answers an agent from the shelves
//! or judges a skill it offered: the `librarian` table, the call each
//! such errand answers, and the step that brings a verdict to the
//! library.

use std::fmt::Display;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use hamlet_actor::{Action, BindParam, Ctx, Handler};
use hamlet_messages::Message;
use hamlet_messages::address::{Agent, Library};
use hamlet_messages::agent::{Input, InputId, Submit};
use hamlet_messages::library::{
    OfferId, OfferRead, ReadOffer, ReadShelves, Verdict,
    VerdictRefusal,
};
use hamlet_messages::realm::profiles::{AskLibrarian, SubmitSkill};
use hamlet_messages::realm::workers::Ended;
use hamlet_messages::realm::{Done, Standing, Tool, ToolCall as _};
use hamlet_messages::sender::sender;
use hamlet_messages::workspace::{
    AgentNumber, CallId, ConversationId, ErrandNumber, RealmTask,
};
use serde::{Deserialize, Serialize};

use super::act::{self, Act, Called, Decided, Decision, ToolAct};
use super::calls::{self, Calls, Delivery};
use super::change::{self, Affected, Rows, Writer, integer, number};
use super::peer::When;
use super::steps::{self, Cx, Kind, StepId, Steps};
use super::tasks::{self, TaskId};
use super::workers::errands::{self, ErrandInput, Purpose};
use super::{Realm, callers, documents, profiles, record, tree};

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

/// The most characters of an offer's `SKILL.md` its judge is shown,
/// as today.
const SHOWN: usize = 6000;

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
    /// The library took the verdict of errand `errand`, or refused
    /// it for `refused`: its offerer is told.
    Judged {
        errand: ErrandNumber,
        refused: Option<VerdictRefusal>,
    },
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
        Change::Judged { errand, refused } => {
            let (skill, words) = writer
                .rows()
                .row::<(String, String)>(
                    "SELECT skill, words FROM librarian WHERE \
                     errand = ?1",
                    vec![integer(errand.get())?],
                )
                .await?
                .with_context(|| {
                    format!("errand {} judged nothing", errand.get())
                })?;
            let text = match refused {
                None => words,
                Some(why) => unjudged(&skill, &why),
            };
            notice(&writer.rows(), errand, text).await
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
            return (false, unjudged(skill, why));
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

/// The words that tell an offerer its skill was not judged.
fn unjudged(skill: &str, why: &dyn Display) -> String {
    format!("The librarian could not judge `{skill}`: {why}")
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
        deliveries: vec![told(
            asker,
            InputId(format!("librarian/{}", errand.get())),
            Some(task.on_the_wire()),
            conversation.map(ConversationId),
            text,
        )],
        ..Affected::default()
    })
}

fn told(
    to: AgentNumber,
    id: InputId,
    task: Option<RealmTask>,
    conversation: Option<ConversationId>,
    text: String,
) -> Delivery {
    Delivery::Line {
        to,
        submit: Submit {
            id,
            task,
            input: Input::Notice { text, conversation },
        },
    }
}

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
    let refused = match cx
        .peers()
        .call(&Library, &verdict, When::AfterAnswer)
        .await?
    {
        Done::Done => None,
        Done::Refused { why } => Some(why),
    };
    Ok(vec![Change::Judged { errand, refused }.into()])
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
    /// to do `asked` with `text`, judging what was `offered`; the act
    /// made `done`.
    fn start<D>(
        &self,
        done: D,
        by: &Agent,
        asker: Asker,
        (asked, text): (&str, String),
        offered: Option<Offered>,
    ) -> Decision<D, Done<Standing>> {
        let input = ErrandInput {
            instructions: Some(asked.to_owned()),
            text,
            task: asker.task,
            model: asker.model,
            cap: errands::UNCAPPED,
        };
        Decision::Change(
            Decided::new(done)
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
        self.start((), by, asker, (LOOKING, text), None)
    }

    fn answer(&self, (): (), _made: &calls::Made) -> Self::Output {
        Done::Done
    }
}

/// An agent's `submit_skill`, with what the library holds of the
/// offer it names.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Submitted {
    asked: SubmitSkill,
    read: OfferRead,
}

/// An agent's `submit_skill`, run once the library has read its
/// offer.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Submitting(pub Tool<SubmitSkill>);

impl Action for Submitting {
    type Output = <Tool<SubmitSkill> as Action>::Output;
    const NAME: &'static str = <Tool<SubmitSkill> as Action>::NAME;
}

impl Message for Submitting {
    type To = <Tool<SubmitSkill> as Message>::To;
    type From = <Tool<SubmitSkill> as Message>::From;
}

impl Handler<Submitting> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Submitting(tool): Submitting,
    ) -> Result<Done<Standing>> {
        let name = Submitting::NAME;
        let by = sender(&ctx, &tool)?;
        let read = self
            .peers
            .call(
                &Library,
                &ReadOffer {
                    offer: tool.act.offer,
                },
                When::BeforeAnswer,
            )
            .await?;
        let (key, called, asked) = act::called(&by, name, tool)?;
        let act = Librarian {
            called: called.clone(),
            act: Submitted { asked, read },
        };
        let ran = act::run_called(
            &self,
            &ctx,
            by,
            (key, called),
            name,
            SubmitSkill::TOOL,
            act,
        )
        .await?;
        callers::unreserved(name, ran)
    }
}

/// The outcome of a submitted offer.
pub(super) enum Judging {
    /// An errand judges it already.
    Already,
    /// It is not judged: its caller is told these words.
    Unjudged(String),
    /// An errand of `asker` is to judge `offered`, asked `text`.
    Judge {
        asker: Asker,
        offered: Offered,
        text: String,
    },
}

impl Act<Agent> for Librarian<Submitted> {
    type Output = Done<Standing>;
    type Loaded = Judging;
    /// The words its caller is told at once, when the offer is not
    /// judged.
    type Done = Option<String>;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Judging> {
        let Submitted { asked, read } = &self.act;
        let (skill, reason, paths, skill_md) = match read {
            OfferRead::Offered {
                agent,
                skill,
                reason,
                paths,
                skill_md,
            } if *agent == by.0 => (skill, reason, paths, skill_md),
            OfferRead::Offered { .. } => {
                let why = format!(
                    "the offer {} is not yours",
                    asked.offer.0
                );
                return Ok(Judging::Unjudged(unjudged(
                    &asked.skill,
                    &why,
                )));
            }
            OfferRead::Refused { why } => {
                return Ok(Judging::Unjudged(unjudged(
                    &asked.skill,
                    why,
                )));
            }
        };
        let judging = rows
            .value::<i64>(
                "SELECT 1 FROM librarian WHERE offer = ?1",
                vec![BindParam::Integer(asked.offer.0.into())],
            )
            .await?
            .is_some();
        if judging {
            return Ok(Judging::Already);
        }
        let asker = asker(rows, by.0, &self.called).await?;
        let shown = skill_md.chars().take(SHOWN).collect::<String>();
        let text = format!(
            "{} offers the skill `{skill}`: {reason}\n\nIts files: \
             {}\n\nIts SKILL.md:\n{shown}",
            asker.name,
            paths.join(", ")
        );
        Ok(Judging::Judge {
            asker,
            offered: Offered {
                offer: asked.offer,
                skill: skill.clone(),
            },
            text,
        })
    }

    fn decide(
        &self,
        by: &Agent,
        judging: Judging,
    ) -> Decision<Option<String>, Self::Output> {
        match judging {
            Judging::Already => Decision::Answer(Done::Done),
            Judging::Unjudged(words) => {
                Decision::Change(Decided::new(Some(words)))
            }
            Judging::Judge {
                asker,
                offered,
                text,
            } => self.start(
                None,
                by,
                asker,
                (JUDGING, text),
                Some(offered),
            ),
        }
    }

    fn deliveries(
        &self,
        by: &Agent,
        words: &Option<String>,
    ) -> Calls {
        let Some(text) = words.clone() else {
            return Calls::none();
        };
        Calls::after_answer(told(
            by.0,
            InputId(format!("submit_skill/{}", self.called.call.0)),
            self.called.task.clone(),
            self.called.conversation.clone(),
            text,
        ))
    }

    fn answer(
        &self,
        _: Option<String>,
        _made: &calls::Made,
    ) -> Self::Output {
        Done::Done
    }
}
