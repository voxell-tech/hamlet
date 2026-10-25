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
    AlignItems, ComputedNode, JustifyContent, Overflow, OverflowAxis,
    ScrollPosition, UiGlobalTransform, UiRect, percent, px,
};
use bevy_fynix::scroll::ScrollGoal;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, ghost,
    label, overlay, scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, each, resource};
use hamlet_client::chats::{self, Entry, Transcript};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_protocol::client as protocol;

use super::entry::entry_view;
use super::timeline::{Line, People, Who, timeline};
use super::{AwayFromEnd, ChatScroll, bubble};
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
                self.agent,
                *session,
                id.clone(),
                self.look,
            ),
            Row::Fold { session, first, .. } => {
                RowKey::Fold(self.agent, *session, first.clone())
            }
        }
    }
}

/// The part of a chat that is built: a window of a few pages, which
/// starts at the end and slides as the chat is read back and forth.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct Reach {
    /// The rows built, and the rows each turn of the page moves.
    pub rows: usize,
    pub page: usize,
    /// The rows after the window, which are not built.
    pub back: usize,
    /// What was sent is shown: the window goes to the end of the
    /// chat and the scroll follows it. Cleared once the rows are
    /// folded.
    pub jump: bool,
}

impl Reach {
    /// The pages the window grows to before it slides.
    const PAGES: usize = 3;

    fn most(&self) -> usize {
        self.page * Self::PAGES
    }
}

