//! The conversation as rows: each entry, the steps of a turn folded
//! into one, and only the end of a long chat built.

use std::collections::HashMap;

use bevy::camera::visibility::Visibility;
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::{Changed, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Local, Query, Res, ResMut};
use bevy::text::{LineBreak, TextLayout};
use bevy::ui::{
    AlignItems, ComputedNode, Overflow, OverflowAxis, ScrollPosition,
    UiGlobalTransform, percent, px,
};
use bevy_fynix::scroll::ScrollGoal;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, ghost, label, scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, each, resource};
use hamlet_core::session::{
    Began, Entry, SessionKey, TalksTo, Transcript,
};

use super::entry::entry_view;
use super::timeline::{Line, Part, People, Who, parts, timeline};
use super::{ChatScroll, bubble};
use crate::View;
use crate::looks::{Fills, note};
use crate::me::Me;

/// Messages are people talking, so they get the width of a readable
/// line.
const LINE: f32 = 760.0;

/// The least width the conversation is laid out at. A panel
/// narrower than this cuts it off.
const LEAST: f32 = 240.0;

const EMPTY: &str = "Nothing has been said here yet. Agents can \
                     make mistakes, so check anything important.";

/// Folds that are open, by their conversation and first entry.
#[derive(Resource, Default)]
pub struct Unfolded(pub Vec<(Entity, u64)>);

/// A row of a conversation: one entry, or the steps of a turn
/// folded into one.
#[derive(Clone, PartialEq, Debug)]
pub(super) enum Row {
    Entry {
        session: Entity,
        id: u64,
    },
    Fold {
        session: Entity,
        /// The first entry folded in, shown or not.
        first: u64,
        /// The steps shown when it is open.
        steps: Vec<u64>,
        /// The edits among them, whose changes are shown when it is
        /// shut too.
        edits: Vec<u64>,
        tools: usize,
        failed: usize,
        running: bool,
        /// The agents that were asked and worked meanwhile. Their
        /// own steps are in their own conversations.
        helpers: Vec<String>,
        open: bool,
    },
}

/// One conversation with each run of thinking, tool calls and
/// notices folded.
pub(super) fn rows(
    session: Entity,
    transcript: &Transcript,
    open: &[(Entity, u64)],
) -> Vec<Row> {
    let mut rows = Vec::<Row>::new();
    for item in transcript.items() {
        let step = match &item.entry {
            // Nothing said is no message: a model that calls a tool
            // often says nothing with it, and that neither makes a
            // bubble nor parts the steps before it from those after.
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
            rows.push(Row::Entry {
                session,
                id: item.id,
            });
            continue;
        };
        if !matches!(rows.last(), Some(Row::Fold { .. })) {
            rows.push(Row::Fold {
                session,
                first: item.id,
                steps: Vec::new(),
                edits: Vec::new(),
                tools: 0,
                failed: 0,
                running: false,
                helpers: Vec::new(),
                open: open.contains(&(session, item.id)),
            });
        }
        let Some(Row::Fold {
            steps,
            edits,
            tools,
            failed,
            running,
            helpers,
            ..
        }) = rows.last_mut()
        else {
            continue;
        };
        *running = exit.is_none();
        if transcript.of_another(item.id) {
            let name = match &item.entry {
                Entry::Agent { name, .. } => name.as_str(),
                Entry::Tool { name, .. } => {
                    name.split(':').next().unwrap_or_default()
                }
                _ => "",
            };
            if !name.is_empty() && !helpers.iter().any(|h| h == name)
            {
                helpers.push(name.to_string());
            }
            continue;
        }
        steps.push(item.id);
        if matches!(
            item.entry,
            Entry::Tool {
                change: Some(_),
                ..
            }
        ) {
            edits.push(item.id);
        }
        *tools += tool as usize;
        *failed += exit.is_some_and(|c| c != 0) as usize;
    }
    rows
}

