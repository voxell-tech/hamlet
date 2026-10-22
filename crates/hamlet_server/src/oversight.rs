//! What waits for a chief (H11, H34): approvals of held tool calls,
//! faults, and questions, each kept until it is answered or lapses.

pub(crate) mod held;

use anyhow::{Context, Result, bail};
use hamlet_protocol::client::{
    self as protocol, Address, ChatRef, Choice, Lasting, Questions,
    WaitDanger, WaitDetail,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::change::clip;
use crate::harness::BindingId;
use crate::record::{AgentRef, Hamlet};
use crate::{OperationId, RuntimeInstance};

/// What a held tool call would do that a chief must allow.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Danger {
    Network,
    /// A society act the chief's limits reserve to the chief.
    Reserved,
    Edit,
    Command,
}

impl Danger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Reserved => "reserved",
            Self::Edit => "edit",
            Self::Command => "command",
        }
    }

    fn named(name: &str) -> Option<Self> {
        [Self::Network, Self::Reserved, Self::Edit, Self::Command]
            .into_iter()
            .find(|danger| danger.as_str() == name)
    }

    /// The danger's words after the agent's name.
    pub fn says(self) -> &'static str {
        match self {
            Self::Network => "wants the network for",
            Self::Reserved => "wants to do what you reserved:",
            Self::Edit => "wants to edit",
            Self::Command => "wants to run",
        }
    }

    /// What `always` lets go: an edit or a command for the agent's
    /// session, the network until it is revoked.
    fn lasting(self) -> Option<Lasting> {
        match self {
            Self::Edit | Self::Command => Some(Lasting::Session),
            Self::Network => Some(Lasting::Grant),
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
}

impl From<Danger> for WaitDanger {
    fn from(danger: Danger) -> Self {
        match danger {
            Danger::Network => Self::Network,
            Danger::Reserved => Self::Reserved,
            Danger::Edit => Self::Edit,
            Danger::Command => Self::Command,
        }
    }
}

/// What a wait waits for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum WaitKind {
    Approval(Danger),
    Fault,
    Question(Questions),
    Objection,
}

impl WaitKind {
    fn name(&self) -> &'static str {
        match self {
            Self::Approval(_) => "approval",
            Self::Fault => "fault",
            Self::Question(_) => "question",
            Self::Objection => "objection",
        }
    }

    fn danger(&self) -> Option<Danger> {
        match self {
            Self::Approval(danger) => Some(*danger),
            _ => None,
        }
    }

    fn questions(&self) -> Option<&Questions> {
        match self {
            Self::Question(questions) => Some(questions),
            _ => None,
        }
    }

    fn of(
        name: &str,
        danger: Option<Danger>,
        questions: Option<Questions>,
    ) -> Result<Self> {
        let asks = questions.is_some();
        Ok(match (name, danger, questions) {
            ("approval", Some(danger), None) => {
                Self::Approval(danger)
            }
            ("fault", _, None) => Self::Fault,
            ("question", _, Some(questions)) => {
                Self::Question(questions)
            }
            ("objection", _, None) => Self::Objection,
            _ => bail!(
                "a wait of kind {name} and danger {danger:?}, with \
                 questions: {asks}"
            ),
        })
    }
}

/// How a wait or the call it held was ended: by a chief's answer, or
/// by a stop or a rewind.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Ruling {
    Allow,
    Always,
    Deny,
    Go,
    Withdraw,
    Reply,
    Dismiss,
    Stopped,
    Rewound,
}

impl Ruling {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Always => "always",
            Self::Deny => "deny",
            Self::Go => "go",
            Self::Withdraw => "withdraw",
            Self::Reply => "reply",
            Self::Dismiss => "dismiss",
            Self::Stopped => "stopped",
            Self::Rewound => "rewound",
        }
    }

    fn named(name: &str) -> Option<Self> {
        [
            Self::Allow,
            Self::Always,
            Self::Deny,
            Self::Go,
            Self::Withdraw,
            Self::Reply,
            Self::Dismiss,
            Self::Stopped,
            Self::Rewound,
        ]
        .into_iter()
        .find(|ruling| ruling.as_str() == name)
    }

    /// Whether it lets a held call run.
    pub fn allows(self) -> bool {
        matches!(self, Self::Allow | Self::Always)
    }
}

impl From<&Choice> for Ruling {
    fn from(choice: &Choice) -> Self {
        match choice {
            Choice::Allow => Self::Allow,
            Choice::Always => Self::Always,
            Choice::Deny => Self::Deny,
            Choice::Go => Self::Go,
            Choice::Withdraw => Self::Withdraw,
            Choice::Reply => Self::Reply,
            Choice::Dismiss => Self::Dismiss,
        }
    }
}

