//! An agent's chat as one line of time, with who speaks in it.

use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::color::{Color, ColorToPacked};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::query::{Added, Changed, Has, Or, With};
use bevy::ecs::system::{Query, Res, SystemParam};
use bevy::math::Vec3;
use hamlet_client::chats::{Entry, Transcript};
use hamlet_client::realm::{
    HamletAgent, Identity, Kind as AgentKind, Lead, LivesIn, Purpose,
    Roster, Shown,
};
use hamlet_protocol::client as protocol;
use hamlet_realm::chief::{Chief, Look};
use hamlet_realm::hamlet::Housed;
use hamlet_realm::keeper;
use hamlet_realm::palette::Palette;
use hamlet_ui::theme::Themed;

use super::lines::{Row, rows};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Kind {
    /// The chief at this window.
    Me,
    Chief,
    Lead,
    /// The realm's own agent.
    Hand,
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
            Option<&'static Identity>,
            Has<Lead>,
            Option<&'static Purpose>,
            Option<&'static LivesIn>,
            Option<&'static Housed>,
            Option<&'static AgentKind>,
            Option<&'static Shown>,
        ),
    >,
    roster: Roster<'w, 's>,
    chiefs: Query<'w, 's, (&'static Chief, &'static Look)>,
    me: Option<Res<'w, hamlet_client::realm::Chief>>,
    palette: Option<Res<'w, Palette>>,
    theme: Themed<'w>,
    changed: Query<
        'w,
        's,
        (),
        (
            With<HamletAgent>,
            Or<(
                Changed<Name>,
                Changed<Shown>,
                Changed<Purpose>,
                Changed<Housed>,
                Changed<LivesIn>,
                Added<Lead>,
            )>,
        ),
    >,
}

impl People<'_, '_> {
    /// An agent's name, look or place, or the name this window's
    /// chief signs in with, may have changed.
    pub(super) fn has_moved(&self) -> bool {
        !self.changed.is_empty()
            || self.me.as_ref().is_some_and(DetectChanges::is_changed)
    }

    /// The name this window's chief signs in with.
    pub(super) fn me(&self) -> String {
        self.me.as_ref().map(|me| me.0.clone()).unwrap_or_default()
    }

    fn shown(colour: Vec3) -> Color {
        Color::linear_rgb(colour.x, colour.y, colour.z)
    }

    fn agent(&self, agent: Entity) -> Option<Who> {
        let (_, name, id, lead, purpose, _, housed, kind, shown) =
            self.agents.get(agent).ok()?;
        let hand = kind
            .is_some_and(|kind| kind.0 == protocol::AgentKind::Hand);
        // An agent of a hamlet not yet housed is drawn by its id.
        let unhoused = id.filter(|_| {
            kind.is_some_and(|kind| {
                kind.0 == protocol::AgentKind::Hamlet
            })
        });
        let look = match (lead || hand, housed, unhoused) {
            (true, ..) => keeper::Who::lead(),
            (_, Some(housed), _) => {
                keeper::Who::agent(housed.0.0 * 7 + 3)
            }
            (_, None, Some(id)) => {
                keeper::Who::agent(id.0.0 as u32 * 7 + 3)
            }
            (_, None, None) => keeper::Who::agent(1),
        };
        let paints = match &self.palette {
            Some(palette) => palette.paints,
            None => Palette::default().paints,
        };
        Some(Who {
            // What it is shown by, else its address.
            name: match (hand, shown) {
                (true, _) => "The Hand".to_string(),
                (_, Some(shown)) => shown.0.clone(),
                _ => name.as_str().to_string(),
            },
            tag: if hand {
                "Hand".to_string()
            } else if lead {
                "Lead".to_string()
            } else {
                tag_of(purpose.map_or("", |p| p.0.as_str()))
            },
            kind: match (hand, lead) {
                (true, _) => Kind::Hand,
                (_, true) => Kind::Lead,
                _ => Kind::Agent,
            },
            paint: Self::shown(paints[look.paint as usize]),
        })
    }

