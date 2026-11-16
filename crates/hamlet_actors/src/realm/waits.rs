//! The waits for a chief: the `wait` table, `NeedsChief`,
//! `TurnFailed`, `Lapse` and the chief's `Answer` (H10, H11, H34).

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use hamlet_actor::{BindParam, Ctx};
use hamlet_messages::address::{Agent, Workspace};
use hamlet_messages::agent::{Input, InputId, Submit};
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::client::{Answer, AnswerRefusal};
use hamlet_messages::realm::models::{Cause, TurnFailed};
use hamlet_messages::realm::oversight::{
    Lapse, LapseReason, Permission,
};
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{Done, NeedsChief};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::reach::Need;
use hamlet_messages::workspace::{
    self, AgentNumber, CallId, ChiefAnswer, ConversationId,
};
use hamlet_protocol::client as protocol;
use serde::{Deserialize, Serialize};

use super::act::{Act, Decided, Decision};
use super::calls::{Calls, Delivery, Made};
use super::change::{self, Affected, Rows, Writer, number};
use super::oversight::{asking, reserved};
use super::peer::When;
use super::tasks::TaskId;
use super::world::Part;
use super::{Realm, oversight, parcels, tree};

fn named(wait: &str) -> Option<WaitId> {
    let n = wait.trim_start_matches('#').parse::<i64>().ok()?;
    u64::try_from(n).ok().map(WaitId)
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Tool,
    Question,
    Objection,
    Reserved,
    Send,
    Fault,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum Scope {
    Once,
    Always,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Allow(Scope),
    Refuse,
    Reply,
    Dismiss,
    Go,
    Withdraw,
    /// Refused with no chief asked, with the words its asker is
    /// told.
    Unasked(String),
    Lapsed(workspace::Lapse),
}

impl Choice {
    /// The `choice`, `scope` and `reason` columns.
    fn columns(&self) -> Result<[BindParam; 3]> {
        let (choice, scope, reason) = match self {
            Self::Allow(scope) => {
                ("allow", Some(change::case_name(scope)?), None)
            }
            Self::Refuse => ("refuse", None, None),
            Self::Reply => ("reply", None, None),
            Self::Dismiss => ("dismiss", None, None),
            Self::Go => ("go", None, None),
            Self::Withdraw => ("withdraw", None, None),
            Self::Unasked(words) => {
                ("unasked", None, Some(words.clone()))
            }
            Self::Lapsed(lapse) => {
                ("lapsed", None, Some(change::case_name(lapse)?))
            }
        };
        Ok([
            BindParam::Text(choice.to_owned()),
            nullable(scope),
            nullable(reason),
        ])
    }

    /// The choice its columns keep.
    fn read(
        choice: &str,
        scope: Option<String>,
        reason: Option<String>,
    ) -> Result<Self> {
        Ok(match (choice, scope, reason) {
            ("allow", Some(scope), _) => {
                Self::Allow(change::from_case_name(scope)?)
            }
            ("refuse", ..) => Self::Refuse,
            ("reply", ..) => Self::Reply,
            ("dismiss", ..) => Self::Dismiss,
            ("go", ..) => Self::Go,
            ("withdraw", ..) => Self::Withdraw,
            ("unasked", _, Some(words)) => Self::Unasked(words),
            ("lapsed", _, Some(lapse)) => {
                Self::Lapsed(change::from_case_name(lapse)?)
            }
            other => bail!("no choice is kept as {other:?}"),
        })
    }
}

fn nullable(text: Option<String>) -> BindParam {
    text.map_or(BindParam::Null, BindParam::Text)
}

/// A wait of a kind other than a tool's.
#[derive(Debug, PartialEq)]
pub(super) struct Raised {
    pub agent: AgentNumber,
    pub kind: Kind,
    pub task: Option<TaskId>,
    pub conversation: Option<ConversationId>,
    /// The words a chief reads.
    pub subject: String,
    /// The kind's own detail, as CBOR.
    pub detail: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// A tool wait opens, once per (agent, call).
    Open {
        asked: NeedsChief,
    },
    /// A tool wait past its agent's count is kept as `wait`, closed
    /// at once.
    Unasked {
        wait: WaitId,
        asked: NeedsChief,
        words: String,
    },
    Raise {
        wait: WaitId,
        raised: Raised,
    },
    /// `text` is the chief's.
    Close {
        wait: WaitId,
        choice: Choice,
        text: Option<String>,
    },
    /// Every open wait of an agent that ended closes, as lapsed.
    Lapse {
        agent: AgentNumber,
    },
}

/// A client sees each, as `Part::Waits`.
pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    let changed = match change {
        Change::Open { asked } => open(writer, None, asked).await?,
        Change::Unasked { wait, asked, words } => {
            open(writer, Some((wait, words)), asked).await?
        }
        Change::Raise { wait, raised } => {
            raise(writer, wait, raised).await?
        }
        Change::Close { wait, choice, text } => {
            close(writer, wait, &choice, text).await?
        }
        Change::Lapse { agent } => lapse(writer, agent).await?,
    };
    Ok(if changed == 0 {
        Affected::default()
    } else {
        Affected::seen([Part::Waits])
    })
}