/// A chief's chat a wait was raised in, with its session number.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChatPlace {
    pub hamlet: String,
    pub number: u64,
    pub chief: String,
    pub session: i64,
}

impl ChatPlace {
    /// Whether it is the chat of `chief` with `agent`.
    fn is_with(&self, agent: &AgentRef, chief: &str) -> bool {
        self.hamlet == agent.hamlet
            && self.number == agent.number
            && self.chief == chief
    }
}

/// Something that waits for a chief.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Wait {
    pub id: String,
    pub kind: WaitKind,
    /// The name of the agent that raised it.
    pub by: String,
    pub agent: Option<AgentRef>,
    /// The exact text the chief reads.
    pub about: String,
    pub change: Option<Value>,
    pub answers: Vec<Choice>,
    pub since: i64,
    /// Times it was asked while it waited.
    pub asked: i64,
    pub chat: Option<ChatPlace>,
}

/// A wait to raise, and the tool call it holds.
pub(crate) struct Raising {
    pub kind: WaitKind,
    pub by: String,
    pub agent: Option<AgentRef>,
    pub about: String,
    pub change: Option<Value>,
    pub answers: Vec<Choice>,
    pub chat: Option<ChatPlace>,
    pub call: Option<Holding>,
}

/// The tool call a raised wait holds.
pub(crate) struct Holding {
    pub id: OperationId,
    pub binding: BindingId,
    pub input: Option<String>,
    pub change: Option<Value>,
}

/// A held call a settled wait ruled on, to be told at once.
pub(crate) type Woken = Vec<(RuntimeInstance, String)>;

/// Raises a fault: a turn that failed, which a chief can only
/// dismiss. The same fault again is asked again.
pub(crate) fn raise_fault(
    db: &Connection,
    by: String,
    agent: AgentRef,
    chat: Option<ChatPlace>,
    about: String,
    at: i64,
) -> Result<String> {
    held::put(
        db,
        Raising {
            kind: WaitKind::Fault,
            by,
            agent: Some(agent),
            about,
            change: None,
            answers: vec![Choice::Dismiss],
            chat,
            call: None,
        },
        at,
    )
}

/// Ends the open wait `id` and rules on each call it held that has
/// not ended. Gives the wait and the calls to tell, or `None` when
/// nothing waits as `id`.
pub(crate) fn settle(
    db: &Connection,
    id: &str,
    ruling: Ruling,
    text: &str,
    chief: Option<&str>,
    at: i64,
) -> Result<Option<(Wait, Woken)>> {
    let Some(wait) = held::find(db, id)? else {
        return Ok(None);
    };
    let woken = held::rule(db, id, ruling, text)?;
    held::close(db, id, ruling, text, chief, at)?;
    Ok(Some((wait, woken)))
}

/// Ends the waits a rewind of the chat of `chief` with `agent` takes
/// back: those raised in its session `session` since `since`.
pub(crate) fn rewind(
    db: &Connection,
    agent: &AgentRef,
    chief: &str,
    session: i64,
    since: i64,
    at: i64,
) -> Result<Woken> {
    end_all(db, Ruling::Rewound, chief, at, |wait| {
        wait.since >= since
            && wait.chat.as_ref().is_some_and(|chat| {
                chat.is_with(agent, chief) && chat.session == session
            })
    })
}

/// Ends what waits for a stop of `agent` by `chief`: every wait but a
/// fault that it raised or that was raised in a chat with it.
pub(crate) fn stop(
    db: &Connection,
    agent: &AgentRef,
    chief: &str,
    at: i64,
) -> Result<Woken> {
    end_all(db, Ruling::Stopped, chief, at, |wait| {
        wait.kind != WaitKind::Fault
            && (wait.agent.as_ref() == Some(agent)
                || wait
                    .chat
                    .as_ref()
                    .is_some_and(|chat| chat.is_with(agent, chief)))
    })
}

fn end_all(
    db: &Connection,
    ruling: Ruling,
    chief: &str,
    at: i64,
    ends: impl Fn(&Wait) -> bool,
) -> Result<Woken> {
    let mut woken = Vec::new();
    for wait in held::open(db)?.into_iter().filter(|wait| ends(wait))
    {
        if let Some((_, calls)) =
            settle(db, &wait.id, ruling, "", Some(chief), at)?
        {
            woken.extend(calls);
        }
    }
    Ok(woken)
}

/// The open waits in the order they were raised.
pub(crate) fn open(db: &Connection) -> Result<Vec<Wait>> {
    held::open(db)
}

