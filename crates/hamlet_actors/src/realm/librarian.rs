//! The librarian, an errand that answers an agent from the shelves:
//! the `librarian` table, the call each such errand answers.

use anyhow::{Context as _, Result};
use hamlet_actor::BindParam;
use hamlet_messages::address::{Agent, Library};
use hamlet_messages::agent::{Input, InputId, Submit};
use hamlet_messages::library::ReadShelves;
use hamlet_messages::realm::profiles::AskLibrarian;
use hamlet_messages::realm::workers::Ended;
use hamlet_messages::realm::{Done, Standing};
use hamlet_messages::workspace::{
    AgentNumber, CallId, ConversationId, ErrandNumber,
};
use serde::{Deserialize, Serialize};

use super::act::{Act, Called, Decided, Decision, ToolAct};
use super::calls::{self, Delivery};
use super::change::{self, Affected, Rows, Writer, integer, number};
use super::peer::When;
use super::steps::Cx;
use super::tasks::{self, TaskId};
use super::workers::errands::{self, ErrandInput, Purpose};
use super::{documents, record, tree};

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

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// Errand `errand` answers `asker`'s call `call`, made in
    /// `conversation`.
    Begin {
        errand: ErrandNumber,
        asker: AgentNumber,
        call: CallId,
        conversation: Option<ConversationId>,
    },
    /// Errand `errand`, paid by `task`, ended as `ended`: its asker
    /// is told, when it has not left.
    Ended {
        errand: ErrandNumber,
        task: TaskId,
        ended: Ended,
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
        } => {
            writer
                .execute(
                    "INSERT INTO librarian (errand, asker, call, \
                     conversation) VALUES (?1, ?2, ?3, ?4)",
                    vec![
                        integer(errand.get())?,
                        number(asker),
                        BindParam::Text(call.0),
                        conversation.map_or(BindParam::Null, |c| {
                            BindParam::Text(c.0)
                        }),
                    ],
                )
                .await?;
            Ok(Affected::default())
        }
        Change::Ended {
            errand,
            task,
            ended,
        } => told(&writer.rows(), errand, task, &ended).await,
    }
}

/// The notice that tells the asker of errand `errand` how it ended.
async fn told(
    rows: &Rows<'_>,
    errand: ErrandNumber,
    task: TaskId,
    ended: &Ended,
) -> Result<Affected> {
    let (asker, conversation) = rows
        .row::<(u32, Option<String>)>(
            "SELECT asker, conversation FROM librarian WHERE errand \
             = ?1",
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
    let text = match ended {
        Ended::Answered { text } => {
            format!("The librarian says: {text}")
        }
        Ended::Failed { why } => {
            format!("The librarian could not answer: {why}")
        }
    };
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

/// The changes the end of errand `errand`, paid by `task`, makes.
pub(super) fn ended(
    errand: ErrandNumber,
    task: TaskId,
    ended: &Ended,
) -> Vec<change::Change> {
    vec![
        Change::Ended {
            errand,
            task,
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
    /// to do `asked` with `text`.
    fn start(
        &self,
        by: &Agent,
        asker: Asker,
        asked: &str,
        text: String,
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
        self.start(by, asker, LOOKING, text)
    }

    fn answer(&self, (): (), _made: &calls::Made) -> Self::Output {
        Done::Done
    }
}