/// The steps of a turn as one line that opens.
pub(super) fn fold_view(
    key: RowKey,
    session: Entity,
    first: u64,
) -> impl View {
    // The row is built once for its fold. Its heading and what is
    // inside read the fold as it stands, so opening it or a new step
    // builds neither the row nor its button again.
    let head = button(
        label(resource::<Lines, _>(move |l| l.heading(key)))
            .size(12.0)
            .tone(resource::<Lines, _>(move |l| l.tone(key))),
    )
    .rules(ghost)
    .on_activate(move |world| {
        let unfolded = &mut world.resource_mut::<Unfolded>().0;
        let fold = (session, first);
        match unfolded.iter().position(|open| *open == fold) {
            Some(at) => {
                unfolded.remove(at);
            }
            None => unfolded.push(fold),
        }
    });
    let inside = each(
        resource::<Lines, _>(move |l| l.shown(key)),
        |id: &u64| *id,
        move |id| entry_view(session, *id),
    )
    // As wide as the chat and no wider, so that what is inside is
    // measured at the width it is shown at.
    .within(
        column(()).gap(6.0).width(percent(100.0)).min_width(px(0.0)),
    );
    column((head, inside)).gap(4.0).align(AlignItems::FlexStart)
}

/// The identity of a row for as long as it is the same row: the agent
/// whose chat it is in, its conversation, and its entry with the look
/// of its speaker, or the first step of its fold.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum RowKey {
    Entry(Entity, Entity, u64, u64),
    Fold(Entity, Entity, u64),
}

impl Line {
    pub(super) fn key(&self) -> RowKey {
        match &self.row {
            Row::Entry { session, id } => {
                RowKey::Entry(self.owner, *session, *id, self.look)
            }
            Row::Fold { session, first, .. } => {
                RowKey::Fold(self.owner, *session, *first)
            }
        }
    }
}

/// The part of a chat that is built: only its end, and more of it as
/// it is read back.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct Reach {
    /// The rows built from the end, and the rows each step back
    /// adds.
    pub rows: usize,
    pub page: usize,
}

impl Default for Reach {
    fn default() -> Self {
        Self { rows: 24, page: 24 }
    }
}

/// The rows of the chat that are built, kept in step with the
/// conversations and the open folds.
#[derive(Resource, Default, PartialEq)]
pub struct Lines {
    rows: Vec<Line>,
    /// The place of each row in `rows`.
    places: HashMap<RowKey, usize>,
    /// The rows before these that are not built.
    earlier: usize,
    /// Everyone who speaks in the chat, the agent it is with first.
    cast: Vec<Who>,
}

impl Lines {
    fn of(rows: Vec<Line>, earlier: usize, cast: Vec<Who>) -> Self {
        let places = rows
            .iter()
            .enumerate()
            .map(|(at, line)| (line.key(), at))
            .collect();
        Lines {
            rows,
            places,
            earlier,
            cast,
        }
    }

    fn line(&self, key: RowKey) -> Option<&Line> {
        self.rows.get(*self.places.get(&key)?)
    }

    fn fold(&self, key: RowKey) -> Option<&Row> {
        self.line(key)
            .map(|line| &line.row)
            .filter(|row| matches!(row, Row::Fold { .. }))
    }

    pub(super) fn cast(&self) -> &[Who] {
        &self.cast
    }

    /// The row `key` is the last message of its run.
    pub(super) fn ends_run(&self, key: RowKey) -> bool {
        self.line(key).is_some_and(|line| line.last)
    }

    /// The words on a fold's button.
    fn heading(&self, key: RowKey) -> String {
        let Some(Row::Fold {
            tools,
            failed,
            running,
            helpers,
            open,
            steps,
            ..
        }) = self.fold(key)
        else {
            return String::new();
        };
        let plural = if *tools == 1 { "step" } else { "steps" };
        let mut text = match (*running, *tools) {
            (true, 0) => "Working".to_string(),
            (true, n) => format!("Working, {n} {plural} so far"),
            (false, 0) if !helpers.is_empty() => {
                "Asked for help".to_string()
            }
            (false, 0) => "Thought it over".to_string(),
            (false, n) => format!("Worked, {n} {plural}"),
        };
        if *failed > 0 {
            text.push_str(&format!(", {failed} failed"));
        }
        if !helpers.is_empty() {
            text.push_str(&format!(", with {}", helpers.join(", ")));
        }
        // Nothing to open in a fold of another agent's work alone.
        let mark = match (steps.is_empty(), *open) {
            (true, _) => " ",
            (false, true) => "-",
            (false, false) => "+",
        };
        format!("{mark}  {text}")
    }

    fn tone(&self, key: RowKey) -> Tone {
        match self.fold(key) {
            Some(Row::Fold { running: true, .. }) => Tone::Accent,
            Some(Row::Fold { failed: 0, .. }) | None => Tone::Dim,
            _ => Tone::Critical,
        }
    }