    fn chief(&self, name: &str, me: &str) -> Who {
        let cloak = self
            .chiefs
            .iter()
            .find(|(chief, _)| chief.name == name)
            .map(|(_, look)| match &self.palette {
                Some(palette) => look.cloak.colour(palette),
                None => look.cloak.colour(&Palette::default()),
            });
        Who {
            name: titled(name),
            tag: String::new(),
            kind: if name == me { Kind::Me } else { Kind::Chief },
            paint: cloak
                .map_or(self.theme.palette.gold_5, Self::shown),
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
    /// The name this window's chief signs in with.
    me: &'a str,
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

    /// `agent`, drawn by `name` when the window does not know it.
    fn agent(&mut self, agent: Entity, name: Option<&str>) -> usize {
        let people = self.people;
        self.place(Id::Agent(agent), || {
            people.agent(agent).unwrap_or_else(|| match name {
                Some(name) => Who {
                    name: name.to_string(),
                    ..Who::default()
                },
                None => Who {
                    name: "The Hand".to_string(),
                    tag: "Hand".to_string(),
                    kind: Kind::Hand,
                    paint: people.theme.palette.gold_5,
                },
            })
        })
    }

    /// Another agent writing to the chat, else its address `name`.
    fn peer(
        &mut self,
        from: Option<protocol::AgentId>,
        name: &str,
    ) -> usize {
        let known =
            from.and_then(|from| self.people.roster.agent(from));
        if let Some(agent) = known
            && self.people.agent(agent).is_some()
        {
            return self.agent(agent, Some(name));
        }
        let paint = self.people.theme.palette.gold_5;
        self.place(Id::Peer(name.into()), || Who {
            name: name.into(),
            tag: "Agent".into(),
            kind: Kind::Agent,
            paint,
        })
    }

    fn chief(&mut self, name: &str) -> usize {
        let (people, me) = (self.people, self.me);
        self.place(Id::Chief(name.to_string()), || {
            people.chief(name, me)
        })
    }
}

/// A row of the chat with its speaker, its listener and its time.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Line {
    pub row: Row,
    /// The agent whose chat it is drawn in.
    pub agent: Entity,
    /// The speaker and the one spoken to, as [`Who`]s of the chat.
    pub by: usize,
    pub to: usize,
    /// In milliseconds since 1970; 0 when not known.
    pub at: u64,
    /// Changes when the speaker's or listener's name or look does.
    pub look: u64,
    /// The first row of a run by one speaker to one listener.
    pub lead: bool,
    /// The first message of its run, which is named.
    pub named: bool,
}

/// Agent `of`'s conversation `chat` in order, with its speakers.
pub(super) fn timeline(
    me: &str,
    of: (Entity, Option<&str>),
    chat: Entity,
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
    let agent = cast.agent(of.0, of.1);
    let chief = cast.chief(me);
    let mut lines = rows(chat, transcript, open)
        .into_iter()
        .map(|row| {
            // A turn is the agent's, whatever it is known by.
            let (item, at) = match &row {
                Row::Entry { id, .. } | Row::Card { id, .. } => {
                    let item = transcript.item(id);
                    (item, item.map_or(0, |item| item.at))
                }
                Row::Fold { start, .. } => (None, *start),
                Row::Changes { at, .. } => (None, *at),
            };
            let (by, to) = match item.map(|item| &item.entry) {
                Some(Entry::Agent { name, from, .. }) => {
                    (cast.peer(*from, name), agent)
                }
                Some(Entry::Chief { name, .. }) => {
                    (cast.chief(name), agent)
                }
                Some(Entry::Answered(_)) => (chief, agent),
                Some(
                    Entry::Assistant(_)
                    | Entry::Reasoning(_)
                    | Entry::Tool { .. }
                    | Entry::Error(_)
                    | Entry::Notice(_)
                    | Entry::Summary(_)
                    | Entry::Merged { .. }
                    | Entry::Event(_),
                )
                | None => (agent, chief),
            };
            Line {
                row,
                agent: of.0,
                by,
                to,
                at,
                look: 0,
                lead: false,
                named: false,
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
fn is_message(line: &Line) -> bool {
    matches!(line.row, Row::Entry { .. })
}

/// A row that stands apart from the runs around it.
fn is_card(line: &Line) -> bool {
    matches!(line.row, Row::Card { .. })
}

/// Marks where each run of one speaker to one listener begins.
fn mark_runs(lines: &mut [Line]) {
    let mut start = 0;
    while start < lines.len() {
        let run = (lines[start].by, lines[start].to);
        let end = match is_card(&lines[start]) {
            true => start + 1,
            false => lines[start..]
                .iter()
                .position(|line| {
                    (line.by, line.to) != run || is_card(line)
                })
                .map_or(lines.len(), |more| start + more),
        };
        lines[start].lead = true;
        let run = &mut lines[start..end];
        if let Some(first) = run.iter_mut().find(|l| is_message(l)) {
            first.named = true;
        }
        start = end;
    }
}
