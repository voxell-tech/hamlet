//! An agent's chat as one line of time: who speaks in it, what was
//! said in the order it was said, the steps of a turn folded into one
//! row, and only the end of a long chat built.

use std::time::{SystemTime, UNIX_EPOCH};

use agents_ui::{AgentsWorldExt, Focused, hour, tag_of, titled};
use hamlet_engine_sdk::World;
use hamlet_engine_sdk::api::agents::{
    Entry, Focus, Part, Recipient, Roster, SessionMessage,
    SessionName,
};

use crate::reply::answers_of;
use crate::talk::{Kept, Talk};

/// The address the mayor of a hamlet is written to under.
const MAYOR: &str = "mayor";

/// The rows built from the end, and the rows each step back adds.
const PAGE: usize = 24;

/// The most notices of the chat's own kept.
const NOTICES: usize = 16;

/// The folds that are open, by their session and first message.
#[derive(Default)]
pub struct Unfolded(pub Vec<(SessionName, u64)>);

/// The rows of the chat that are built.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Reach(pub usize);

impl Default for Reach {
    fn default() -> Self {
        Reach(PAGE)
    }
}

impl Reach {
    /// More rows, as the chat is read back.
    pub fn earlier(&mut self) {
        self.0 += PAGE;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    /// The chief at this window.
    Me,
    Chief,
    Mayor,
    #[default]
    Agent,
}

/// Someone who speaks in a chat.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Who {
    pub name: String,
    /// Its office, or the start of its purpose.
    pub tag: String,
    pub kind: Kind,
}

/// A row of the chat: one message, the steps of a turn folded into
/// one, or a notice of the chat's own.
#[derive(Clone, PartialEq, Debug)]
pub enum Row {
    Message {
        session: SessionName,
        message: SessionMessage,
        rev: u64,
    },
    Fold(Fold),
    Notice(String),
}

/// The steps of a turn as one row that opens.
#[derive(Clone, PartialEq, Debug)]
pub struct Fold {
    pub session: SessionName,
    pub first: u64,
    /// The steps shown when it is open, with their counts of
    /// changes.
    pub steps: Vec<(SessionMessage, u64)>,
    /// The edits among them, shown when it is shut too.
    pub edits: Vec<(SessionMessage, u64)>,
    pub tools: usize,
    pub failed: usize,
    pub running: bool,
    /// The agents that were asked and worked meanwhile.
    pub helpers: Vec<String>,
    pub open: bool,
}

impl Fold {
    /// The words on its button.
    pub fn heading(&self) -> String {
        let plural = if self.tools == 1 { "step" } else { "steps" };
        let mut text = match (self.running, self.tools) {
            (true, 0) => "Working".to_string(),
            (true, n) => format!("Working, {n} {plural} so far"),
            (false, 0) if !self.helpers.is_empty() => {
                "Asked for help".to_string()
            }
            (false, 0) => "Thought it over".to_string(),
            (false, n) => format!("Worked, {n} {plural}"),
        };
        if self.failed > 0 {
            text.push_str(&format!(", {} failed", self.failed));
        }
        if !self.helpers.is_empty() {
            text.push_str(&format!(
                ", with {}",
                self.helpers.join(", ")
            ));
        }
        // Nothing to open in a fold of another agent's work alone.
        let mark = match (self.steps.is_empty(), self.open) {
            (true, _) => " ",
            (false, true) => "-",
            (false, false) => "+",
        };
        format!("{mark}  {text}")
    }

    /// The steps shown: all of them open, the edits shut.
    pub fn shown(&self) -> &[(SessionMessage, u64)] {
        if self.open { &self.steps } else { &self.edits }
    }
}

/// A row with who says it to whom and when, as it is drawn.
#[derive(Clone, PartialEq, Debug)]
pub struct Line {
    pub row: Row,
    pub by: Who,
    /// The one spoken to, when that needs saying.
    pub to: Option<String>,
    pub at: u64,
    /// Its hour on the chief's clock.
    pub hour: String,
    /// The first row of a run by one speaker to one listener.
    pub lead: bool,
    /// The first message of its run, which is named.
    pub named: bool,
    /// The last message of its run.
    pub last: bool,
    /// The chief can take the open conversation back to it.
    pub rewinds: bool,
    /// The name its answer is replied to under, when it can be.
    pub replies: Option<String>,
}