/// A tool wait: open, or kept as `unasked`'s wait, closed with its
/// words.
async fn open(
    writer: &Writer<'_>,
    unasked: Option<(WaitId, String)>,
    asked: NeedsChief,
) -> Result<u64> {
    let at = BindParam::Integer(writer.at().0);
    let (wait, [choice, scope, reason], closed_at) = match unasked {
        Some((wait, words)) => (
            change::integer(wait.0)?,
            Choice::Unasked(words).columns()?,
            at.clone(),
        ),
        None => (
            BindParam::Null,
            [BindParam::Null, BindParam::Null, BindParam::Null],
            BindParam::Null,
        ),
    };
    writer
        .execute(
            "INSERT INTO wait (id, agent, kind, task, conversation, \
             call, subject, needs, reach_version, choice, scope, \
             reason, closed_at, at) VALUES (?1, ?2, 'tool', ?3, ?4, \
             ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) ON CONFLICT \
             (agent, call) DO NOTHING",
            vec![
                wait,
                number(asked.agent),
                task_column(
                    asked.realm_task.as_ref().and_then(TaskId::of),
                )?,
                BindParam::Text(asked.conversation.0),
                BindParam::Text(asked.call.0),
                BindParam::Text(asked.subject),
                BindParam::Text(serde_json::to_string(&asked.needs)?),
                change::integer(asked.reach_version.0)?,
                choice,
                scope,
                reason,
                closed_at,
                at,
            ],
        )
        .await
}

async fn raise(
    writer: &Writer<'_>,
    wait: WaitId,
    raised: Raised,
) -> Result<u64> {
    writer
        .execute(
            "INSERT INTO wait (id, agent, kind, task, conversation, \
             subject, detail, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, \
             ?7, ?8)",
            vec![
                change::integer(wait.0)?,
                number(raised.agent),
                BindParam::Text(change::case_name(&raised.kind)?),
                task_column(raised.task)?,
                nullable(raised.conversation.map(|c| c.0)),
                BindParam::Text(raised.subject),
                BindParam::Blob(raised.detail),
                BindParam::Integer(writer.at().0),
            ],
        )
        .await
}

fn task_column(task: Option<TaskId>) -> Result<BindParam> {
    task.map_or(Ok(BindParam::Null), TaskId::column)
}

async fn close(
    writer: &Writer<'_>,
    wait: WaitId,
    choice: &Choice,
    ruling: Option<String>,
) -> Result<u64> {
    let [choice, scope, reason] = choice.columns()?;
    writer
        .execute(
            "UPDATE wait SET choice = ?1, scope = ?2, reason = ?3, \
             ruling = ?4, closed_at = ?5 WHERE id = ?6 AND \
             closed_at IS NULL",
            vec![
                choice,
                scope,
                reason,
                nullable(ruling),
                BindParam::Integer(writer.at().0),
                change::integer(wait.0)?,
            ],
        )
        .await
}

async fn lapse(
    writer: &Writer<'_>,
    agent: AgentNumber,
) -> Result<u64> {
    writer
        .execute(
            "UPDATE wait SET choice = 'lapsed', reason = ?1, \
             closed_at = ?2 WHERE agent = ?3 AND closed_at IS NULL",
            vec![
                BindParam::Text(change::case_name(
                    &workspace::Lapse::Ended,
                )?),
                BindParam::Integer(writer.at().0),
                number(agent),
            ],
        )
        .await
}

