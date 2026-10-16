//! An agent's chat as one line of time: the conversations it is a
//! party to, who speaks in them, and what was said in the order it
//! was said.

use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::color::{Color, ColorToPacked};
use bevy::ecs::change_detection::Ref;
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::query::{Added, Changed, Has, Or, With};
use bevy::ecs::system::{Query, Res, SystemParam};
use bevy::math::Vec3;
use hamlet_core::agents::{Citizen, LivesIn, Mayor, Purpose};
use hamlet_core::session::{
    Entry, Item, SessionKey, TalksTo, Transcript, reply_in,
};
use hamlet_rules::hamlet::MAYOR;
use hamlet_world::chief::{Chief, Look};
use hamlet_world::hamlet::Housed;
use hamlet_world::keeper;
use hamlet_world::palette::Palette;

use super::lines::{Row, rows};
use crate::me::Me;
use crate::theme::Themed;

/// The part a conversation has in an agent's chat.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Part {
    /// A conversation with the agent. All of it is shown.
    With,
    /// A conversation the agent wrote to another. Its own messages
    /// are shown, and the answers to what it asked and waited on.
    Wrote,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Kind {
    /// The chief at this window.
    Me,
    Chief,
    Mayor,
    #[default]
    Agent,
}

/// Someone who speaks in a chat.
#[derive(Clone, PartialEq, Debug, Default)]
pub(super) struct Who {
    pub name: String,
    /// Its office, or the start of its purpose.
    pub tag: String,
    pub kind: Kind,
    /// The paint of its robot, or a chief's cloak.
    pub paint: Color,
}

/// The longest tag, in letters.
const TAG: usize = 24;

/// Words a tag does not end on.
const LOOSE: [&str; 12] = [
    "a", "an", "and", "the", "its", "of", "to", "in", "for", "with",
    "on", "or",
];

/// The start of a purpose, cut at a word.
fn tag_of(purpose: &str) -> String {
    let stops = ['.', ',', ';', ':'];
    let words = purpose
        .split_whitespace()
        .map(|word| word.trim_end_matches(stops).to_lowercase())
        .collect::<Vec<_>>();
    let mut letters = 0;
    let mut kept = words
        .iter()
        .take_while(|word| {
            letters += word.chars().count() + 1;
            letters <= TAG + 1
        })
        .count();
    if kept == words.len() {
        return words.join(" ");
    }
    while kept > 1 && LOOSE.contains(&words[kept - 1].as_str()) {
        kept -= 1;
    }
    format!("{}...", words[..kept].join(" "))
}