/// The open wait `id`.
pub(crate) fn find(
    db: &Connection,
    id: &str,
) -> Result<Option<Wait>> {
    held::find(db, id)
}

/// How the wait `id` ended, `lapsed` when that was not kept.
pub(crate) fn ended(db: &Connection, id: &str) -> Result<String> {
    Ok(held::ended(db, id)?.map_or("lapsed", Ruling::as_str).into())
}

/// The waits raised by `agent`, across its chats.
pub(crate) fn for_agent<'a>(
    waits: &'a [Wait],
    agent: &AgentRef,
) -> impl Iterator<Item = &'a Wait> {
    waits
        .iter()
        .filter(move |wait| wait.agent.as_ref() == Some(agent))
}

/// The waits of the chat of `chief` with `agent`, across its
/// sessions.
pub(crate) fn for_chat<'a>(
    waits: &'a [Wait],
    agent: &'a AgentRef,
    chief: &'a str,
) -> impl Iterator<Item = &'a Wait> {
    waits.iter().filter(move |wait| {
        wait.chat
            .as_ref()
            .is_some_and(|chat| chat.is_with(agent, chief))
    })
}

/// The line of a wait's text a one-line view quotes: an edit's file,
/// or an objection's instruction under the name of its giver.
pub(crate) fn subject(wait: &Wait) -> &str {
    let mut lines = wait.about.lines();
    match wait.kind {
        WaitKind::Approval(Danger::Edit) => {
            lines.next().unwrap_or_default()
        }
        WaitKind::Objection => lines.nth(1).unwrap_or(&wait.about),
        _ => &wait.about,
    }
}

/// One line for a wait, quoting at most `most` characters of its
/// subject.
pub(crate) fn line(wait: &Wait, most: usize) -> String {
    let subject = subject(wait);
    match wait.kind {
        WaitKind::Approval(danger) => format!(
            "{} {} `{}`",
            wait.by,
            danger.says(),
            clip(subject, most, true, true)
        ),
        WaitKind::Objection => {
            clip(subject, most + wait.by.len(), true, true)
        }
        WaitKind::Question(_) => format!(
            "{} asks: {}",
            wait.by,
            clip(subject, most, true, true)
        ),
        WaitKind::Fault => format!(
            "{} failed: {}",
            wait.by,
            clip(subject, most, true, true)
        ),
    }
}

/// A wait in full, with its subject and the answers offered.
pub(crate) fn strip(wait: &Wait) -> String {
    format!(
        "[{}] {}\n    {}\n    answer with: {}",
        wait.kind.name(),
        line(wait, 60),
        wait.about.replace('\n', "\n    "),
        wait.answers
            .iter()
            .map(|choice| format!(
                "/{}",
                Ruling::from(choice).as_str()
            ))
            .collect::<Vec<_>>()
            .join(" ")
    )
}

/// A chief's view of `wait`; a wait raised in the current session of
/// its chat names no past session.
pub(crate) fn view(
    wait: &Wait,
    record: Option<&Hamlet>,
    current: Option<i64>,
) -> Result<protocol::Wait> {
    let chat = match (&wait.chat, record) {
        (Some(chat), Some(record)) => Some(ChatRef {
            with: Address {
                hamlet: chat.hamlet.clone(),
                agent: record.address_of(chat.number).into(),
            },
            past: (current != Some(chat.session))
                .then_some(chat.session),
            from: None,
            kind: None,
        }),
        _ => None,
    };
    let detail = match &wait.kind {
        WaitKind::Approval(danger) => WaitDetail::Approval {
            always: danger
                .lasting()
                .filter(|_| wait.answers.contains(&Choice::Always)),
            danger: WaitDanger::from(*danger),
            says: danger.says().into(),
        },
        WaitKind::Fault => WaitDetail::Fault {},
        WaitKind::Question(questions) => WaitDetail::Question {
            questions: questions.clone(),
        },
        WaitKind::Objection => WaitDetail::Objection {},
    };
    let change = wait
        .change
        .clone()
        .map(serde_json::from_value)
        .transpose()
        .with_context(|| format!("wait {}'s change", wait.id))?;
    Ok(protocol::Wait {
        id: wait.id.clone(),
        by: wait.by.clone(),
        agent: wait.agent.clone().map(Into::into),
        about: wait.about.clone(),
        subject: subject(wait).into(),
        change,
        answers: wait.answers.clone(),
        since: u64::try_from(wait.since)
            .with_context(|| format!("wait {}'s start", wait.id))?,
        asked: u64::try_from(wait.asked)
            .with_context(|| format!("wait {}'s asking", wait.id))?,
        chat,
        detail,
    })
}