    /// The steps of a fold that is open, and the edits of one that
    /// is shut.
    fn shown(&self, key: RowKey) -> Vec<u64> {
        match self.fold(key) {
            Some(Row::Fold {
                steps, open: true, ..
            }) => steps.clone(),
            Some(Row::Fold { edits, .. }) => edits.clone(),
            _ => Vec::new(),
        }
    }
}

/// The whole chat in order, and the shapes it was put in order from.
#[derive(Default)]
pub struct Whole {
    /// The conversations in it, each with the shape of its
    /// transcript.
    from: Vec<(Entity, Part, usize, u64)>,
    /// The time the chief's own session began.
    since: u64,
    rows: Vec<Line>,
    cast: Vec<Who>,
}

/// Keeps [`Lines`] in step with the conversations, the open folds and
/// the reach of the chat.
#[allow(clippy::too_many_arguments)]
pub fn fold_lines(
    me: Res<Me>,
    unfolded: Res<Unfolded>,
    mut reach: ResMut<Reach>,
    sessions: Query<(Entity, &SessionKey, Option<&TalksTo>)>,
    transcripts: Query<Ref<Transcript>>,
    began: Query<&Began>,
    people: People,
    mut lines: ResMut<Lines>,
    mut whole: Local<Whole>,
) {
    // Another chat is built from its end again.
    if me.is_changed() && reach.rows != reach.page {
        reach.rows = reach.page;
    }
    // A reply that grows changes no shape, so its words cost a look
    // at each conversation and no more.
    let from = parts(&me, &sessions, &people)
        .into_iter()
        .map(|(session, part)| {
            let (entries, shape) = transcripts
                .get(session)
                .map_or((0, 0), |t| (t.items().len(), t.shape()));
            (session, part, entries, shape)
        })
        .collect::<Vec<_>>();
    let since = began.get(me.session).map_or(0, |began| began.0);
    let moved = me.is_changed()
        || unfolded.is_changed()
        || people.moved()
        || whole.from != from
        || whole.since != since;
    if !moved && !reach.is_changed() {
        return;
    }
    if moved {
        let parts = from
            .iter()
            .map(|(session, part, ..)| (*session, *part))
            .collect::<Vec<_>>();
        let (rows, cast) = timeline(
            &me,
            &parts,
            &sessions,
            &transcripts,
            &unfolded.0,
            &people,
            since,
        );
        *whole = Whole {
            from,
            since,
            rows,
            cast,
        };
    }
    let earlier = whole.rows.len().saturating_sub(reach.rows);
    let now = Lines::of(
        whole.rows[earlier..].to_vec(),
        earlier,
        whole.cast.clone(),
    );
    if *lines != now {
        *lines = now;
    }
}

/// Builds more of the chat when it is read back to where it begins,
/// and keeps what was being read where it was as the rows above it
/// come in.
pub fn read_back(
    me: Res<Me>,
    lines: Res<Lines>,
    mut reach: ResMut<Reach>,
    mut scrolls: Query<
        (&ComputedNode, &mut ScrollPosition, Option<&mut ScrollGoal>),
        With<ChatScroll>,
    >,
    mut held: Local<Option<(f32, f32)>>,
) {
    // The place held in one chat is not held in another.
    if me.is_changed() {
        *held = None;
    }
    for (node, mut at, goal) in &mut scrolls {
        let scale = node.inverse_scale_factor();
        let tall = node.content_size().y * scale;
        let most = super::furthest(node);
        if let Some((was, from)) = *held {
            if tall != was {
                at.y = from + (tall - was);
                // The wheel was headed for the top, which is now
                // further up: left alone it would go there and ask
                // for more again.
                if let Some(mut goal) = goal {
                    *goal = ScrollGoal::default();
                }
                *held = None;
            }
            continue;
        }
        if lines.earlier > 0 && most > 0.0 && at.y <= 1.0 {
            reach.rows += reach.page;
            *held = Some((tall, at.y));
        }
    }
}

/// The room between two rows of one run. A run is set further
/// from the one before it.
const GAP: f32 = 3.0;