/// `name` with its first letter a capital.
fn titled(name: &str) -> String {
    let mut letters = name.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

/// The agents and chiefs a chat can name.
#[derive(SystemParam)]
pub struct People<'w, 's> {
    agents: Query<
        'w,
        's,
        (
            Entity,
            &'static Name,
            Option<&'static Citizen>,
            Has<Mayor>,
            Option<&'static Purpose>,
            Option<&'static LivesIn>,
            Option<&'static Housed>,
        ),
    >,
    chiefs: Query<'w, 's, (&'static Chief, &'static Look)>,
    palette: Option<Res<'w, Palette>>,
    theme: Themed<'w>,
    changed: Query<
        'w,
        's,
        (),
        (
            With<Citizen>,
            Or<(
                Changed<Name>,
                Changed<Purpose>,
                Changed<Housed>,
                Changed<LivesIn>,
                Added<Mayor>,
            )>,
        ),
    >,
}

impl People<'_, '_> {
    /// An agent's name, look or place may have changed.
    pub(super) fn moved(&self) -> bool {
        !self.changed.is_empty()
    }

    fn shown(colour: Vec3) -> Color {
        Color::linear_rgb(colour.x, colour.y, colour.z)
    }

    fn lives(&self, agent: Entity) -> Option<Entity> {
        let (.., lives, _) = self.agents.get(agent).ok()?;
        lives.map(|lives| lives.0)
    }

    /// The name `agent` is written to and writes under.
    fn address(&self, agent: Entity) -> Option<String> {
        let (_, name, _, mayor, ..) = self.agents.get(agent).ok()?;
        Some(if mayor {
            MAYOR.to_string()
        } else {
            name.as_str().to_string()
        })
    }

    /// The agent at `address`, as one of the hamlet of `near`
    /// writes it.
    fn at(&self, address: &str, near: Entity) -> Option<Entity> {
        let lives = self.lives(near);
        self.agents
            .iter()
            .filter(|(_, _, citizen, ..)| citizen.is_some())
            .find(|(_, name, _, mayor, _, at, _)| {
                if address == MAYOR {
                    *mayor && at.map(|at| at.0) == lives
                } else {
                    name.as_str() == address
                }
            })
            .map(|(entity, ..)| entity)
    }

    fn agent(&self, agent: Entity) -> Option<Who> {
        let (_, name, citizen, mayor, purpose, _, housed) =
            self.agents.get(agent).ok()?;
        let look = match (mayor, housed, citizen) {
            (true, ..) => keeper::Who::leader(),
            (_, Some(housed), _) => {
                keeper::Who::citizen(housed.0.0 * 7 + 3)
            }
            (_, None, Some(citizen)) => {
                keeper::Who::citizen(citizen.0 as u32 * 7 + 3)
            }
            (_, None, None) => keeper::Who::citizen(1),
        };
        let paints = match &self.palette {
            Some(palette) => palette.paints,
            None => Palette::default().paints,
        };
        Some(Who {
            name: titled(name.as_str()),
            tag: if mayor {
                "MAYOR".to_string()
            } else {
                tag_of(purpose.map_or("", |p| p.0.as_str()))
            },
            kind: if mayor { Kind::Mayor } else { Kind::Agent },
            paint: Self::shown(paints[look.paint as usize]),
        })
    }

    fn chief(&self, user: &str, me: &Me) -> Who {
        let cloak = self
            .chiefs
            .iter()
            .find(|(chief, _)| chief.user == user)
            .map(|(_, look)| match &self.palette {
                Some(palette) => look.cloak.colour(palette),
                None => look.cloak.colour(&Palette::default()),
            });
        Who {
            name: titled(user),
            tag: String::new(),
            kind: if user == me.user {
                Kind::Me
            } else {
                Kind::Chief
            },
            paint: cloak
                .map_or(self.theme.palette.brass_5, Self::shown),
        }
    }
}

#[derive(PartialEq)]
enum Id {
    Agent(Entity),
    Chief(String),
    /// A name nobody here answers to.
    Stranger(String),
}

/// Everyone who speaks in a chat, the agent it is with first.
struct Cast<'a, 'w, 's> {
    people: &'a People<'w, 's>,
    me: &'a Me,
    ids: Vec<Id>,
    all: Vec<Who>,
}

impl Cast<'_, '_, '_> {
    fn place(&mut self, id: Id, who: impl FnOnce() -> Who) -> usize {
        if let Some(at) = self.ids.iter().position(|i| *i == id) {
            return at;
        }
        self.ids.push(id);
        self.all.push(who());
        self.all.len() - 1
    }

    fn agent(&mut self, agent: Entity) -> usize {
        let (people, me) = (self.people, self.me);
        self.place(Id::Agent(agent), || {
            people.agent(agent).unwrap_or_else(|| match &me.with {
                Some(name) => Who {
                    name: titled(name),
                    ..Who::default()
                },
                None => Who {
                    name: "Mayor".to_string(),
                    tag: "MAYOR".to_string(),
                    kind: Kind::Mayor,
                    paint: people.theme.palette.brass_5,
                },
            })
        })
    }

    fn chief(&mut self, user: &str) -> usize {
        let (people, me) = (self.people, self.me);
        self.place(Id::Chief(user.to_string()), || {
            people.chief(user, me)
        })
    }

    /// The writer under `address` in the hamlet of the chat's agent.
    fn address(&mut self, address: &str) -> usize {
        match self.people.at(address, self.me.agent) {
            Some(agent) => self.agent(agent),
            None => {
                self.place(Id::Stranger(address.to_string()), || {
                    Who {
                        name: titled(address),
                        paint: self.people.theme.palette.brass_5,
                        ..Who::default()
                    }
                })
            }
        }
    }
}

/// A row of the chat with its speaker, its listener and its time.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Line {
    pub row: Row,
    /// The agent whose chat it is drawn in.
    pub owner: Entity,
    /// The speaker and the one spoken to, as [`Who`]s of the chat.
    pub by: usize,
    pub to: usize,
    /// The time it was said, in milliseconds since 1970. 0 when that
    /// is not known.
    pub at: u64,
    /// A number that changes when the speaker's or the listener's
    /// name or look does.
    pub look: u64,
    /// The first row of a run of rows by one speaker to one
    /// listener.
    pub lead: bool,
    /// The first message of its run, which is the one that is
    /// named.
    pub named: bool,
    /// The last message of its run, which the speaker's picture
    /// stands by.
    pub last: bool,
}

