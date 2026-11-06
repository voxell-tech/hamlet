//! What waits for a chief (H11, H10, H34): approvals, questions,
//! objections, faults and newcomers, each a row until a chief answers
//! it or its asker goes.

pub(crate) mod gate;

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_protocol::client::{
    self as protocol, Choice, Lasting, Questions, WaitDanger,
    WaitDetail,
};
use hamlet_store::Db;
use hamlet_store::ids::{
    AgentId, ChiefId, ConversationId, ScholarId, TaskId, WaitId,
};
use hamlet_store::schema::wait;

use crate::change::Change;
use crate::clients::state::{wire_agent, wire_scholar};
use crate::society::addresses;
use crate::talk::conversations::{self, Kind};
use crate::text::text_enum;

text_enum! {
    pub(crate) enum WaitKind {
        /// A call the gate asks a chief to let run.
        Approval = "approval",
        /// A turn that failed, which a chief can only dismiss.
        Fault = "fault",
        /// `ask_chief`'s questions.
        Question = "question",
        /// An objection to a chief's instruction.
        Objection = "objection",
        /// A scholar an agent made, waiting at the harbor to be let
        /// into the city.
        Newcomer = "newcomer",
    }
}

text_enum! {
    /// What an approval would let through.
    pub(crate) enum Danger {
        Network = "network",
        Unconfined = "unconfined",
        /// An act a chief keeps for itself (H22).
        Reserved = "reserved",
        /// An edit in the hamlet's folder in manual mode.
        Edit = "edit",
        /// A command in manual mode.
        Command = "command",
    }
}

impl Danger {
    /// The words after the asker's name.
    pub fn says(self) -> &'static str {
        match self {
            Self::Network => "wants the network for",
            Self::Unconfined => "wants to run outside the sandbox:",
            Self::Reserved => "wants to do what you reserved:",
            Self::Edit => "wants to edit",
            Self::Command => "wants to run",
        }
    }

    /// What `always` lets go: an edit or a command for the rest of
    /// the chat's session, the network until it is revoked.
    pub fn lasting(self) -> Option<Lasting> {
        match self {
            Self::Edit | Self::Command => Some(Lasting::Session),
            Self::Network | Self::Unconfined => Some(Lasting::Grant),
            Self::Reserved => None,
        }
    }

    /// The answers a chief may give.
    pub fn answers(self) -> Vec<Choice> {
        match self.lasting() {
            Some(_) => {
                vec![Choice::Allow, Choice::Always, Choice::Deny]
            }
            None => vec![Choice::Allow, Choice::Deny],
        }
    }

    fn wire(self) -> WaitDanger {
        match self {
            Self::Network => WaitDanger::Network,
            Self::Unconfined => WaitDanger::Unconfined,
            Self::Reserved => WaitDanger::Reserved,
            Self::Edit => WaitDanger::Edit,
            Self::Command => WaitDanger::Command,
        }
    }
}

text_enum! {
    /// How a wait ended: a chief's answer, or its asker gone.
    pub(crate) enum Ruling {
        Allow = "allow",
        Always = "always",
        Deny = "deny",
        Go = "go",
        Withdraw = "withdraw",
        Reply = "reply",
        Dismiss = "dismiss",
        /// A newcomer was let into the city.
        Welcome = "welcome",
        /// The turn that asked was stopped.
        Stopped = "stopped",
        /// The agent that asked ended.
        Ended = "ended",
        /// A rewind took the turn that asked back.
        Rewound = "rewound",
    }
}

impl Ruling {
    pub fn of(choice: &Choice) -> Self {
        match choice {
            Choice::Allow => Self::Allow,
            Choice::Always => Self::Always,
            Choice::Deny => Self::Deny,
            Choice::Go => Self::Go,
            Choice::Withdraw => Self::Withdraw,
            Choice::Reply => Self::Reply,
            Choice::Dismiss => Self::Dismiss,
            Choice::Welcome => Self::Welcome,
        }
    }

    /// Whether the call it rules on runs.
    pub fn is_allowing(self) -> bool {
        matches!(self, Self::Allow | Self::Always | Self::Go)
    }
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = wait)]
pub(crate) struct Wait {
    pub id: WaitId,
    pub kind: WaitKind,
    pub danger: Option<Danger>,
    pub agent: Option<AgentId>,
    /// The newcomer it asks about.
    pub scholar: Option<ScholarId>,
    /// The conversation it was asked in.
    pub conversation: Option<ConversationId>,
    pub task: Option<TaskId>,
    pub about: String,
    pub change: Option<String>,
    pub answers: String,
    pub questions: Option<String>,
    pub since: i64,
    pub asked: i64,
    pub open: bool,
    pub how: Option<Ruling>,
    pub text: Option<String>,
    pub chief: Option<ChiefId>,
    pub settled: Option<i64>,
}