/// The id the next wait opened in this transaction takes.
pub(super) async fn next(rows: &Rows<'_>) -> Result<WaitId> {
    Ok(WaitId(rows.next_id("wait").await?))
}

/// Whom a closed wait is told to.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct Told {
    /// The tool waits whose answer goes to the workspace.
    workspace: Vec<WaitId>,
    /// The wait whose notice goes to its agent.
    notice: Option<WaitId>,
    /// The task whose held letters go on.
    held: Option<TaskId>,
}

impl Told {
    fn workspace(wait: WaitId) -> Self {
        Self {
            workspace: vec![wait],
            ..Self::default()
        }
    }

    fn calls(&self) -> Calls {
        let answers =
            self.workspace.iter().copied().map(Delivery::ChiefAnswer);
        let notice = self.notice.map(Delivery::Notice);
        let held = self.held.map(Delivery::Held);
        Calls {
            before_answer: Vec::new(),
            after_answer: answers.chain(notice).chain(held).collect(),
        }
    }
}

/// A closed wait's row.
struct Closed {
    agent: AgentNumber,
    kind: Kind,
    call: Option<String>,
    task: Option<TaskId>,
    conversation: Option<ConversationId>,
    choice: Choice,
    /// The chief's text.
    ruling: Option<String>,
}

async fn closed(ctx: &Ctx<Realm>, wait: WaitId) -> Result<Closed> {
    let rows = Rows::outside(ctx)?;
    let id = change::integer(wait.0)?;
    let no_row = || format!("wait {} has no row", wait.0);
    let (agent, kind, call, task, conversation) =
        rows.row::<(
            u32,
            String,
            Option<String>,
            Option<TaskId>,
            Option<String>,
        )>(
            "SELECT agent, kind, call, task, conversation FROM wait \
             WHERE id = ?1",
            vec![id.clone()],
        )
        .await?
        .with_context(no_row)?;
    let (choice, scope, reason, ruling) = rows
        .row::<(
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )>(
            "SELECT choice, scope, reason, ruling FROM wait WHERE \
             id = ?1",
            vec![id],
        )
        .await?
        .with_context(no_row)?;
    let choice =
        choice.with_context(|| format!("wait {} is open", wait.0))?;
    Ok(Closed {
        agent: AgentNumber(agent),
        kind: change::from_case_name(kind)?,
        call,
        task,
        conversation: conversation.map(ConversationId),
        choice: Choice::read(&choice, scope, reason)?,
        ruling,
    })
}

/// Calls the workspace with the answer to the tool wait `wait`, from
/// its row.
pub(super) async fn deliver(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    wait: WaitId,
) -> Result<()> {
    let closed = closed(ctx, wait).await?;
    let call = closed
        .call
        .with_context(|| format!("wait {} has no call", wait.0))?;
    let answer = match closed.choice {
        Choice::Allow(_) => workspace::Answer::Allow,
        Choice::Refuse => workspace::Answer::Refuse {
            reason: closed.ruling,
        },
        Choice::Unasked(words) => workspace::Answer::Refuse {
            reason: Some(words),
        },
        Choice::Lapsed(lapse) => workspace::Answer::Lapsed { lapse },
        other => bail!("tool wait {} closed as {other:?}", wait.0),
    };
    realm
        .peers
        .call(
            &Workspace,
            &ChiefAnswer {
                agent: closed.agent,
                call: CallId(call),
                answer,
            },
            When::AfterAnswer,
        )
        .await?;
    Ok(())
}