/// The conversations the chat with `me.agent` holds, the chief's
/// own with it first.
pub(super) fn parts(
    me: &Me,
    sessions: &Query<(Entity, &SessionKey, Option<&TalksTo>)>,
    people: &People,
) -> Vec<(Entity, Part)> {
    let mut parts = vec![(me.session, Part::With)];
    let address = people.address(me.agent);
    let lives = people.lives(me.agent);
    let mut others = sessions
        .iter()
        .filter(|(session, key, _)| {
            *session != me.session
                && (key.is_mail() || key.is_asked())
        })
        .filter_map(|(session, key, to)| {
            let to = to?.0;
            if to == me.agent {
                return Some((session, Part::With));
            }
            let wrote = address.as_deref() == Some(&key.chat)
                && people.lives(to) == lives;
            wrote.then_some((session, Part::Wrote))
        })
        .collect::<Vec<_>>();
    others.sort_by_key(|(session, _)| *session);
    parts.extend(others);
    parts
}

/// The time the entry `id` was said.
fn said_at(transcript: &Transcript, id: u64) -> u64 {
    transcript.item(id).map_or(0, |item| item.at)
}

/// Every row of the conversations in `parts`, in the order they were
/// said, with everyone who speaks in them. Of the conversations
/// besides the chief's own, only what was said from `since` on.
pub(super) fn timeline(
    me: &Me,
    parts: &[(Entity, Part)],
    keys: &Query<(Entity, &SessionKey, Option<&TalksTo>)>,
    transcripts: &Query<Ref<Transcript>>,
    open: &[(Entity, u64)],
    people: &People,
    since: u64,
) -> (Vec<Line>, Vec<Who>) {
    let mut cast = Cast {
        people,
        me,
        ids: Vec::new(),
        all: Vec::new(),
    };
    let owner = cast.agent(me.agent);
    // Each row with the time it is sorted by, which never runs
    // back within a conversation.
    let mut placed = Vec::<(u64, Line)>::new();
    for (session, part) in parts {
        let Ok(transcript) = transcripts.get(*session) else {
            continue;
        };
        let key = keys.get(*session).ok();
        let between = key.is_some_and(|(_, key, _)| {
            key.is_mail() || key.is_asked()
        });
        let mut clock = 0;
        let mut place = |row: Row, by: usize, to: usize, at: u64| {
            // A new session starts the chat afresh: what the
            // agents wrote each other before it is not shown.
            if *session != me.session && at < since {
                return;
            }
            clock = clock.max(at);
            let line = Line {
                row,
                owner: me.agent,
                by,
                to,
                at,
                look: 0,
                lead: false,
                named: false,
                last: false,
            };
            placed.push((clock, line));
        };
        match (part, key) {
            (Part::With, _) => {
                let other = match key {
                    Some((_, key, _)) if between => {
                        cast.address(&key.chat)
                    }
                    _ => cast.chief(&me.user),
                };
                for row in rows(*session, &transcript, open) {
                    let first = match &row {
                        Row::Entry { id, .. } => *id,
                        Row::Fold { first, .. } => *first,
                    };
                    let said = match &row {
                        Row::Entry { id, .. } => transcript.get(*id),
                        Row::Fold { .. } => None,
                    };
                    let (by, to) = match said {
                        Some(Entry::User { user, text }) => {
                            let agent = between
                                || reply_in(user, text).is_some();
                            let by = if agent {
                                cast.address(user)
                            } else {
                                cast.chief(user)
                            };
                            (by, owner)
                        }
                        // The chief answered the agent's questions.
                        Some(Entry::Answered(_)) => {
                            (cast.chief(&me.user), owner)
                        }
                        _ => (owner, other),
                    };
                    place(row, by, to, said_at(&transcript, first));
                }
            }
            (Part::Wrote, Some((_, key, with))) => {
                let other = match with {
                    Some(with) => cast.agent(with.0),
                    None => continue,
                };
                // Its questions, and the answer to each: the last
                // thing the other said before it was asked again, or
                // before it was done. The other's work on the way
                // there is in its own chat. A letter's answer comes
                // back as a message of its own.
                let asked = |item: &Item| {
                    matches!(&item.entry, Entry::User { user, .. }
                        if *user == key.chat)
                };
                let items = transcript.items();
                let answers = match key.is_asked() {
                    true => {
                        answers(items, asked, !transcript.answering())
                    }
                    false => Vec::new(),
                };
                for item in items {
                    let (by, to) = if asked(item) {
                        (owner, other)
                    } else if answers.contains(&item.id) {
                        (other, owner)
                    } else {
                        continue;
                    };
                    let row = Row::Entry {
                        session: *session,
                        id: item.id,
                    };
                    place(row, by, to, item.at);
                }
            }
            (Part::Wrote, None) => {}
        }
    }
    placed.sort_by_key(|(at, _)| *at);
    let looks = cast.all.iter().map(look).collect::<Vec<_>>();
    let mut lines = placed
        .into_iter()
        .map(|(_, line)| Line {
            look: looks[line.by] ^ looks[line.to].rotate_left(1),
            ..line
        })
        .collect::<Vec<_>>();
    mark_runs(&mut lines);
    (lines, cast.all)
}