impl Wait {
    pub fn answers(&self) -> Result<Vec<Choice>> {
        Ok(serde_json::from_str(&self.answers)?)
    }

    pub fn questions(&self) -> Result<Option<Questions>> {
        self.questions
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(Into::into)
    }

    /// The line of its text a one-line view quotes: an edit's file,
    /// or an objection's instruction.
    pub fn subject(&self) -> &str {
        let mut lines = self.about.lines();
        match (self.kind, self.danger) {
            (WaitKind::Approval, Some(Danger::Edit)) => {
                lines.next().unwrap_or_default()
            }
            (WaitKind::Objection, _) => {
                lines.nth(1).unwrap_or(&self.about)
            }
            _ => &self.about,
        }
    }
}

/// A wait to raise.
#[derive(Clone, Debug)]
pub(crate) struct Raise {
    pub kind: WaitKind,
    pub danger: Option<Danger>,
    pub agent: AgentId,
    /// The newcomer a `newcomer` wait asks about.
    pub scholar: Option<ScholarId>,
    pub conversation: Option<ConversationId>,
    pub task: Option<TaskId>,
    pub about: String,
    pub change: Option<String>,
    pub answers: Vec<Choice>,
    pub questions: Option<Questions>,
}

/// Raises `raise`, or asks again the open wait that asks the same of
/// the same agent.
pub(crate) fn raise(
    cx: &mut Change<'_>,
    raise: Raise,
) -> Result<WaitId> {
    let same = wait::table
        .filter(wait::open.eq(true))
        .filter(wait::kind.eq(raise.kind))
        .filter(wait::agent.eq(raise.agent))
        .filter(wait::about.eq(&raise.about))
        .filter(wait::danger.is(raise.danger))
        .filter(wait::scholar.is(raise.scholar))
        .filter(wait::conversation.is(raise.conversation))
        .select(wait::id)
        .first::<WaitId>(cx.db)
        .optional()?;
    cx.touched.agents.insert(raise.agent);
    if let Some(id) = same {
        diesel::update(wait::table.find(id))
            .set(wait::asked.eq(wait::asked + 1))
            .execute(cx.db)?;
        return Ok(id);
    }
    let questions = raise
        .questions
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    Ok(diesel::insert_into(wait::table)
        .values((
            wait::kind.eq(raise.kind),
            wait::danger.eq(raise.danger),
            wait::agent.eq(raise.agent),
            wait::scholar.eq(raise.scholar),
            wait::conversation.eq(raise.conversation),
            wait::task.eq(raise.task),
            wait::about.eq(&raise.about),
            wait::change.eq(&raise.change),
            wait::answers.eq(serde_json::to_string(&raise.answers)?),
            wait::questions.eq(questions),
            wait::since.eq(cx.at),
        ))
        .returning(wait::id)
        .get_result(cx.db)?)
}

pub(crate) fn find(db: &mut Db, id: WaitId) -> Result<Option<Wait>> {
    Ok(wait::table
        .find(id)
        .select(Wait::as_select())
        .first(db)
        .optional()?)
}

/// Every wait still open, oldest first.
pub(crate) fn open(db: &mut Db) -> Result<Vec<Wait>> {
    Ok(wait::table
        .filter(wait::open.eq(true))
        .order(wait::id)
        .select(Wait::as_select())
        .load(db)?)
}

/// Ends the open wait `id` as `ruling`, with a chief's words. `false`
/// when it no longer waits.
pub(crate) fn settle(
    cx: &mut Change<'_>,
    id: WaitId,
    ruling: Ruling,
    text: &str,
    chief: Option<ChiefId>,
) -> Result<bool> {
    let ended = diesel::update(
        wait::table.find(id).filter(wait::open.eq(true)),
    )
    .set((
        wait::open.eq(false),
        wait::how.eq(ruling),
        wait::text.eq(text),
        wait::chief.eq(chief),
        wait::settled.eq(cx.at),
    ))
    .execute(cx.db)?;
    if let Some(agent) = wait::table
        .find(id)
        .select(wait::agent)
        .first::<Option<AgentId>>(cx.db)?
    {
        cx.touched.agents.insert(agent);
    }
    Ok(ended == 1)
}