/// Delivers the notice that closed `wait` to its agent, under the
/// wait's task.
pub(super) async fn notice(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    wait: WaitId,
) -> Result<()> {
    let Closed {
        agent,
        kind,
        task,
        conversation,
        choice,
        ruling,
        ..
    } = closed(ctx, wait).await?;
    let says = ruling.as_ref().map_or_else(String::new, |ruling| {
        format!(" The chief says: {ruling}")
    });
    let text = match (kind, choice) {
        (Kind::Question, Choice::Reply) => format!(
            "The chief answers: {}",
            ruling.unwrap_or_default()
        ),
        (Kind::Question, Choice::Dismiss) => {
            "The chief did not answer; go on with what you know."
                .to_owned()
        }
        (Kind::Objection, Choice::Go) => {
            format!(
                "The chief rules: go on with the instruction.{says}"
            )
        }
        (Kind::Objection, Choice::Withdraw) => format!(
            "The chief withdrew the instruction. Stop working on \
             it.{says}"
        ),
        (Kind::Reserved | Kind::Send, Choice::Refuse) => format!(
            "the chief refused it: {}",
            ruling.as_deref().unwrap_or("no reason given")
        ),
        (kind, choice) => bail!(
            "wait {} of kind {kind:?} closed as {choice:?} has no \
             notice",
            wait.0
        ),
    };
    realm
        .peers
        .call(
            &Agent(agent),
            &Submit {
                id: InputId(format!("wait/{}", wait.0)),
                task: task.map(TaskId::on_the_wire),
                input: Input::Notice { text, conversation },
            },
            When::AfterAnswer,
        )
        .await
}

/// The changes that close `payer`'s open budget faults when a chief
/// resets its budget.
pub(super) async fn budget_faults(
    rows: &Rows<'_>,
    payer: &Payer,
) -> Result<Vec<change::Change>> {
    let faults = rows
        .rows::<(u64, Vec<u8>)>(
            "SELECT wait.id, wait.detail FROM wait JOIN task ON \
             task.id = wait.task WHERE wait.kind = 'fault' AND \
             wait.closed_at IS NULL AND task.payer = ?1 ORDER BY \
             wait.id",
            vec![BindParam::Text(serde_json::to_string(payer)?)],
        )
        .await?;
    let mut closes = Vec::new();
    for (wait, detail) in faults {
        if ciborium::from_reader::<Cause, _>(detail.as_slice())?
            == Cause::Budget
        {
            closes.push(
                Change::Close {
                    wait: WaitId(wait),
                    choice: Choice::Dismiss,
                    text: None,
                }
                .into(),
            );
        }
    }
    Ok(closes)
}

