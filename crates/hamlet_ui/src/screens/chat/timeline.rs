//! An agent's chat as one line of time: the conversations it is a
//! party to, who speaks in them, and what was said in the order it
//! was said.

use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::color::{Color, ColorToPacked};
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::query::{Added, Changed, Has, Or, With};
use bevy::ecs::system::{Query, Res, SystemParam};
use bevy::math::Vec3;
use hamlet_client::chats::{Entry, Transcript};
use hamlet_client::realm::{Citizen, LivesIn, Mayor, Purpose};
use hamlet_realm::chief::{Chief, Look};
use hamlet_realm::hamlet::Housed;
use hamlet_realm::keeper;
use hamlet_realm::palette::Palette;

use super::lines::{Row, rows};
use crate::me::Me;
use crate::theme::Themed;

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
    Peer(String),
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

    fn peer(&mut self, name: &str) -> usize {
        let paint = self.people.theme.palette.brass_5;
        self.place(Id::Peer(name.into()), || Who {
            name: name.into(),
            tag: "AGENT".into(),
            kind: Kind::Agent,
            paint,
        })
    }

    fn chief(&mut self, user: &str) -> usize {
        let (people, me) = (self.people, self.me);
        self.place(Id::Chief(user.to_string()), || {
            people.chief(user, me)
        })
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

/// The received chat in display order with its speakers.
pub(super) fn timeline(
    me: &Me,
    transcript: &Transcript,
    open: &[(Entity, String)],
    people: &People,
) -> (Vec<Line>, Vec<Who>) {
    let mut cast = Cast {
        people,
        me,
        ids: Vec::new(),
        all: Vec::new(),
    };
    let owner = cast.agent(me.agent);
    let chief = cast.chief(&me.user);
    let mut lines = rows(me.session, transcript, open)
        .into_iter()
        .map(|row| {
            let first = match &row {
                Row::Entry { id, .. } => id,
                Row::Fold { first, .. } => first,
            };
            let item = transcript.item(first);
            let (by, to) = match item.map(|item| &item.entry) {
                Some(Entry::Agent { name, .. }) => {
                    (cast.peer(name), owner)
                }
                Some(Entry::User { user, .. }) => {
                    (cast.chief(user), owner)
                }
                Some(Entry::Answered(_)) => (chief, owner),
                _ => (owner, chief),
            };
            Line {
                row,
                owner: me.agent,
                by,
                to,
                at: item.map_or(0, |item| item.at),
                look: 0,
                lead: false,
                named: false,
                last: false,
            }
        })
        .collect::<Vec<_>>();
    let looks = cast.all.iter().map(look).collect::<Vec<_>>();
    for line in &mut lines {
        line.look = looks[line.by] ^ looks[line.to].rotate_left(1);
    }
    mark_runs(&mut lines);
    (lines, cast.all)
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
