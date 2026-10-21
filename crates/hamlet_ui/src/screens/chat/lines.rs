//! The conversation as rows: each entry, the steps of a turn folded
//! into one, and only the end of a long chat built.

use std::collections::HashMap;

use bevy::app::App;
use bevy::camera::visibility::Visibility;
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
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
use hamlet_client::chats::{self, Entry, Transcript};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_protocol as protocol;

use super::entry::entry_view;
use super::timeline::{Line, People, Who, timeline};
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

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Reading>().add_observer(page_received);
}

fn page_received(
    response: On<Response>,
    mut reading: ResMut<Reading>,
    lines: Res<Lines>,
    mut reach: ResMut<Reach>,
    mut requests: ResMut<super::Requests>,
    scrolls: Query<
        (&ComputedNode, &ScrollPosition),
        With<ChatScroll>,
    >,
) {
    let Some((id, reference)) = &reading.page else {
        return;
    };
    if id != &response.to {
        if matches!(&response.message, protocol::ServerMessage::Transcript(page) if page.chat == *reference)
        {
            requests.0.remove(id);
            *reading = Reading::default();
        }
        return;
    }
    match &response.message {
        protocol::ServerMessage::Transcript(_) => {
            reach.rows = lines.rows.len() + reach.page;
            reading.held = scrolls.iter().next().map(|(node, at)| {
                (
                    node.content_size().y
                        * node.inverse_scale_factor(),
                    at.y,
                    2,
                )
            });
            reading.page = None;
        }
        protocol::ServerMessage::Reply(_)
        | protocol::ServerMessage::Fault(_) => reading.page = None,
        _ => {}
    }
}

/// Folds that are open, by their conversation and first entry.
#[derive(Resource, Default)]
pub struct Unfolded(pub Vec<(Entity, String)>);