/// The most refusals a chief gives `agent` on one task before the
/// realm refuses unasked.
async fn most_refusals(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<u32> {
    let hamlet = tree::lineage(rows, agent)
        .await?
        .and_then(|lineage| lineage.hamlet);
    Ok(hamlet
        .map(|hamlet| hamlet.limits)
        .unwrap_or_default()
        .asking
        .refusals)
}

/// A tool call of an agent's that a chief refused on a task.
struct Refused {
    subject: String,
    needs: Vec<Need>,
    ruling: Option<String>,
}

impl Refused {
    /// Whether this refusal covers a call for `needs` on `subject`:
    /// every call for the network, else the same call.
    fn covers(&self, needs: &[Need], subject: &str) -> bool {
        let network = |needs: &[Need]| needs.contains(&Need::Network);
        if network(needs) {
            network(&self.needs)
        } else {
            !network(&self.needs) && self.subject == subject
        }
    }
}

async fn refusals(
    rows: &Rows<'_>,
    agent: AgentNumber,
    task: TaskId,
) -> Result<Vec<Refused>> {
    rows.rows::<(String, String, Option<String>)>(
        "SELECT subject, needs, ruling FROM wait WHERE agent = ?1 \
         AND task = ?2 AND kind = 'tool' AND choice = 'refuse' \
         ORDER BY id",
        vec![number(agent), task.column()?],
    )
    .await?
    .into_iter()
    .map(|(subject, needs, ruling)| {
        Ok(Refused {
            subject,
            needs: serde_json::from_str(&needs)?,
            ruling,
        })
    })
    .collect()
}

/// The words a call is refused with when `refused` calls of its task
/// reach `most`.
fn past_the_count(refused: usize, most: u32) -> Option<String> {
    (refused >= usize::try_from(most).unwrap_or(usize::MAX)).then(
        || {
            format!(
                "the chief has refused {refused} requests this turn \
                 and will not be asked again in it. Stop and report \
                 what you could not do."
            )
        },
    )
}

pub(super) enum Asker {
    Ended,
    /// Its call already has a wait.
    Known,
    Asks,
    Refused {
        wait: WaitId,
        words: String,
    },
}

impl Act<Workspace> for NeedsChief {
    type Output = ();
    type Loaded = Asker;
    type Done = Told;

    async fn load(
        &self,
        _by: &Workspace,
        rows: &Rows<'_>,
    ) -> Result<Asker> {
        if tree::left(rows, self.agent).await?.is_some() {
            return Ok(Asker::Ended);
        }
        let known = rows
            .value::<bool>(
                "SELECT EXISTS (SELECT 1 FROM wait WHERE agent = ?1 \
                 AND call = ?2)",
                vec![
                    number(self.agent),
                    BindParam::Text(self.call.0.clone()),
                ],
            )
            .await?
            .unwrap_or(false);
        if known {
            return Ok(Asker::Known);
        }
        let Some(task) =
            self.realm_task.as_ref().and_then(TaskId::of)
        else {
            return Ok(Asker::Asks);
        };
        let refused = refusals(rows, self.agent, task).await?;
        let words = match refused.iter().find(|refused| {
            refused.covers(&self.needs, &self.subject)
        }) {
            Some(earlier) => Some(format!(
                "the chief already refused this in this turn ({}). \
                 Do not ask again: go on without it, or report that \
                 you could not.",
                earlier
                    .ruling
                    .as_deref()
                    .unwrap_or("no reason given")
            )),
            None => past_the_count(
                refused.len(),
                most_refusals(rows, self.agent).await?,
            ),
        };
        Ok(match words {
            Some(words) => Asker::Refused {
                wait: next(rows).await?,
                words,
            },
            None => Asker::Asks,
        })
    }

    fn decide(
        &self,
        _by: &Workspace,
        asker: Asker,
    ) -> Decision<Told, ()> {
        match asker {
            Asker::Ended | Asker::Known => Decision::Answer(()),
            Asker::Asks => Decision::Change(
                Decided::new(Told::default()).change(Change::Open {
                    asked: self.clone(),
                }),
            ),
            Asker::Refused { wait, words } => Decision::Change(
                Decided::new(Told::workspace(wait)).change(
                    Change::Unasked {
                        wait,
                        asked: self.clone(),
                        words,
                    },
                ),
            ),
        }
    }

    fn deliveries(&self, _by: &Workspace, told: &Told) -> Calls {
        told.calls()
    }

    fn answer(&self, _told: Told, _made: &Made) {}
}

/// What the unit of a wait's kind adds to a chief's answer: the
/// changes committed with the close, and the words the chief's picks
/// stand for.
#[derive(Default)]
pub(super) struct Ruling {
    pub(super) changes: Vec<change::Change>,
    pub(super) picked: Option<String>,
}

/// The open wait a chief answers.
pub(super) struct Waiting {
    wait: WaitId,
    agent: AgentNumber,
    kind: Kind,
    task: Option<TaskId>,
    conversation: Option<ConversationId>,
    needs: Vec<Need>,
    /// For a refusal that reaches its task's count: the agent's
    /// other open tool waits on the task, with the words they close
    /// with.
    past_the_count: Option<(Vec<WaitId>, String)>,
    ruling: Ruling,
}

impl Waiting {
    /// `choice` as this wait takes it, `None` where it is not
    /// offered.
    fn takes(
        &self,
        choice: &protocol::Choice,
        text: Option<&str>,
    ) -> Option<Choice> {
        use protocol::Choice as Chief;
        Some(match (self.kind, choice) {
            (
                Kind::Tool | Kind::Reserved | Kind::Send,
                Chief::Allow,
            ) => Choice::Allow(Scope::Once),
            (Kind::Tool, Chief::Always) if !self.needs.is_empty() => {
                Choice::Allow(Scope::Always)
            }
            (
                Kind::Tool | Kind::Reserved | Kind::Send,
                Chief::Deny,
            ) => Choice::Refuse,
            (Kind::Question, Chief::Reply) if text.is_some() => {
                Choice::Reply
            }
            (Kind::Question | Kind::Fault, Chief::Dismiss) => {
                Choice::Dismiss
            }
            (Kind::Objection, Chief::Go) => Choice::Go,
            (Kind::Objection, Chief::Withdraw) => Choice::Withdraw,
            _ => return None,
        })
    }

    /// What an always on this tool wait keeps: a grant of the
    /// network, else a trust for its conversation.
    fn lasting(&self) -> Option<oversight::Change> {
        if self.needs.contains(&Need::Network) {
            return Some(oversight::Change::Grant {
                agent: self.agent,
                permission: Permission::Network,
            });
        }
        Some(oversight::Change::Trust {
            agent: self.agent,
            conversation: self.conversation.clone()?,
            edits: self.needs.contains(&Need::Edit),
            commands: self.needs.contains(&Need::Command),
        })
    }

    fn told(&self, choice: &Choice) -> Told {
        let notice = Some(self.wait);
        match (self.kind, choice) {
            (Kind::Tool, _) => Told::workspace(self.wait),
            (Kind::Objection, Choice::Go) => Told {
                notice,
                held: self.task,
                ..Told::default()
            },
            (Kind::Question | Kind::Objection, _)
            | (Kind::Reserved | Kind::Send, Choice::Refuse) => Told {
                notice,
                ..Told::default()
            },
            _ => Told::default(),
        }
    }
}

impl Act<Client> for Answer {
    type Output = Done<AnswerRefusal>;
    type Loaded = Option<Waiting>;
    type Done = Told;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<Waiting>> {
        let Some(wait) = named(&self.0.wait) else {
            return Ok(None);
        };
        let Some((agent, kind, task, conversation, needs)) = rows
            .row::<(
                u32,
                String,
                Option<TaskId>,
                Option<String>,
                Option<String>,
            )>(
                "SELECT agent, kind, task, conversation, needs FROM \
                 wait WHERE id = ?1 AND closed_at IS NULL",
                vec![change::integer(wait.0)?],
            )
            .await?
        else {
            return Ok(None);
        };
        let agent = AgentNumber(agent);
        let kind = change::from_case_name::<Kind>(kind)?;
        let ruling = match kind {
            Kind::Question | Kind::Objection => {
                asking::ruling(rows, wait, self).await?
            }
            Kind::Reserved => {
                reserved::ruling(rows, wait, self).await?
            }
            Kind::Send => parcels::ruling(rows, wait, self).await?,
            Kind::Tool | Kind::Fault => Ruling::default(),
        };
        let past_the_count = match (kind, &self.0.choice, task) {
            (Kind::Tool, protocol::Choice::Deny, Some(task)) => {
                let refused =
                    refusals(rows, agent, task).await?.len();
                let most = most_refusals(rows, agent).await?;
                match past_the_count(refused + 1, most) {
                    Some(words) => Some((
                        rows.values::<u64>(
                            "SELECT id FROM wait WHERE agent = ?1 \
                             AND task = ?2 AND kind = 'tool' AND \
                             closed_at IS NULL AND id != ?3 ORDER \
                             BY id",
                            vec![
                                number(agent),
                                task.column()?,
                                change::integer(wait.0)?,
                            ],
                        )
                        .await?
                        .into_iter()
                        .map(WaitId)
                        .collect(),
                        words,
                    )),
                    None => None,
                }
            }
            _ => None,
        };
        Ok(Some(Waiting {
            wait,
            agent,
            kind,
            task,
            past_the_count,
            ruling,
            conversation: conversation.map(ConversationId),
            needs: needs
                .map(|needs| serde_json::from_str(&needs))
                .transpose()?
                .unwrap_or_default(),
        }))
    }

    fn decide(
        &self,
        _by: &Client,
        open: Option<Waiting>,
    ) -> Decision<Told, Self::Output> {
        let refuse = |why| Decision::Answer(Done::Refused { why });
        let Some(mut open) = open else {
            return refuse(AnswerRefusal::NothingWaits {
                wait: self.0.wait.clone(),
            });
        };
        let text = self
            .0
            .text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty());
        let ruling = std::mem::take(&mut open.ruling);
        let text = ruling.picked.as_deref().or(text);
        let choice = match (&self.0.picks, &ruling.picked) {
            (Some(_), None) => None,
            _ => open.takes(&self.0.choice, text),
        };
        let Some(choice) = choice else {
            return refuse(AnswerRefusal::NotOffered);
        };
        let mut decided = Decided::new(open.told(&choice));
        for change in ruling.changes {
            decided = decided.change(change);
        }
        if choice == Choice::Allow(Scope::Always)
            && let Some(lasting) = open.lasting()
        {
            decided = decided.change(lasting);
        }
        if let Some((others, words)) = open.past_the_count {
            for other in others {
                decided.done.workspace.push(other);
                decided = decided.change(Change::Close {
                    wait: other,
                    choice: Choice::Unasked(words.clone()),
                    text: None,
                });
            }
        }
        Decision::Change(decided.change(Change::Close {
            wait: open.wait,
            choice,
            text: text.map(str::to_owned),
        }))
    }

    fn deliveries(&self, _by: &Client, told: &Told) -> Calls {
        told.calls()
    }

    fn answer(&self, _told: Told, _made: &Made) -> Self::Output {
        Done::Done
    }
}