/// The chat as it is built, with everyone who speaks in it, the
/// agent it is with first.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Lines {
    pub rows: Vec<Line>,
    /// The rows before these that are not built.
    pub earlier: usize,
    pub cast: Vec<Who>,
    /// Nothing was said at all.
    pub empty: bool,
}

#[derive(PartialEq)]
enum Id {
    Agent(String),
    Chief(String),
    /// A name nobody here answers to.
    Stranger(String),
}

/// Everyone who speaks in a chat.
struct Cast<'a> {
    focus: &'a Focus,
    roster: &'a Roster,
    /// The hamlet of the agent the chat is with.
    hamlet: Option<String>,
    ids: Vec<Id>,
    all: Vec<Who>,
}

impl Cast<'_> {
    fn place(&mut self, id: Id, who: impl FnOnce() -> Who) -> usize {
        if let Some(at) = self.ids.iter().position(|one| *one == id) {
            return at;
        }
        self.ids.push(id);
        self.all.push(who());
        self.all.len() - 1
    }

    /// The living agent named `name`.
    fn agent(&mut self, name: &str) -> usize {
        let found = self
            .roster
            .agents
            .iter()
            .find(|agent| agent.name == name && !agent.gone)
            .map(|agent| Who {
                name: titled(&agent.name),
                tag: if agent.mayor {
                    "MAYOR".to_string()
                } else {
                    tag_of(&agent.purpose)
                },
                kind: if agent.mayor {
                    Kind::Mayor
                } else {
                    Kind::Agent
                },
            });
        let with = self.focus.with.clone();
        self.place(Id::Agent(name.to_string()), || {
            found.unwrap_or_else(|| match with {
                Recipient::Agent(name) => Who {
                    name: titled(&name),
                    ..Who::default()
                },
                Recipient::Mayor(_) => Who {
                    name: "Mayor".to_string(),
                    tag: "MAYOR".to_string(),
                    kind: Kind::Mayor,
                },
            })
        })
    }

    fn chief(&mut self, user: &str) -> usize {
        let me = user == self.focus.chief;
        self.place(Id::Chief(user.to_string()), || Who {
            name: titled(user),
            tag: String::new(),
            kind: if me { Kind::Me } else { Kind::Chief },
        })
    }

    /// The writer under `address` in the hamlet of the chat's agent.
    fn address(&mut self, address: &str) -> usize {
        let found = self
            .roster
            .agents
            .iter()
            .find(|agent| {
                !agent.gone
                    && if address == MAYOR {
                        agent.mayor
                            && Some(&agent.hamlet)
                                == self.hamlet.as_ref()
                    } else {
                        agent.name == address
                    }
            })
            .map(|agent| agent.name.clone());
        match found {
            Some(name) => self.agent(&name),
            None => {
                self.place(Id::Stranger(address.to_string()), || {
                    Who {
                        name: titled(address),
                        ..Who::default()
                    }
                })
            }
        }
    }
}

/// A row before it is placed: its speaker and listener by their
/// places in the cast, and its time.
struct Placed {
    row: Row,
    by: usize,
    to: usize,
    at: u64,
}

/// The time now, in milliseconds since 1970.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(0)
        })
}