/// A row of a conversation: one entry, or the steps of a turn
/// folded into one.
#[derive(Clone, PartialEq, Debug)]
pub(super) enum Row {
    Entry {
        session: Entity,
        id: String,
    },
    Fold {
        session: Entity,
        /// The first entry folded in, shown or not.
        first: String,
        /// The steps shown when it is open.
        steps: Vec<String>,
        /// The edits among them, whose changes are shown when it is
        /// shut too.
        edits: Vec<String>,
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
    open: &[(Entity, String)],
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
                id: item.id.clone(),
            });
            continue;
        };
        if !matches!(rows.last(), Some(Row::Fold { .. })) {
            rows.push(Row::Fold {
                session,
                first: item.id.clone(),
                steps: Vec::new(),
                edits: Vec::new(),
                tools: 0,
                failed: 0,
                running: false,
                helpers: Vec::new(),
                open: open.contains(&(session, item.id.clone())),
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
        *running |= exit.is_none();
        if let Entry::Agent { name, .. } = &item.entry
            && !helpers.contains(name)
        {
            helpers.push(name.clone());
        }
        steps.push(item.id.clone());
        if matches!(
            item.entry,
            Entry::Tool {
                change: Some(_),
                ..
            }
        ) {
            edits.push(item.id.clone());
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
    first: String,
) -> impl View {
    // The row is built once for its fold. Its heading and what is
    // inside read the fold as it stands, so opening it or a new step
    // builds neither the row nor its button again.
    let heading_key = key.clone();
    let tone_key = key.clone();
    let head = button(
        label(resource::<Lines, _>(move |l| l.heading(&heading_key)))
            .size(12.0)
            .tone(resource::<Lines, _>(move |l| l.tone(&tone_key))),
    )
    .rules(ghost)
    .on_activate(move |world| {
        let unfolded = &mut world.resource_mut::<Unfolded>().0;
        let fold = (session, first.clone());
        match unfolded.iter().position(|open| *open == fold) {
            Some(at) => {
                unfolded.remove(at);
            }
            None => unfolded.push(fold),
        }
    });
    let inside = each(
        resource::<Lines, _>(move |l| l.shown(&key)),
        |id: &String| id.clone(),
        move |id| entry_view(session, id.clone()),
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
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(super) enum RowKey {
    Entry(Entity, Entity, String, u64),
    Fold(Entity, Entity, String),
}

impl Line {
    pub(super) fn key(&self) -> RowKey {
        match &self.row {
            Row::Entry { session, id } => RowKey::Entry(
                self.owner,
                *session,
                id.clone(),
                self.look,
            ),
            Row::Fold { session, first, .. } => {
                RowKey::Fold(self.owner, *session, first.clone())
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
    more: bool,
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
            more: earlier > 0,
            cast,
        }
    }

    fn line(&self, key: &RowKey) -> Option<&Line> {
        self.rows.get(*self.places.get(key)?)
    }

    fn fold(&self, key: &RowKey) -> Option<&Row> {
        self.line(key)
            .map(|line| &line.row)
            .filter(|row| matches!(row, Row::Fold { .. }))
    }

    pub(super) fn cast(&self) -> &[Who] {
        &self.cast
    }

    /// The row `key` is the last message of its run.
    pub(super) fn ends_run(&self, key: &RowKey) -> bool {
        self.line(key).is_some_and(|line| line.last)
    }

    /// The words on a fold's button.
    fn heading(&self, key: &RowKey) -> String {
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

    fn tone(&self, key: &RowKey) -> Tone {
        match self.fold(key) {
            Some(Row::Fold { running: true, .. }) => Tone::Accent,
            Some(Row::Fold { failed: 0, .. }) | None => Tone::Dim,
            _ => Tone::Critical,
        }
    }

    /// The steps of a fold that is open, and the edits of one that
    /// is shut.
    fn shown(&self, key: &RowKey) -> Vec<String> {
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
    from: Option<(Entity, usize, u64)>,
    rows: Vec<Line>,
    cast: Vec<Who>,
}

/// The visible rows of the received chat.
#[allow(clippy::too_many_arguments)]
pub fn fold_lines(
    me: Res<Me>,
    unfolded: Res<Unfolded>,
    mut reach: ResMut<Reach>,
    transcripts: Query<Ref<Transcript>>,
    people: People,
    mut lines: ResMut<Lines>,
    mut whole: Local<Whole>,
) {
    if me.is_changed() && reach.rows != reach.page {
        reach.rows = reach.page;
    }
    let Ok(transcript) = transcripts.get(me.session) else {
        return;
    };
    let from = Some((
        me.session,
        transcript.items().len(),
        transcript.shape(),
    ));
    let moved = me.is_changed()
        || unfolded.is_changed()
        || people.moved()
        || whole.from != from;
    if !moved && !reach.is_changed() {
        return;
    }
    if moved {
        let (rows, cast) =
            timeline(&me, &transcript, &unfolded.0, &people);
        *whole = Whole { from, rows, cast };
    }
    let earlier = whole.rows.len().saturating_sub(reach.rows);
    let mut now = Lines::of(
        whole.rows[earlier..].to_vec(),
        earlier,
        whole.cast.clone(),
    );
    now.more |= transcript.earlier().is_some();
    if *lines != now {
        *lines = now;
    }
}

/// The scroll anchor and the earlier page being received.
#[derive(Resource, Default)]
pub struct Reading {
    held: Option<(f32, f32, u8)>,
    page: Option<(String, protocol::ChatRef)>,
    at_top: bool,
}

/// The earlier rows built or requested at the top, with the current
/// rows held in place.
#[allow(clippy::too_many_arguments)]
pub fn read_back(
    mut commands: Commands,
    me: Res<Me>,
    lines: Res<Lines>,
    transcripts: Query<&Transcript>,
    mut reach: ResMut<Reach>,
    mut scrolls: Query<
        (&ComputedNode, &mut ScrollPosition, Option<&mut ScrollGoal>),
        With<ChatScroll>,
    >,
    mut reading: ResMut<Reading>,
) {
    if me.is_changed() {
        *reading = Reading::default();
    }
    for (node, mut at, goal) in &mut scrolls {
        let tall =
            node.content_size().y * node.inverse_scale_factor();
        let furthest = super::furthest(node).max(0.0);
        if furthest == 0.0 || at.y > 1.0 {
            reading.at_top = false;
        }
        if reading.page.is_some() {
            continue;
        }
        if let Some((was, from, left)) = reading.held {
            if tall != was {
                at.y = (from + (tall - was)).clamp(0.0, furthest);
                if let Some(mut goal) = goal {
                    *goal = ScrollGoal::default();
                }
                reading.held = None;
            } else {
                // A page can join a fold without adding any height.
                reading.held = (left > 0).then_some((
                    was,
                    from,
                    left.saturating_sub(1),
                ));
            }
            continue;
        }
        if node.size().y <= 0.0 || furthest > 0.0 && at.y > 1.0 {
            continue;
        }
        if lines.earlier > 0 {
            reach.rows += reach.page;
            reading.held = Some((tall, at.y, 2));
        } else if !reading.at_top
            && transcripts
                .get(me.session)
                .is_ok_and(|said| said.earlier().is_some())
        {
            reading.at_top = true;
            let chat = me.session;
            commands.queue(
                move |world: &mut bevy::ecs::world::World| {
                    let Some(reference) = world
                        .get::<chats::Chat>(chat)
                        .map(|chat| chat.0.clone())
                    else {
                        return;
                    };
                    chats::earlier(world, chat);
                    let id = world
                        .resource::<Outbox>()
                        .iter()
                        .filter_map(|message| match message {
                            protocol::ClientMessage::Open(open)
                                if open.chat == reference
                                    && open.before.is_some() =>
                            {
                                Some(open.id.clone())
                            }
                            _ => None,
                        })
                        .last();
                    if let Some(id) = &id {
                        world
                            .resource_mut::<super::Requests>()
                            .0
                            .insert(id.clone());
                    }
                    world.resource_mut::<Reading>().page =
                        id.map(|id| (id, reference));
                },
            );
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
            if l.more {
                "Earlier lines. Scroll up to read them.".to_string()
            } else if l.rows.is_empty() {
                EMPTY.to_string()
            } else {
                String::new()
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