/// The chat with whoever `Me` points at.
pub fn transcript() -> impl View {
    // One list for the whole chat, each row built once for as long
    // as it is the same row.
    let list = each(
        resource::<Lines, _>(|l| l.rows.clone()),
        |line: &Line| line.key(),
        |line| bubble::line_view(line).tagged(ChatRow).boxed(),
    )
    .within(
        column(())
            .gap(GAP)
            .width(percent(100.0))
            .min_width(px(LEAST))
            .max_width(px(LINE)),
    );
    let said = scroll((
        note(resource::<Lines, _>(|l| {
            match (l.rows.is_empty(), l.earlier) {
                (true, _) => EMPTY.to_string(),
                (false, 0) => String::new(),
                (false, n) => format!(
                    "{n} earlier lines. Scroll up to read them."
                ),
            }
        })),
        list,
    ))
    .fills_height()
    // The chat scrolls up and down and never sideways: what is in
    // it fits its width.
    .overflow(Overflow {
        x: OverflowAxis::Clip,
        y: OverflowAxis::Scroll,
    })
    .tagged(ChatScroll);
    column((bubble::header(), said)).fills_height().gap(6.0)
}

/// A row of the chat.
#[derive(Component)]
pub struct ChatRow;

/// Hides the rows of the chat that are scrolled out of sight. A
/// hidden row keeps its room, and one within a panel's height of the
/// edge stays shown.
pub fn hide_unseen(
    scrolls: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<ChatScroll>,
    >,
    mut rows: Query<
        (&ComputedNode, &UiGlobalTransform, &mut Visibility),
        With<ChatRow>,
    >,
) {
    let Some((pane, at)) = scrolls.iter().next() else {
        return;
    };
    let (middle, reach) = (at.translation.y, pane.size().y * 1.5);
    for (row, at, mut shown) in &mut rows {
        let seen = (at.translation.y - middle).abs()
            < reach + row.size().y * 0.5;
        let want = if seen {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *shown != want {
            *shown = want;
        }
    }
}

/// Lets a word that is wider than the conversation break inside
/// itself, so that a long path or address wraps like anything else.
pub fn break_long_words(
    scrolls: Query<(), With<ChatScroll>>,
    parents: Query<&ChildOf>,
    mut texts: Query<(Entity, &mut TextLayout), Changed<TextLayout>>,
) {
    for (text, mut layout) in &mut texts {
        if layout.linebreak == LineBreak::WordBoundary
            && parents
                .iter_ancestors(text)
                .any(|above| scrolls.contains(above))
        {
            layout.linebreak = LineBreak::WordOrCharacter;
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy_fynix::Theme;
    use hamlet_assets::Tuning;
    use serde_json::json;

    use super::*;
    use crate::me::MeOf;
    use crate::theme::HamletTheme;

    #[test]
    fn the_steps_of_a_turn_fold_into_one_row() {
        let call = |id: &str| {
            json!({"type": "assistant", "message": {"content": "", "tool_calls": [
                {"id": id, "function": {"name": "bash", "arguments": "{}"}}]}})
        };
        let transcript = Transcript::from_log(&[
            json!({"type": "user", "user": "a", "content": "go"}),
            call("1"),
            json!({"type": "tool", "exit_code": 0}),
            call("2"),
            json!({"type": "tool", "exit_code": 3}),
            json!({"type": "assistant", "message": {"content": "done"}}),
            json!({"type": "user", "user": "a", "content": "again"}),
            call("3"),
        ]);
        let ids = transcript.ids();
        let session = Entity::PLACEHOLDER;
        let folded = rows(session, &transcript, &[(session, ids[1])]);
        assert_eq!(folded.len(), 5);
        assert!(matches!(folded[0], Row::Entry { .. }));
        assert_eq!(
            folded[1],
            Row::Fold {
                session,
                first: ids[1],
                steps: vec![ids[1], ids[2]],
                edits: Vec::new(),
                tools: 2,
                failed: 1,
                running: false,
                helpers: Vec::new(),
                open: true,
            }
        );
        assert!(matches!(
            folded[4],
            Row::Fold {
                running: true,
                open: false,
                ..
            }
        ));
    }

    #[test]
    fn a_chat_is_every_conversation_of_its_agent_in_the_order_said() {
        use bevy::ecs::name::Name;
        use bevy::ecs::system::RunSystemOnce;
        use bevy::ecs::world::World;
        use hamlet_core::agents::{Citizen, Mayor};
        use hamlet_core::session::{ASKED, MAIL};

        let mut world = World::new();
        let juniper =
            world.spawn((Name::new("juniper"), Citizen(1))).id();
        let pip = world.spawn((Name::new("pip"), Citizen(2))).id();
        world.spawn((Name::new("bramble"), Citizen(0), Mayor));
        let said = |world: &mut World,
                    lines: &[(u64, &str, &str)]| {
            let log = lines
                .iter()
                .map(|(at, from, text)| match *from {
                    "" => json!({"type": "assistant", "ts": at,
                        "message": {"content": text}}),
                    from => json!({"type": "user", "ts": at,
                        "user": from, "content": text}),
                })
                .collect::<Vec<_>>();
            world.spawn(Transcript::from_log(&log)).id()
        };
        let session = said(
            &mut world,
            &[
                (1, "chief", "find the key"),
                (4, "", "I wrote to pip"),
                (7, "pip", "Reply from `pip`: by the door"),
            ],
        );
        let letter = said(
            &mut world,
            &[(2, "mayor", "plan?"), (3, "", "here")],
        );
        world.entity_mut(letter).insert((
            SessionKey::with_agent(MAIL, "mayor", "juniper"),
            TalksTo(juniper),
        ));
        // Pip's answer to a letter is shown where it came back.
        let wrote = said(
            &mut world,
            &[(5, "juniper", "key?"), (6, "", "by the door")],
        );
        world.entity_mut(wrote).insert((
            SessionKey::with_agent(MAIL, "juniper", "pip"),
            TalksTo(pip),
        ));
        let asked = said(
            &mut world,
            &[(8, "juniper", "sure?"), (9, "", "sure")],
        );
        world.entity_mut(asked).insert((
            SessionKey::with_agent(ASKED, "juniper", "pip"),
            TalksTo(pip),
        ));
        world.insert_resource(Me::of(
            session,
            juniper,
            "chief",
            Some("juniper"),
        ));
        world.insert_resource(Theme(HamletTheme::designed()));
        world.init_resource::<Unfolded>();
        world.init_resource::<Reach>();
        world.init_resource::<Lines>();
        world.run_system_once(fold_lines).unwrap();

        let lines = world.resource::<Lines>();
        let name = |who: usize| lines.cast[who].name.as_str();
        let order = lines
            .rows
            .iter()
            .map(|line| (name(line.by), name(line.to), line.at))
            .collect::<Vec<_>>();
        assert_eq!(
            order,
            [
                ("Chief", "Juniper", 1),
                ("Bramble", "Juniper", 2),
                ("Juniper", "Bramble", 3),
                ("Juniper", "Chief", 4),
                ("Juniper", "Pip", 5),
                ("Pip", "Juniper", 7),
                ("Juniper", "Pip", 8),
                ("Pip", "Juniper", 9),
            ]
        );

        // A new session starts the chat afresh: the chief's own
        // lines are all of that session, and of what the agents
        // wrote each other only what came since.
        world.entity_mut(session).insert(Began(6));
        world.run_system_once(fold_lines).unwrap();
        let lines = world.resource::<Lines>();
        let since =
            lines.rows.iter().map(|line| line.at).collect::<Vec<_>>();
        assert_eq!(since, [1, 4, 7, 8, 9]);
    }

    #[test]
    fn another_agents_work_is_named_and_not_shown() {
        use hamlet_core::agent::Event as AgentEvent;
        use hamlet_core::session::Reply;
        let mut transcript = Transcript::from_log(&[
            json!({"type": "user", "user": "a", "content": "go"}),
        ]);
        let by_scout = |event: AgentEvent| {
            Reply::Event(AgentEvent::Agent {
                name: "scout".into(),
                event: Box::new(event),
            })
        };
        transcript.apply(&by_scout(AgentEvent::Tool {
            name: "bash".into(),
            summary: "{}".into(),
            change: None,
        }));
        transcript.apply(&by_scout(AgentEvent::Text("found".into())));
        let session = Entity::PLACEHOLDER;
        let folded = rows(session, &transcript, &[]);
        let Row::Fold {
            steps,
            tools,
            helpers,
            running,
            ..
        } = &folded[1]
        else {
            panic!("a fold after the first line");
        };
        assert!(steps.is_empty());
        assert_eq!((*tools, *running), (0, false));
        assert_eq!(helpers, &["scout".to_string()]);
        let line = |row: &Row| Line {
            row: row.clone(),
            owner: session,
            by: 0,
            to: 0,
            at: 0,
            look: 0,
            lead: false,
            named: false,
            last: false,
        };
        let lines = Lines::of(
            folded.iter().map(line).collect(),
            0,
            Vec::new(),
        );
        let first = transcript.ids()[1];
        assert_eq!(
            lines.heading(RowKey::Fold(session, session, first)),
            "   Asked for help, with scout"
        );
    }
}