impl Act<Agent> for TurnFailed {
    type Output = ();
    /// The fault's wait with its cause as CBOR; `None` for an agent
    /// that ended.
    type Loaded = Option<(WaitId, Vec<u8>)>;
    type Done = ();

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Option<(WaitId, Vec<u8>)>> {
        if tree::left(rows, by.0).await?.is_some() {
            return Ok(None);
        }
        let mut cause = Vec::new();
        ciborium::into_writer(&self.cause, &mut cause)?;
        Ok(Some((next(rows).await?, cause)))
    }

    fn decide(
        &self,
        by: &Agent,
        fault: Option<(WaitId, Vec<u8>)>,
    ) -> Decision<(), ()> {
        let Some((wait, cause)) = fault else {
            return Decision::Answer(());
        };
        Decision::Change(Decided::new(()).change(Change::Raise {
            wait,
            raised: Raised {
                agent: by.0,
                kind: Kind::Fault,
                task: TaskId::of(&self.task),
                conversation: Some(self.conversation.clone()),
                subject: self.words.clone(),
                detail: cause,
            },
        }))
    }

    fn answer(&self, (): (), _made: &Made) {}
}

/// The lapse the workspace is told for `why`. A resume closes no
/// wait.
fn lapse_of(why: LapseReason) -> Option<workspace::Lapse> {
    match why {
        LapseReason::Stopped => Some(workspace::Lapse::Stopped),
        LapseReason::Rewound => Some(workspace::Lapse::Rewound),
        LapseReason::Ended => Some(workspace::Lapse::Ended),
        LapseReason::Resumed => None,
    }
}

