//! The conversation as rows: each entry, the steps of a turn folded
//! into one, and only the rows near the view built.

use std::collections::HashMap;

use bevy::app::App;
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::text::{Justify, LineBreak, TextLayout};
use bevy::ui::{
    AlignItems, ComputedNode, JustifyContent, ScrollPosition, UiRect,
    percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, RowSlot, VirtualList, button, column,
    frame, ghost, label, overlay, row, virtual_list,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, each, every_frame, keyed, resource,
};
use hamlet_client::chats::{self, Entry, Transcript};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::Kind;
use hamlet_protocol::client as protocol;
use hamlet_realm::hand::FileDrop;
use hamlet_ui::styles::{
    Fills, LINE, Looks, Tipped, lettered, note, small_rules,
};
use hamlet_ui::widgets::edges::faded;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::entry::{edit_view, entry_view};
use super::timeline::{Line, People, Who, timeline};
use super::{AwayFromEnd, ChatScroll, bubble};
use crate::convo::Convo;

/// The least width the conversation is laid out at. A panel
/// narrower than this cuts it off.
const LEAST: f32 = 240.0;

/// The height of a row of the chat before it is laid out.
const ROW_ESTIMATE: f32 = 60.0;

/// What can be said first to the Hand, which sets things up.
const HAND_STARTERS: &[&str] = &[
    "What can you do for me here?",
    "Help me work on a project folder.",
    "Set up a hamlet for a new project.",
];

/// What can be said first to a hire, which works on its hamlet's
/// folder.
const HIRE_STARTERS: &[&str] = &[
    "What are you working on?",
    "Look through the folder and tell me what you see.",
    "What do you remember so far?",
];

/// What can be said first to a worker, which has one task.
const WORKER_STARTERS: &[&str] =
    &["How far along are you?", "What have you found so far?"];