impl Default for Reach {
    fn default() -> Self {
        Self {
            rows: 6,
            page: 6,
            back: 0,
            jump: false,
        }
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
    /// The rows after these that are not built.
    pub(super) later: usize,
    more: bool,
    /// Everyone who speaks in the chat, the agent it is with first.
    cast: Vec<Who>,
}

impl Lines {
    fn of(
        rows: Vec<Line>,
        earlier: usize,
        later: usize,
        cast: Vec<Who>,
    ) -> Self {
        let places = rows
            .iter()
            .enumerate()
            .map(|(at, line)| (line.key(), at))
            .collect();
        Lines {
            rows,
            places,
            earlier,
            later,
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
    pub(super) fn is_run_end(&self, key: &RowKey) -> bool {
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
    /// The last row of the window while it is read back, and the
    /// rows after it then.
    anchor: Option<(RowKey, usize)>,
}

/// The visible rows of the received chat.
pub fn fold_lines(
    me: Res<Me>,
    unfolded: Res<Unfolded>,
    mut reach: ResMut<Reach>,
    transcripts: Query<Ref<Transcript>>,
    people: People,
    mut lines: ResMut<Lines>,
    mut whole: Local<Whole>,
) {
    if reach.jump {
        reach.jump = false;
    }
    if me.is_changed() && (reach.rows != reach.page || reach.back > 0)
    {
        reach.rows = reach.page;
        reach.back = 0;
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
        || people.has_moved()
        || whole.from != from;
    if !moved && !reach.is_changed() {
        return;
    }
    if moved {
        let (rows, cast) =
            timeline(&me, &transcript, &unfolded.0, &people);
        let anchor = whole.anchor.take();
        *whole = Whole {
            from,
            rows,
            cast,
            anchor,
        };
        // Rows added after the window, or before it, do not move
        // what is being read: it stays at the row it ended at.
        if let Some((key, back)) = whole.anchor.as_ref()
            && reach.back == *back
            && let Some(at) =
                whole.rows.iter().rposition(|line| line.key() == *key)
        {
            let back = whole.rows.len() - 1 - at;
            if reach.back != back {
                reach.back = back;
            }
        }
    }
    let end = whole.rows.len().saturating_sub(reach.back);
    let earlier = end.saturating_sub(reach.rows);
    let mut now = Lines::of(
        whole.rows[earlier..end].to_vec(),
        earlier,
        whole.rows.len() - end,
        whole.cast.clone(),
    );
    now.more |= transcript.earlier().is_some();
    whole.anchor = (reach.back > 0)
        .then(|| now.rows.last().map(|line| (line.key(), reach.back)))
        .flatten();
    if *lines != now {
        *lines = now;
    }
}

/// The scroll anchor and the earlier page being received.
#[derive(Resource, Default)]
pub struct Reading {
    held: Option<(f32, f32, u8)>,
    page: Option<(protocol::RequestId, protocol::ChatRef)>,
    at_top: bool,
}

/// The row held in place while the window slides: where it is below
/// the top of the chat, and how tall the chat was.
pub struct Turn {
    row: RowKey,
    below: f32,
    tall: f32,
    frames: u8,
}

/// The frames a turn waits for the rows to be laid out.
const WAIT: u8 = 30;

/// Slides the window of built rows when the chat is read to its top
/// or its bottom, and asks for the earlier rows of the chat when the
/// top is the chat's first row built. The row at the edge being read
/// stays where it was.
pub fn read_back(
    mut commands: Commands,
    me: Res<Me>,
    lines: Res<Lines>,
    transcripts: Query<&Transcript>,
    mut reach: ResMut<Reach>,
    mut scrolls: Query<
        (
            &ComputedNode,
            &UiGlobalTransform,
            &mut ScrollPosition,
            Option<&mut ScrollGoal>,
        ),
        With<ChatScroll>,
    >,
    rows: Query<(&ChatRow, &ComputedNode, &UiGlobalTransform)>,
    mut reading: ResMut<Reading>,
    mut turning: Local<Option<Turn>>,
) {
    if me.is_changed() {
        *reading = Reading::default();
        *turning = None;
    }
    // The window is on its way to the end, where nothing is turned.
    if reach.jump {
        *turning = None;
        return;
    }
    for (node, place, mut at, goal) in &mut scrolls {
        let tall =
            node.content_size().y * node.inverse_scale_factor();
        let top = place.translation.y - node.size().y * 0.5;
        let furthest = super::furthest(node).max(0.0);
        if furthest == 0.0 || at.y > 1.0 {
            reading.at_top = false;
        }
        if reading.page.is_some() {
            continue;
        }
        if let Some(turn) = turning.as_mut() {
            turn.frames += 1;
            let found =
                rows.iter().find(|(row, ..)| row.0 == turn.row);
            if (tall != turn.tall || turn.frames > WAIT)
                && let Some((_, row, place)) = found
            {
                let below =
                    place.translation.y - row.size().y * 0.5 - top;
                at.y = (at.y
                    + (below - turn.below)
                        * node.inverse_scale_factor())
                .clamp(0.0, furthest);
                // The wheel was headed for an edge that is now
                // further off: left alone it would go there and ask
                // for more again.
                if let Some(mut goal) = goal {
                    *goal = ScrollGoal::default();
                }
                *turning = None;
            } else if turn.frames > WAIT {
                *turning = None;
            }
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
        if node.size().y <= 0.0 {
            continue;
        }
        let at_top = furthest == 0.0 || at.y <= 1.0;
        let at_bottom = lines.later > 0
            && furthest > 0.0
            && at.y >= furthest - 1.0;
        if !at_top && !at_bottom {
            continue;
        }
        if at_bottom || lines.earlier > 0 {
            // The row at the edge being read stays through the turn.
            let edge = rows
                .iter()
                .map(|(row, node, place)| {
                    let y = place.translation.y;
                    (row.0.clone(), y - node.size().y * 0.5 - top, y)
                })
                .reduce(
                    |a, b| {
                        if (a.2 < b.2) == at_top { a } else { b }
                    },
                );
            let Some((row, below, _)) = edge else {
                continue;
            };
            *turning = Some(Turn {
                row,
                below,
                tall,
                frames: 0,
            });
            if at_bottom {
                reach.back = reach.back.saturating_sub(reach.page);
            } else if reach.rows < reach.most() {
                reach.rows += reach.page;
            } else {
                reach.back += reach.page;
            }
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
        |line| {
            bubble::line_view(line)
                .tagged(ChatRow(line.key()))
                .boxed()
        },
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
        note(resource::<Lines, _>(|l| match l.later {
            0 => String::new(),
            n => {
                format!("{n} later lines. Scroll down to read them.")
            }
        })),
    ))
    .fills_height()
    // The chat scrolls up and down and never sideways: what is in
    // it fits its width.
    .overflow(Overflow {
        x: OverflowAxis::Clip,
        y: OverflowAxis::Scroll,
    })
    .tagged(ChatScroll);
    // The way back to the end, over the corner of the chat, offered
    // only when the chat is not at its end.
    let back = each(
        resource::<AwayFromEnd, _>(|away| {
            if away.0 { vec![()] } else { Vec::new() }
        }),
        |_: &()| 0u8,
        |_| {
            button(crate::looks::raised(crate::icons::glyph(
                crate::icons::Glyph::ToEnd,
                crate::looks::ICON,
            )))
            .padding(UiRect::all(px(8.0)))
            .tooltip(|| note("Scroll to the latest"))
            .rules(crate::looks::retrace)
            .tagged(crate::feel::Mood::Ink)
            .on_activate(super::to_end)
            .boxed()
        },
    )
    .within(column(()).padding(UiRect::all(px(12.0))));
    let corner = overlay((back,))
        .justify(JustifyContent::FlexEnd)
        .align(AlignItems::FlexEnd);
    column((bubble::header(), column((said, corner)).fills_height()))
        .fills_height()
        .gap(6.0)
}

/// A row of the chat.
#[derive(Component)]
pub struct ChatRow(RowKey);

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