/// One conversation with each run of thinking, tool calls and
/// notices folded.
fn rows(
    session: &SessionName,
    said: &[Kept],
    open: &Unfolded,
) -> Vec<(Row, u64)> {
    let mut rows = Vec::<(Row, u64)>::new();
    for kept in said {
        let message = &kept.message;
        let step = match &message.entry {
            // Nothing said is no message: it neither makes a bubble
            // nor parts the steps before it from those after.
            Entry::Assistant(text) if text.trim().is_empty() => {
                continue;
            }
            Entry::Tool { exit, .. } => Some((true, *exit)),
            Entry::Reasoning(_)
            | Entry::Agent { .. }
            | Entry::Notice(_) => Some((false, Some(0))),
            _ => None,
        };
        let Some((tool, exit)) = step else {
            rows.push((
                Row::Message {
                    session: session.clone(),
                    message: message.clone(),
                    rev: kept.rev,
                },
                message.at,
            ));
            continue;
        };
        if !matches!(rows.last(), Some((Row::Fold(_), _))) {
            rows.push((
                Row::Fold(Fold {
                    session: session.clone(),
                    first: message.id,
                    steps: Vec::new(),
                    edits: Vec::new(),
                    tools: 0,
                    failed: 0,
                    running: false,
                    helpers: Vec::new(),
                    open: open.0.iter().any(|(at, first)| {
                        at == session && *first == message.id
                    }),
                }),
                message.at,
            ));
        }
        let Some((Row::Fold(fold), _)) = rows.last_mut() else {
            continue;
        };
        fold.running = exit.is_none();
        if message.another {
            let name = match &message.entry {
                Entry::Agent { name, .. } => name.as_str(),
                Entry::Tool { name, .. } => {
                    name.split(':').next().unwrap_or_default()
                }
                _ => "",
            };
            if !name.is_empty()
                && !fold.helpers.iter().any(|h| h == name)
            {
                fold.helpers.push(name.to_string());
            }
            continue;
        }
        fold.steps.push((message.clone(), kept.rev));
        if matches!(
            message.entry,
            Entry::Tool {
                change: Some(_),
                ..
            }
        ) {
            fold.edits.push((message.clone(), kept.rev));
        }
        fold.tools += usize::from(tool);
        fold.failed +=
            usize::from(exit.is_some_and(|code| code != 0));
    }
    rows
}

/// The messages of `said` that answer what was `asked`: of each
/// stretch after something asked, the last thing said that says
/// anything. The last stretch has an answer only once it is `done`.
fn answers(
    said: &[Kept],
    asked: impl Fn(&SessionMessage) -> bool,
    done: bool,
) -> Vec<u64> {
    let says = |message: &SessionMessage| matches!(&message.entry, Entry::Assistant(text) if !text.trim().is_empty());
    let mut answers = Vec::new();
    let mut last = None;
    let mut begun = false;
    for kept in said {
        if asked(&kept.message) {
            answers.extend(last.take());
            begun = true;
        } else if begun && says(&kept.message) {
            last = Some(kept.message.id);
        }
    }
    if done {
        answers.extend(last);
    }
    answers
}