impl Act<Agent> for Lapse {
    type Output = ();
    /// The agent's open waits that lapse, with their kinds.
    type Loaded = Vec<(WaitId, Kind)>;
    type Done = Told;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Vec<(WaitId, Kind)>> {
        if lapse_of(self.why).is_none() {
            return Ok(Vec::new());
        }
        let conversation =
            nullable(self.conversation.clone().map(|c| c.0));
        rows.rows::<(u64, String)>(
            "SELECT id, kind FROM wait WHERE agent = ?1 AND \
             closed_at IS NULL AND (?2 IS NULL OR conversation = \
             ?2) ORDER BY id",
            vec![number(by.0), conversation],
        )
        .await?
        .into_iter()
        .map(|(wait, kind)| {
            Ok((WaitId(wait), change::from_case_name(kind)?))
        })
        .collect()
    }

    fn decide(
        &self,
        by: &Agent,
        open: Vec<(WaitId, Kind)>,
    ) -> Decision<Told, ()> {
        let Some(lapse) = lapse_of(self.why) else {
            let Some(conversation) = self.conversation.clone() else {
                return Decision::Answer(());
            };
            return Decision::Change(
                Decided::new(Told::default()).change(
                    oversight::Change::DropTrust {
                        agent: by.0,
                        conversation,
                    },
                ),
            );
        };
        if open.is_empty() {
            return Decision::Answer(());
        }
        let mut decided = Decided::new(Told {
            workspace: open
                .iter()
                .filter(|(_, kind)| *kind == Kind::Tool)
                .map(|(wait, _)| *wait)
                .collect(),
            ..Told::default()
        });
        for (wait, _) in open {
            decided = decided.change(Change::Close {
                wait,
                choice: Choice::Lapsed(lapse),
                text: None,
            });
        }
        Decision::Change(decided)
    }

    fn deliveries(&self, _by: &Agent, told: &Told) -> Calls {
        told.calls()
    }

    fn answer(&self, _told: Told, _made: &Made) {}
}