/// Ends every open wait `agent` raised, as `ruling` (H34).
pub(crate) fn lapse(
    cx: &mut Change<'_>,
    agent: AgentId,
    ruling: Ruling,
) -> Result<()> {
    let ended = diesel::update(
        wait::table
            .filter(wait::agent.eq(agent))
            .filter(wait::open.eq(true)),
    )
    .set((
        wait::open.eq(false),
        wait::how.eq(ruling),
        wait::settled.eq(cx.at),
    ))
    .execute(cx.db)?;
    if ended > 0 {
        cx.touched.agents.insert(agent);
    }
    Ok(())
}

/// Ends every open wait asked in `conversation`, as `ruling`.
pub(crate) fn lapse_in(
    cx: &mut Change<'_>,
    conversation: ConversationId,
    ruling: Ruling,
) -> Result<()> {
    diesel::update(
        wait::table
            .filter(wait::conversation.eq(conversation))
            .filter(wait::open.eq(true)),
    )
    .set((
        wait::open.eq(false),
        wait::how.eq(ruling),
        wait::settled.eq(cx.at),
    ))
    .execute(cx.db)?;
    Ok(())
}

/// One line for `wait`, quoting at most `most` characters of its
/// subject.
pub(crate) fn line(wait: &Wait, by: &str, most: usize) -> String {
    let subject = clip(wait.subject(), most);
    match (wait.kind, wait.danger) {
        (WaitKind::Approval, Some(danger)) => {
            format!("{by} {} `{subject}`", danger.says())
        }
        (WaitKind::Approval, None) => {
            format!("{by} asks `{subject}`")
        }
        (WaitKind::Objection, _) => {
            format!("{by} objects: {subject}")
        }
        (WaitKind::Question, _) => format!("{by} asks: {subject}"),
        (WaitKind::Fault, _) => format!("{by} failed: {subject}"),
        (WaitKind::Newcomer, _) => {
            format!("{subject}, whom {by} made, waits at the harbor")
        }
    }
}

/// `text` on one line, cut to `most` characters.
fn clip(text: &str, most: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= most {
        return flat;
    }
    let mut cut = flat.chars().take(most).collect::<String>();
    cut.push('…');
    cut
}

/// The open waits as a client sees them.
pub(crate) fn views(db: &mut Db) -> Result<Vec<protocol::Wait>> {
    open(db)?.iter().map(|wait| view(db, wait)).collect()
}

fn view(db: &mut Db, wait: &Wait) -> Result<protocol::Wait> {
    let by = match wait.agent {
        Some(agent) => addresses::address_of(db, agent)?,
        None => "the realm".into(),
    };
    let chat = match wait.conversation {
        Some(id) => {
            let kept = conversations::get(db, id)?;
            match (kept.kind, kept.chief, kept.session) {
                (Kind::Chat, Some(chief), Some(session)) => {
                    let current = crate::talk::chats::current_number(
                        db, kept.agent, chief,
                    )?;
                    Some(protocol::ChatRef {
                        with: wire_agent(kept.agent),
                        past: (current != Some(session))
                            .then_some(session),
                        from: None,
                        kind: None,
                    })
                }
                _ => None,
            }
        }
        None => None,
    };
    let answers = wait.answers()?;
    let detail = match wait.kind {
        WaitKind::Approval => {
            let danger = wait.danger.with_context(|| {
                format!("wait {} has no danger", wait.id)
            })?;
            WaitDetail::Approval {
                always: danger
                    .lasting()
                    .filter(|_| answers.contains(&Choice::Always)),
                danger: danger.wire(),
                says: danger.says().into(),
            }
        }
        WaitKind::Fault => WaitDetail::Fault {},
        WaitKind::Question => WaitDetail::Question {
            questions: wait.questions()?.unwrap_or(Questions {
                message: String::new(),
                questions: Vec::new(),
            }),
        },
        WaitKind::Objection => WaitDetail::Objection {},
        WaitKind::Newcomer => WaitDetail::Newcomer {
            scholar: wire_scholar(wait.scholar.with_context(
                || format!("wait {} has no scholar", wait.id),
            )?),
        },
    };
    let change = wait
        .change
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .with_context(|| format!("wait {}'s change", wait.id))?;
    Ok(protocol::Wait {
        id: wait.id.to_string(),
        by,
        agent: wait.agent.map(wire_agent),
        about: wait.about.clone(),
        subject: wait.subject().into(),
        change,
        answers,
        since: u64::try_from(wait.since)?,
        asked: u64::try_from(wait.asked)?,
        chat,
        detail,
    })
}