/// The chat of the open conversation, as `world` holds it now.
pub fn lines(world: &World) -> Lines {
    let (Some(focus), Some(roster), Some(talk)) = (
        world
            .get::<Focused>()
            .and_then(|focused| focused.0.as_ref()),
        world.get::<Roster>(),
        world.get::<Talk>(),
    ) else {
        return Lines::default();
    };
    let unfolded = world.resource::<Unfolded>();
    let reach = world.resource::<Reach>();
    let owner_name = focus.open.agent.clone();
    let hamlet = owner_name.as_ref().and_then(|name| {
        roster
            .agents
            .iter()
            .find(|agent| agent.name == *name)
            .map(|agent| agent.hamlet.clone())
    });
    let mut cast = Cast {
        focus,
        roster,
        hamlet,
        ids: Vec::new(),
        all: Vec::new(),
    };
    let owner = match &owner_name {
        Some(name) => cast.agent(name),
        None => cast.agent(""),
    };
    let since = focus.open.began;
    let mut placed = Vec::<(u64, Placed)>::new();
    for related in &talk.related {
        let session = &related.session.name;
        let Some(said) = talk.said.get(session) else {
            continue;
        };
        let elsewhere = *session != focus.open.name;
        let mut clock = 0;
        let mut place = |row: Row, by: usize, to: usize, at: u64| {
            // A new session starts the chat afresh: what the agents
            // wrote each other before it is not shown.
            if elsewhere && at < since {
                return;
            }
            clock = clock.max(at);
            placed.push((clock, Placed { row, by, to, at }));
        };
        match related.part {
            Part::With => {
                let other = match &related.from {
                    Some(from) => cast.address(from),
                    None => cast.chief(&focus.chief),
                };
                let between = related.from.is_some();
                for (row, at) in rows(session, said, unfolded) {
                    let (by, to) = match &row {
                        Row::Message { message, .. } => {
                            match &message.entry {
                                Entry::User { user, .. } => {
                                    let by =
                                        if between || message.reply {
                                            cast.address(user)
                                        } else {
                                            cast.chief(user)
                                        };
                                    (by, owner)
                                }
                                // The chief answered the agent's
                                // questions.
                                Entry::Answered(_) => {
                                    (cast.chief(&focus.chief), owner)
                                }
                                _ => (owner, other),
                            }
                        }
                        _ => (owner, other),
                    };
                    place(row, by, to, at);
                }
            }
            Part::Wrote => {
                let (Some(with), Some(from)) =
                    (&related.session.agent, &related.from)
                else {
                    continue;
                };
                let other = cast.agent(with);
                // Its questions, and the answer to each: the last
                // thing the other said before it was asked again,
                // or before it was done.
                let asked = |message: &SessionMessage| matches!(&message.entry, Entry::User { user, .. } if user == from);
                let answered = if related.asked {
                    answers(said, asked, !related.session.answering)
                } else {
                    Vec::new()
                };
                for kept in said {
                    let message = &kept.message;
                    let (by, to) = if asked(message) {
                        (owner, other)
                    } else if answered.contains(&message.id) {
                        (other, owner)
                    } else {
                        continue;
                    };
                    let row = Row::Message {
                        session: session.clone(),
                        message: message.clone(),
                        rev: kept.rev,
                    };
                    place(row, by, to, message.at);
                }
            }
        }
    }
    let me = cast.chief(&focus.chief);
    for (at, notice) in &talk.notices {
        placed.push((
            *at,
            Placed {
                row: Row::Notice(notice.clone()),
                by: owner,
                to: me,
                at: *at,
            },
        ));
    }
    placed.sort_by_key(|(at, _)| *at);
    let working = talk
        .related
        .first()
        .is_some_and(|open| open.session.working);
    let all = cast.all;
    let mut lines = placed
        .into_iter()
        .map(|(_, placed)| {
            let mut by = all[placed.by].clone();
            // The head of the chat says what its own agent is for.
            if placed.by == 0 {
                by.tag.clear();
            }
            // What is said to the agent the chat is with, or to the
            // chief here, needs no saying.
            let to = all
                .get(placed.to)
                .filter(|to| placed.to != 0 && to.kind != Kind::Me)
                .map(|to| to.name.clone());
            let (rewinds, replies) = match &placed.row {
                Row::Message {
                    session, message, ..
                } => (
                    by.kind == Kind::Me
                        && !working
                        && !message.reply
                        && *session == focus.open.name
                        && matches!(
                            message.entry,
                            Entry::User { .. }
                        ),
                    answers_of(&message.entry)
                        .map(|_| by.name.clone()),
                ),
                _ => (false, None),
            };
            Line {
                hour: hour(placed.at, focus.offset),
                row: placed.row,
                to,
                at: placed.at,
                lead: false,
                named: false,
                last: false,
                rewinds,
                replies,
                by,
            }
        })
        .collect::<Vec<_>>();
    mark_runs(&mut lines);
    let empty = lines.is_empty();
    let earlier = lines.len().saturating_sub(reach.0);
    Lines {
        rows: lines.split_off(earlier),
        earlier,
        cast: all,
        empty,
    }
}

/// A row that is a message, which a run is named by and ends on.
fn message(line: &Line) -> bool {
    matches!(line.row, Row::Message { .. })
}

/// Sets where each run of rows by one speaker to one listener begins,
/// and its first and last message.
fn mark_runs(lines: &mut [Line]) {
    let mut start = 0;
    while start < lines.len() {
        let run = (lines[start].by.clone(), lines[start].to.clone());
        let end = lines[start..]
            .iter()
            .position(|line| (&line.by, &line.to) != (&run.0, &run.1))
            .map_or(lines.len(), |more| start + more);
        lines[start].lead = true;
        let run = &mut lines[start..end];
        if let Some(first) = run.iter_mut().find(|line| message(line))
        {
            first.named = true;
        }
        if let Some(last) = run.iter_mut().rfind(|line| message(line))
        {
            last.last = true;
        }
        start = end;
    }
}

/// Builds the chat again from what `world` holds, when it changed.
pub fn refresh(world: &mut World) {
    let now = lines(world);
    world.keep(now);
}

/// Puts a notice of the chat's own in it, now.
pub fn notice(world: &mut World, text: &str) {
    let at = now();
    let notices = &mut world.resource_mut::<Talk>().notices;
    notices.push((at, text.to_string()));
    let over = notices.len().saturating_sub(NOTICES);
    notices.drain(..over);
}