/// What can be said first to `agent`, by what it is.
fn starters(world: &World, agent: Entity) -> &'static [&'static str] {
    match world.get::<Kind>(agent).map(|kind| kind.0) {
        Some(protocol::AgentKind::Hand) => HAND_STARTERS,
        Some(protocol::AgentKind::Worker) => WORKER_STARTERS,
        _ => HIRE_STARTERS,
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Reading>()
        .init_resource::<PastLines>()
        .add_observer(page_received);
}

fn page_received(
    response: On<Response>,
    mut reading: ResMut<Reading>,
    mut requests: ResMut<super::Requests>,
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
    if matches!(
        response.message,
        protocol::ServerMessage::Transcript(_)
            | protocol::ServerMessage::Reply(_)
            | protocol::ServerMessage::Fault(_)
    ) {
        reading.page = None;
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
    // Whether the last row is a fold that opened its turn, and
    // whether the last entry was a reply.
    let (mut opening, mut replied) = (false, false);
    // The fault the last row says, if it says one: the same fault
    // again, as each try that fails the same way raises, stands in
    // its place, so a run of them is one row, its last.
    let mut fault: Option<&str> = None;
    for item in transcript.items() {
        if let Entry::Error(said) = &item.entry {
            if fault == Some(said.as_str()) {
                rows.pop();
            }
            fault = Some(said.as_str());
        } else if !matches!(
            &item.entry,
            Entry::Assistant(text) if text.trim().is_empty()
        ) {
            // A reply that says nothing makes no row to part them.
            fault = None;
        }
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
            let reply = matches!(item.entry, Entry::Assistant(_));
            let row = Row::Entry {
                session,
                id: item.id.clone(),
            };
            // Steps stand under the reply before them, and those
            // that open a turn under its first reply.
            if reply && opening {
                rows.insert(rows.len() - 1, row);
            } else {
                rows.push(row);
            }
            (opening, replied) = (false, reply);
            continue;
        };
        if !matches!(rows.last(), Some(Row::Fold { .. })) {
            opening = !replied;
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

/// How far in from a fold's edge the line down from its chevron is,
/// and how far from that line its steps are.
const GUIDE: f32 = 4.0;

/// How far in from a fold's edge its chevron is, as a bubble's words
/// are from its edge.
pub(super) const INSET: f32 = 12.0;

/// The steps of a turn as one line that opens, as board `B` holds
/// them.
pub(super) fn fold_view<B: Board>(
    key: RowKey,
    session: Entity,
    first: String,
) -> impl View {
    // The row is built once for its fold. Its heading and what is
    // inside read the fold as it stands, so opening it or a new step
    // builds neither the row nor its button again.
    let heading_key = key.clone();
    let tone_key = key.clone();
    let turn_key = key.clone();
    let words = row((
        glyph(Glyph::Chevron, 10.0).tone(Tone::Dim).rotation(
            resource::<B, _>(move |b| {
                if b.lines().is_open(&turn_key) {
                    180.0
                } else {
                    90.0
                }
            }),
        ),
        label(resource::<B, _>(move |b| {
            b.lines().heading(&heading_key)
        }))
        .tone(resource::<B, _>(move |b| b.lines().tone(&tone_key)))
        .rules(small_rules),
    ))
    .gap(6.0)
    .align(AlignItems::Center);
    // Unframed, so it reads as a note on the reply, its chevron in
    // line with the bubbles' words.
    let head = with_cx(move |cx| {
        button(words)
            .padding(UiRect::axes(px(INSET), px(2.0)))
            .radius(cx.theme().space.round)
            .rules(ghost)
            .on_activate(move |world| {
                let unfolded =
                    &mut world.resource_mut::<Unfolded>().0;
                let fold = (session, first.clone());
                match unfolded.iter().position(|open| *open == fold) {
                    Some(at) => {
                        unfolded.remove(at);
                    }
                    None => unfolded.push(fold),
                }
            })
    });
    let kept_key = key.clone();
    let inside = each(
        resource::<B, _>(move |b| b.lines().shown(&key)),
        |id: &String| id.clone(),
        move |id| entry_view(session, id.clone()),
    )
    // As wide as the chat and no wider, so that what is inside is
    // measured at the width it is shown at.
    .within(
        column(()).gap(6.0).width(percent(100.0)).min_width(px(0.0)),
    );
    // A line down from the chevron holds what is shown inside, under
    // its heading.
    let inside = with_cx(move |cx| {
        let guide = frame()
            .width(px(LINE))
            .shrink(0.0)
            .margin(UiRect::left(px(GUIDE)))
            .fill(cx.theme().palette.gold_6);
        row((guide, inside))
            .gap(GUIDE)
            .align(AlignItems::Stretch)
            .width(percent(100.0))
            .min_width(px(0.0))
    });
    let kept = each(
        resource::<B, _>(move |b| b.lines().kept(&kept_key)),
        |id: &String| id.clone(),
        move |id| edit_view(session, id.clone()),
    )
    .within(
        column(()).gap(6.0).width(percent(100.0)).min_width(px(0.0)),
    );
    column((head, inside, kept))
        .gap(4.0)
        .align(AlignItems::FlexStart)
        .width(percent(100.0))
        .min_width(px(0.0))
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

/// The rows of the chat, kept in step with the conversations and the
/// open folds.
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

/// A resource that holds the rows of a conversation as the chat
/// draws them.
pub(crate) trait Board: Resource {
    fn lines(&self) -> &Lines;
}

impl Board for Lines {
    fn lines(&self) -> &Lines {
        self
    }
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

    /// The words on a fold's button.
    fn heading(&self, key: &RowKey) -> String {
        let Some(Row::Fold {
            tools,
            failed,
            running,
            helpers,
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
        text
    }

    /// Whether a fold is open. A fold of edits alone is open until it
    /// is shut, as its edits are shown under a shut fold of more.
    fn is_open(&self, key: &RowKey) -> bool {
        match self.fold(key) {
            Some(Row::Fold {
                steps, edits, open, ..
            }) => *open != (steps.len() == edits.len()),
            _ => false,
        }
    }

    /// Whether a fold holds steps besides its edits.
    fn has_more(&self, key: &RowKey) -> bool {
        matches!(
            self.fold(key),
            Some(Row::Fold { steps, edits, .. })
                if steps.len() > edits.len()
        )
    }

    fn tone(&self, key: &RowKey) -> Tone {
        match self.fold(key) {
            Some(Row::Fold { running: true, .. }) => Tone::Warning,
            Some(Row::Fold { failed: 0, .. }) | None => Tone::Dim,
            _ => Tone::Critical,
        }
    }

    /// The steps of a fold that is open.
    fn shown(&self, key: &RowKey) -> Vec<String> {
        match self.fold(key) {
            Some(Row::Fold { steps, .. })
                if self.has_more(key) && self.is_open(key) =>
            {
                steps.clone()
            }
            _ => Vec::new(),
        }
    }

    /// The edits that stand under a fold's heading as their own
    /// cards: those of a shut fold of more, or of an open fold of
    /// edits alone.
    fn kept(&self, key: &RowKey) -> Vec<String> {
        match self.fold(key) {
            Some(Row::Fold { edits, .. })
                if self.has_more(key) != self.is_open(key) =>
            {
                edits.clone()
            }
            _ => Vec::new(),
        }
    }
}

/// Rows of a past session added each time more of it is asked for.
const PAST_PAGE: usize = 40;

/// The past session read in the Hub, drawn as the chat is: the entity
/// with its transcript, and its last rows.
#[derive(Resource, Default)]
pub struct PastLines {
    holder: Option<Entity>,
    /// Rows built, counted from the end.
    reach: usize,
    lines: Lines,
}

impl Board for PastLines {
    fn lines(&self) -> &Lines {
        &self.lines
    }
}

impl PastLines {
    /// Rows before those built, here or still on the server.
    pub(crate) fn has_earlier(&self) -> bool {
        self.lines.more
    }
}

/// Reads the transcript held by `holder` as the past session, or
/// none.
pub(crate) fn read_past(world: &mut World, holder: Option<Entity>) {
    *world.resource_mut::<PastLines>() = PastLines {
        holder,
        reach: PAST_PAGE,
        lines: Lines::default(),
    };
}

/// Builds a page more of the past session, and asks the server for
/// its earlier rows once every row it has is built.
pub(crate) fn read_earlier(world: &mut World) {
    let mut past = world.resource_mut::<PastLines>();
    let Some(holder) = past.holder else {
        return;
    };
    let all_built = past.lines.earlier == 0;
    past.reach += PAST_PAGE;
    if all_built {
        chats::earlier(world, holder);
    }
}

/// The rows of the past session.
pub fn fold_past(
    convo: Res<Convo>,
    unfolded: Res<Unfolded>,
    transcripts: Query<Ref<Transcript>>,
    people: People,
    mut past: ResMut<PastLines>,
) {
    let said = past.holder.and_then(|at| transcripts.get(at).ok());
    let moved = past.is_changed()
        || convo.is_changed()
        || unfolded.is_changed()
        || people.has_moved()
        || said.as_ref().is_some_and(DetectChanges::is_changed);
    let (Some(holder), Some(said), true) = (past.holder, said, moved)
    else {
        return;
    };
    let (rows, cast) =
        timeline(&convo, holder, &said, &unfolded.0, &people);
    let earlier = rows.len().saturating_sub(past.reach);
    let mut now = Lines::of(rows[earlier..].to_vec(), earlier, cast);
    now.more |= said.earlier().is_some();
    if past.lines != now {
        past.lines = now;
    }
}

/// The rows of the received chat. The list is told which rows
/// changed, so the rows around them keep their place on screen.
pub fn fold_lines(
    convo: Res<Convo>,
    unfolded: Res<Unfolded>,
    transcripts: Query<Ref<Transcript>>,
    people: People,
    mut lines: ResMut<Lines>,
    mut lists: Query<&mut VirtualList, With<ChatScroll>>,
    mut from: Local<Option<(Entity, usize, u64)>>,
) {
    let Ok(transcript) = transcripts.get(convo.session) else {
        return;
    };
    let now_from = Some((
        convo.session,
        transcript.items().len(),
        transcript.shape(),
    ));
    if !convo.is_changed()
        && !unfolded.is_changed()
        && !people.has_moved()
        && *from == now_from
    {
        return;
    }
    *from = now_from;
    let (rows, cast) = timeline(
        &convo,
        convo.session,
        &transcript,
        &unfolded.0,
        &people,
    );
    let mut now = Lines::of(rows, 0, cast);
    now.more |= transcript.earlier().is_some();
    if *lines == now {
        return;
    }
    for mut list in &mut lists {
        splice(&mut list, &lines.rows, &now.rows);
    }
    *lines = now;
}

/// Tells `list` that the rows between the start and the end `was` and
/// `now` share changed.
fn splice(list: &mut VirtualList, was: &[Line], now: &[Line]) {
    // A list built since the rows last changed already counts them.
    if list.count() != was.len() {
        return;
    }
    let same = |(a, b): &(&Line, &Line)| a.key() == b.key();
    let head = was.iter().zip(now).take_while(same).count();
    let tail = was[head..]
        .iter()
        .rev()
        .zip(now[head..].iter().rev())
        .take_while(same)
        .count();
    let (removed, inserted) =
        (was.len() - head - tail, now.len() - head - tail);
    if removed > 0 || inserted > 0 {
        list.splice(head, removed, inserted);
    }
}

/// The earlier page being received.
#[derive(Resource, Default)]
pub struct Reading {
    page: Option<(protocol::RequestId, protocol::ChatRef)>,
    at_top: bool,
}

/// Asks for the earlier rows of the chat once it is read to its top,
/// or while all of it fits.
pub fn ask_earlier(
    mut commands: Commands,
    convo: Res<Convo>,
    transcripts: Query<&Transcript>,
    scrolls: Query<
        (&ComputedNode, &ScrollPosition),
        With<ChatScroll>,
    >,
    mut reading: ResMut<Reading>,
) {
    if convo.is_changed() {
        *reading = Reading::default();
    }
    for (node, at) in &scrolls {
        let furthest = super::furthest(node);
        // Once the top is left, or while there is no top to leave,
        // reaching it asks again.
        if furthest == 0.0 || at.y > 1.0 {
            reading.at_top = false;
        }
        let at_top = furthest == 0.0 || at.y <= 1.0;
        if reading.page.is_some()
            || reading.at_top
            || !at_top
            || node.size().y <= 0.0
        {
            continue;
        }
        if transcripts
            .get(convo.session)
            .is_ok_and(|said| said.earlier().is_some())
        {
            reading.at_top = true;
            let chat = convo.session;
            commands.queue(move |world: &mut World| {
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
            });
        }
    }
}

/// The room between two rows of one run. A run is set further
/// from the one before it.
const GAP: f32 = 2.0;

/// The chat with whoever `Convo` points at.
pub fn transcript() -> impl View {
    // Only the rows near the view are built, and the view stays at
    // the end as the chat grows until it is scrolled up.
    let said = virtual_list(
        resource::<Lines, _>(|l| l.rows.len()),
        ROW_ESTIMATE,
        chat_row,
    )
    .follow_bottom(true)
    .tagged(ChatScroll)
    .tagged(FileDrop);
    // The way back to the end, over the corner of the chat, offered
    // only when the chat is not at its end.
    let back = each(
        resource::<AwayFromEnd, _>(|away| {
            if away.0 { vec![()] } else { Vec::new() }
        }),
        |_: &()| 0u8,
        |_| {
            button(hamlet_ui::styles::raised(glyph(
                Glyph::ToEnd,
                hamlet_ui::styles::ICON,
            )))
            .padding(UiRect::all(px(8.0)))
            .variant(hamlet_ui::widgets::effects::Variant::Text)
            .tip("Scroll to the latest")
            .on_activate(super::to_end)
            .boxed()
        },
    )
    .within(column(()).padding(UiRect::all(px(12.0))));
    let corner = overlay((back,))
        .justify(JustifyContent::FlexEnd)
        .align(AlignItems::FlexEnd);
    column((faded(said), quiet(), corner)).fills_height()
}

/// The window's words over a chat nothing has been said in: whom to
/// say hello to and what could be said first, which a press puts in
/// the field.
fn quiet() -> impl View {
    let empty = each(
        derived(|world: &World| {
            let lines = world.resource::<Lines>();
            let convo = world.resource::<Convo>();
            // No starter is offered while nothing could answer it.
            let own = world
                .get::<chats::Chat>(convo.session)
                .is_some_and(|chat| chat.0.from.is_none())
                && !super::without_model(world);
            match (
                lines.rows.is_empty() && !lines.more,
                lines.cast.first(),
            ) {
                (true, Some(agent)) => vec![(
                    agent.name.clone(),
                    own,
                    starters(world, convo.agent),
                )],
                _ => Vec::new(),
            }
        })
        .when(every_frame()),
        |shown: &(String, bool, &'static [&'static str])| {
            shown.clone()
        },
        |(name, own, starters)| {
            let (name, own, starters) =
                (name.clone(), *own, *starters);
            with_cx(move |cx| {
                let theme = cx.theme();
                let starters = starters
                    .iter()
                    .filter(|_| own)
                    .map(|said| {
                        lettered(*said)
                            .rules(hamlet_ui::styles::button)
                            .on_activate(move |world| {
                                super::field::set(
                                    world,
                                    vec![super::Piece::Text(
                                        said.to_string(),
                                    )],
                                );
                            })
                            .boxed()
                    })
                    .collect::<Vec<_>>();
                column((
                    label(format!("Say hello to {name}"))
                        .size(theme.text.heading)
                        .bold(true),
                    column(starters)
                        .gap(theme.space.gap)
                        .align(AlignItems::Center),
                    // Centred line by line, as it wraps under the
                    // rest.
                    note(
                        "Agents can make mistakes, so check \
                         anything important.",
                    )
                    .tagged(TextLayout::justify(Justify::Center)),
                ))
                .gap(theme.space.section)
                .align(AlignItems::Center)
                .padding(UiRect::all(px(theme.space.section)))
            })
            .boxed()
        },
    );
    overlay((empty,))
        .justify(JustifyContent::Center)
        .align(AlignItems::Center)
}

/// The column the rows of a conversation stand in.
fn rows_column() -> impl View {
    column(())
        .gap(GAP)
        .width(percent(100.0))
        .min_width(px(LEAST))
}

/// The past session's rows, drawn as the chat's are.
pub(crate) fn past() -> impl View {
    each(
        resource::<PastLines, _>(|p| p.lines.rows.clone()),
        |line: &Line| line.key(),
        bubble::line_view::<PastLines>,
    )
    .within(rows_column())
}

/// The row of the chat that `slot` shows, built again only when the
/// slot shows another row.
fn chat_row(slot: RowSlot) -> AnyView {
    let index = slot.index();
    let mut moved = slot.index();
    let mut changed = resource::<Lines, _>(|_| ());
    let key = derived(move |world: &World| {
        let lines = world.resource::<Lines>();
        lines.rows.get(index.get(world)).map(Line::key)
    })
    .when(move |world: &World| {
        // Both checks run, as each takes in what it saw.
        moved.changed(world) | changed.changed(world)
    });
    keyed(key, |key: &Option<RowKey>| {
        let key = key.clone();
        AnyView::new(move |cx: &mut Cx| {
            let line = key
                .as_ref()
                .and_then(|key| {
                    cx.world.resource::<Lines>().line(key)
                })
                .cloned();
            match line {
                Some(line) => cx.build(
                    column((bubble::line_view::<Lines>(&line),))
                        .padding(UiRect::top(px(GAP)))
                        .width(percent(100.0))
                        .min_width(px(LEAST)),
                ),
                None => cx.spawn(),
            }
        })
    })
    .boxed()
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