/// The entries of `items` that answer what was asked: of each stretch
/// after something `asked`, the last thing said that says anything.
/// The last stretch has an answer only once it is `done`.
fn answers(
    items: &[Item],
    asked: impl Fn(&Item) -> bool,
    done: bool,
) -> Vec<u64> {
    let said = |item: &Item| {
        matches!(&item.entry, Entry::Assistant(text)
            if !text.trim().is_empty())
    };
    let mut answers = Vec::new();
    let mut last = None;
    let mut begun = false;
    for item in items {
        if asked(item) {
            answers.extend(last.take());
            begun = true;
        } else if begun && said(item) {
            last = Some(item.id);
        }
    }
    if done {
        answers.extend(last);
    }
    answers
}

/// A number that stands for how `who` is drawn.
fn look(who: &Who) -> u64 {
    let mut hasher = DefaultHasher::new();
    let paint = who.paint.to_srgba().to_u8_array();
    (&who.name, &who.tag, who.kind as u8, paint).hash(&mut hasher);
    hasher.finish()
}

/// A row that is a message, which a run is named by and ends on.
fn message(line: &Line) -> bool {
    matches!(line.row, Row::Entry { .. })
}

/// Sets where each run of rows by one speaker to one listener
/// begins, and its first and last message.
fn mark_runs(lines: &mut [Line]) {
    let mut start = 0;
    while start < lines.len() {
        let run = (lines[start].by, lines[start].to);
        let end = lines[start..]
            .iter()
            .position(|line| (line.by, line.to) != run)
            .map_or(lines.len(), |more| start + more);
        lines[start].lead = true;
        let run = &mut lines[start..end];
        if let Some(first) = run.iter_mut().find(|l| message(l)) {
            first.named = true;
        }
        if let Some(last) = run.iter_mut().rfind(|l| message(l)) {
            last.last = true;
        }
        start = end;
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn of_what_a_helper_says_only_its_answer_is_the_asker_s_to_read()
    {
        let says = |text: &str| json!({"type": "assistant", "message": {"content": text}});
        let asks = |text: &str| json!({"type": "user", "user": "mayor", "content": text});
        let transcript = Transcript::from_log(&[
            asks("fix the tags"),
            says("I'll start by reading the key files."),
            json!({"type": "tool", "exit_code": 0}),
            says("Now the two edits."),
            says("Fixed: the tags are clipped by the panel."),
            asks("and the signs"),
            says("Reading the signs."),
        ]);
        let items = transcript.items();
        let asked = |item: &Item| {
            matches!(&item.entry, Entry::User { user, .. }
                if user == "mayor")
        };
        let text = |ids: Vec<u64>| {
            ids.into_iter()
                .filter_map(|id| match transcript.get(id) {
                    Some(Entry::Assistant(text)) => {
                        Some(text.clone())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        // While the helper is still at its second task, the first
        // one's last word is its answer, and nothing of the second.
        assert_eq!(
            text(answers(items, asked, false)),
            ["Fixed: the tags are clipped by the panel."]
        );
        // Done, its last word is the answer to the second.
        assert_eq!(
            text(answers(items, asked, true)),
            [
                "Fixed: the tags are clipped by the panel.",
                "Reading the signs."
            ]
        );
    }
}
