//! The conversation as rows: each entry, the steps of a turn folded
//! into one, and only the rows near the view built.

use std::collections::HashMap;

use bevy::app::App;
use bevy::camera::visibility::Visibility;
use bevy::ecs::change_detection::{
    DetectChanges, DetectChangesMut, Ref,
};
use bevy::ecs::component::Component;
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
    frame, label, overlay, row, virtual_list,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, each, every_frame, keyed, resource,
};
use hamlet_client::chats::{self, Entry, Transcript};
use hamlet_client::messages::Response;
use hamlet_client::realm::Kind;
use hamlet_protocol::client as protocol;
use hamlet_realm::hand::FileDrop;
use hamlet_ui::styles::{
    Fills, Looks, Tipped, lettered, note, small_rules, text_button,
};
use hamlet_ui::widgets::edges::faded;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::shimmer::shimmer;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::entry::entry_view;
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

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Reading>()
        .init_resource::<MergedLines>()
        .add_observer(page_received);
}

/// What can be said first to `agent`, by what it is.
fn starters(world: &World, agent: Entity) -> &'static [&'static str] {
    match world.get::<Kind>(agent).map(|kind| kind.0) {
        Some(protocol::AgentKind::Hand) => HAND_STARTERS,
        Some(protocol::AgentKind::Worker) => WORKER_STARTERS,
        _ => HIRE_STARTERS,
    }
}

/// The frames a list put back where its chat was left is given to
/// land before where it is counts as where it was left.
const LANDING: u8 = 3;

/// Where the chief left a chat, kept on the chat.
#[derive(Component, Clone)]
pub(super) enum Place {
    /// At its end, which it follows as rows come.
    End,
    /// With this row the first that shows.
    Row(RowKey),
}

/// The conversation the chat's list shows.
#[derive(Component)]
pub struct Shows {
    chat: Entity,
    /// The frames left before where the list is counts as where the
    /// chat was left.
    landing: u8,
}

/// Keeps where the chat's list is as where its conversation was left.
pub(super) fn remember_place(
    lines: Res<Lines>,
    mut places: Query<&mut Place>,
    mut lists: Query<(&VirtualList, &mut Shows), With<ChatScroll>>,
    mut commands: Commands,
) {
    for (list, mut shows) in &mut lists {
        if shows.landing > 0 {
            shows.landing -= 1;
            continue;
        }
        if list.count() != lines.rows.len() {
            continue;
        }
        let place = match list.is_following() {
            true => Place::End,
            false => match lines.rows.get(list.first_visible()) {
                Some(line) => Place::Row(line.key()),
                None => continue,
            },
        };
        match places.get_mut(shows.chat) {
            Ok(mut kept) => *kept = place,
            Err(_) => {
                commands.entity(shows.chat).try_insert(place);
            }
        }
    }
}

/// Puts the chat's list where its conversation was left, or at its
/// end, when it is built or shows another conversation. A list is
/// built hidden and shown once it is put there.
pub(super) fn restore_place(
    mut commands: Commands,
    convo: Res<Convo>,
    lines: Res<Lines>,
    places: Query<&Place>,
    mut lists: Query<
        (
            Entity,
            &mut VirtualList,
            &mut Visibility,
            Option<&mut Shows>,
        ),
        With<ChatScroll>,
    >,
) {
    for (list, mut state, mut seen, shows) in &mut lists {
        if shows
            .as_ref()
            .is_some_and(|shows| shows.chat == convo.chat)
        {
            continue;
        }
        seen.set_if_neq(Visibility::Inherited);
        // Every row is of another conversation, whose heights are not
        // known.
        if shows.is_some() {
            let count = state.count();
            state.splice(0, count, lines.rows.len());
        }
        let row = match places.get(convo.chat) {
            Ok(Place::Row(key)) => lines.places.get(key).copied(),
            Ok(Place::End) | Err(_) => None,
        };
        // The last row first is the end, as far as the list scrolls.
        let last = state.count().saturating_sub(1);
        state.scroll_to(row.unwrap_or(last));
        let now = Shows {
            chat: convo.chat,
            landing: LANDING,
        };
        match shows {
            Some(mut shows) => *shows = now,
            None => {
                commands.entity(list).insert(now);
            }
        }
    }
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
        chat: Entity,
        id: String,
    },
    /// An entry drawn as a block of its own that opens in place: the
    /// summary of the earlier messages, a conversation merged in, or
    /// a worker started.
    Card {
        chat: Entity,
        id: String,
    },
    /// The steps of one turn under a header, open while it runs.
    Fold {
        chat: Entity,
        /// The entry the turn is known by: the one before it, else
        /// its first step.
        first: String,
        /// The steps shown when it is open.
        steps: Vec<String>,
        failed: usize,
        /// When its first step began and its last ended, as far as
        /// the steps tell, in milliseconds since 1970.
        start: u64,
        end: u64,
        running: bool,
        /// Its model reasons now.
        thinking: bool,
        open: bool,
    },
    /// The files a turn that ended changed, after its reply.
    Changes {
        chat: Entity,
        /// The entry its turn is known by.
        first: String,
        /// The edits that were applied, in order.
        edits: Vec<String>,
        /// When its turn ended.
        at: u64,
    },
}

/// Whether `entry` is drawn as a [`Row::Card`].
fn is_card(entry: &Entry) -> bool {
    match entry {
        Entry::Summary(_)
        | Entry::Merged { .. }
        | Entry::Event(_) => true,
        Entry::Chief { .. }
        | Entry::Assistant(_)
        | Entry::Reasoning(_)
        | Entry::Tool { .. }
        | Entry::Error(_)
        | Entry::Notice(_)
        | Entry::Answered(_)
        | Entry::Agent { .. } => false,
    }
}

/// One turn of the agent while its steps are gathered.
struct Turn {
    first: String,
    steps: Vec<String>,
    edits: Vec<String>,
    failed: usize,
    start: u64,
    end: u64,
    /// The last step is words the agent wrote, which are its reply
    /// unless a step follows.
    replied: bool,
    reasons: bool,
}

impl Turn {
    fn new(first: String, at: u64) -> Self {
        Turn {
            first,
            steps: Vec::new(),
            edits: Vec::new(),
            failed: 0,
            start: at,
            end: at,
            replied: false,
            reasons: false,
        }
    }

    fn take(&mut self, item: &chats::Item) {
        if self.steps.is_empty() && item.at > 0 {
            self.start = item.at;
        }
        let ms = match &item.entry {
            Entry::Tool {
                exit, ms, change, ..
            } => {
                self.failed += exit.is_some_and(|c| c != 0) as usize;
                if change.is_some() && *exit == Some(0) {
                    self.edits.push(item.id.clone());
                }
                ms.unwrap_or(0)
            }
            _ => 0,
        };
        self.end = self.end.max(item.at + ms);
        self.steps.push(item.id.clone());
        self.replied = matches!(item.entry, Entry::Assistant(_));
        self.reasons = matches!(item.entry, Entry::Reasoning(_));
    }

    /// Its rows: the steps under their header, then its reply.
    fn close(
        mut self,
        chat: Entity,
        running: bool,
        open: &[(Entity, String)],
        rows: &mut Vec<Row>,
    ) {
        let reply = match self.replied {
            true => self.steps.pop(),
            false => None,
        };
        if running || !self.steps.is_empty() {
            rows.push(Row::Fold {
                chat,
                open: running
                    || open.contains(&(chat, self.first.clone())),
                first: self.first.clone(),
                steps: self.steps,
                failed: self.failed,
                start: self.start,
                end: self.end,
                running,
                thinking: running && self.reasons,
            });
        }
        if let Some(id) = reply {
            rows.push(Row::Entry { chat, id });
        }
        if !running && !self.edits.is_empty() {
            rows.push(Row::Changes {
                chat,
                first: self.first,
                edits: self.edits,
                at: self.end,
            });
        }
    }
}

/// One conversation with the steps of each turn folded under its
/// header, and its reply after them.
pub(super) fn rows(
    chat: Entity,
    transcript: &Transcript,
    open: &[(Entity, String)],
) -> Vec<Row> {
    let live = transcript.is_answering();
    let mut rows = Vec::<Row>::new();
    let mut turn = None::<Turn>;
    // The entry a turn that begins now is known by, and whether a
    // turn has followed it.
    let mut before = None::<&chats::Item>;
    let mut answered = false;
    // The turn that runs ends where the queue begins.
    let mut queue = false;
    // The fault the last row says, if it says one: the same fault
    // again, as each try that fails the same way raises, stands in
    // its place, so a run of them is one row, its last.
    let mut fault: Option<&str> = None;
    let end = |turn: Option<Turn>,
               before: Option<&chats::Item>,
               answered: bool,
               rows: &mut Vec<Row>| {
        match turn {
            Some(turn) => turn.close(chat, live, open, rows),
            // A turn that has shown nothing yet, known by what it
            // answers.
            None if live && !answered => {
                if let Some(before) = before {
                    Turn::new(before.id.clone(), before.at)
                        .close(chat, true, open, rows);
                }
            }
            None => {}
        }
    };
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
            // row nor parts the steps before it from those after.
            Entry::Assistant(text) if text.trim().is_empty() => {
                continue;
            }
            Entry::Tool { .. }
            | Entry::Reasoning(_)
            | Entry::Assistant(_) => true,
            // A notice often follows a turn's reply, and stands
            // after it.
            Entry::Notice(_)
            | Entry::Chief { .. }
            | Entry::Error(_)
            | Entry::Answered(_)
            | Entry::Agent { .. }
            | Entry::Summary(_)
            | Entry::Merged { .. }
            | Entry::Event(_) => false,
        };
        if step {
            let first = before.unwrap_or(item);
            turn.get_or_insert_with(|| {
                Turn::new(first.id.clone(), first.at)
            })
            .take(item);
            answered = true;
            continue;
        }
        if item.queued && !queue {
            queue = true;
            end(turn.take(), before, answered, &mut rows);
        } else if let Some(done) = turn.take() {
            done.close(chat, false, open, &mut rows);
        }
        let id = item.id.clone();
        rows.push(match is_card(&item.entry) {
            true => Row::Card { chat, id },
            false => Row::Entry { chat, id },
        });
        if !item.queued {
            (before, answered) = (Some(item), false);
        }
    }
    if !queue {
        end(turn.take(), before, answered, &mut rows);
    }
    rows
}

/// How far in from a fold's edge its chevron is, as a bubble's words
/// are from its edge.
pub(super) const INSET: f32 = 12.0;

/// `ms` as a person reads a span of time: `13s`, `2m 49s`, `1h 4m`.
pub(super) fn lasted(ms: u64) -> String {
    let seconds = ms / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds / 60 % 60),
    }
}

/// The steps of a turn under its header, as board `B` holds them:
/// how long it has worked, and the steps while it runs or when it is
/// opened.
pub(super) fn fold_view<B: Board>(
    key: RowKey,
    chat: Entity,
    first: String,
) -> impl View {
    // The row is built once for its fold. Its header and what is
    // inside read the fold as it stands, so a new step or its end
    // builds neither the row nor its button again.
    let running_key = key.clone();
    let head_key = key.clone();
    let head = keyed(
        resource::<B, _>(move |b| b.lines().is_running(&running_key)),
        move |running| match running {
            true => working::<B>(head_key.clone()).boxed(),
            false => {
                worked::<B>(head_key.clone(), chat, first.clone())
                    .boxed()
            }
        },
    )
    .within(row(()).width(percent(100.0)));
    let inside = each(
        resource::<B, _>(move |b| b.lines().shown(&key)),
        |id: &String| id.clone(),
        move |id| entry_view(chat, id.clone()),
    )
    // As wide as the chat and no wider, so that what is inside is
    // measured at the width it is shown at.
    .within(column(()).width(percent(100.0)).min_width(px(0.0)));
    with_cx(move |cx| {
        let theme = cx.theme();
        let rule = frame()
            .height(px(theme.space.border))
            .width(percent(100.0))
            .fill(theme.palette.gold_6);
        column((head, rule, inside))
            .gap(theme.space.gap / 2.0)
            .align(AlignItems::FlexStart)
            .width(percent(100.0))
            .min_width(px(0.0))
    })
}

/// The header of a turn that runs: "Working for 13s", or "Thinking"
/// while its model reasons, the word shimmering.
fn working<B: Board>(key: RowKey) -> impl View {
    let thinking_key = key.clone();
    let word = keyed(
        resource::<B, _>(move |b| {
            b.lines().is_thinking(&thinking_key)
        }),
        |thinking| {
            shimmer(if *thinking { "Thinking" } else { "Working" })
                .rules(small_rules)
                .boxed()
        },
    )
    .within(row(()));
    let since = derived(move |world: &World| {
        let start = world.resource::<B>().lines().start(&key);
        match start {
            Some(start) if start > 0 => format!(
                " for {}",
                lasted(crate::status::now_ms().saturating_sub(start))
            ),
            _ => String::new(),
        }
    })
    .when(every_frame());
    row((word, label(since).wrap(false).tone(Tone::Dim)))
        .padding(UiRect::axes(px(INSET), px(2.0)))
        .align(AlignItems::Center)
        .rules(small_rules)
}

/// The header of a turn that ended, which opens to its steps:
/// "Worked for 2m 49s", and how many of them failed.
fn worked<B: Board>(
    key: RowKey,
    chat: Entity,
    first: String,
) -> impl View {
    let (turn_key, failed_key) = (key.clone(), key.clone());
    let failed = keyed(
        resource::<B, _>(move |b| b.lines().failed(&failed_key)),
        |failed| match failed {
            0 => frame().boxed(),
            n => label(format!(" · {n} failed"))
                .wrap(false)
                .tone(Tone::Critical)
                .rules(small_rules)
                .boxed(),
        },
    )
    .within(row(()));
    let words = row((
        glyph(Glyph::Chevron, 10.0).rotation(resource::<B, _>(
            move |b| {
                if b.lines().is_open(&turn_key) {
                    180.0
                } else {
                    90.0
                }
            },
        )),
        row((
            label(resource::<B, _>(move |b| b.lines().heading(&key)))
                .wrap(false)
                .rules(small_rules),
            failed,
        )),
    ))
    .gap(6.0)
    .align(AlignItems::Center);
    // Words alone, its chevron in line with the words of the rows.
    button(words)
        .padding(UiRect::axes(px(INSET), px(2.0)))
        .rules(text_button)
        .on_activate(move |world| {
            let unfolded = &mut world.resource_mut::<Unfolded>().0;
            let fold = (chat, first.clone());
            match unfolded.iter().position(|open| *open == fold) {
                Some(at) => {
                    unfolded.remove(at);
                }
                None => unfolded.push(fold),
            }
        })
}

/// The identity of a row for as long as it is the same row: the agent
/// whose chat it is in, its conversation, and its entry with the look
/// of its speaker, or the first step of its fold.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(super) enum RowKey {
    Entry(Entity, Entity, String, u64),
    Fold(Entity, Entity, String),
    Changes(Entity, Entity, String),
}

impl Line {
    pub(super) fn key(&self) -> RowKey {
        match &self.row {
            Row::Entry { chat, id } | Row::Card { chat, id } => {
                RowKey::Entry(
                    self.agent,
                    *chat,
                    id.clone(),
                    self.look,
                )
            }
            Row::Fold { chat, first, .. } => {
                RowKey::Fold(self.agent, *chat, first.clone())
            }
            Row::Changes { chat, first, .. } => {
                RowKey::Changes(self.agent, *chat, first.clone())
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

    /// The words on the header of a turn that ended.
    fn heading(&self, key: &RowKey) -> String {
        match self.fold(key) {
            Some(Row::Fold { start, end, .. })
                if *end >= start + 1000 =>
            {
                format!("Worked for {}", lasted(end - start))
            }
            _ => "Worked".to_string(),
        }
    }

    fn is_open(&self, key: &RowKey) -> bool {
        matches!(self.fold(key), Some(Row::Fold { open: true, .. }))
    }

    fn is_running(&self, key: &RowKey) -> bool {
        matches!(
            self.fold(key),
            Some(Row::Fold { running: true, .. })
        )
    }

    fn is_thinking(&self, key: &RowKey) -> bool {
        matches!(
            self.fold(key),
            Some(Row::Fold { thinking: true, .. })
        )
    }

    /// When a turn began.
    fn start(&self, key: &RowKey) -> Option<u64> {
        match self.fold(key) {
            Some(Row::Fold { start, .. }) => Some(*start),
            _ => None,
        }
    }

    fn failed(&self, key: &RowKey) -> usize {
        match self.fold(key) {
            Some(Row::Fold { failed, .. }) => *failed,
            _ => 0,
        }
    }

    /// The steps of a fold that is open.
    fn shown(&self, key: &RowKey) -> Vec<String> {
        match self.fold(key) {
            Some(Row::Fold {
                steps, open: true, ..
            }) => steps.clone(),
            _ => Vec::new(),
        }
    }
}

/// Rows of a merged conversation added each time more of it is asked
/// for.
const MERGED_PAGE: usize = 40;

/// The conversation of a hire merged into the chat's agent, open in
/// place under the row that says so and drawn as the chat is.
#[derive(Resource, Default)]
pub struct MergedLines {
    /// The row it is open under, by its conversation and entry.
    under: Option<(Entity, String)>,
    /// The chat watched for it, and whether it was watched for this
    /// alone.
    holder: Option<(Entity, bool)>,
    /// The hire merged in, and its name.
    agent: Option<(Entity, String)>,
    /// Rows built, counted from the end.
    reach: usize,
    lines: Lines,
}

impl Board for MergedLines {
    fn lines(&self) -> &Lines {
        &self.lines
    }
}

impl MergedLines {
    /// Rows before those built, here or still on the server.
    pub(crate) fn has_earlier(&self) -> bool {
        self.lines.more
    }

    /// Whether it is open under a row of `chat`, or watched in it.
    pub(super) fn is_in(&self, chat: Entity) -> bool {
        self.under.as_ref().is_some_and(|(of, _)| *of == chat)
            || self.holder.is_some_and(|(holder, _)| holder == chat)
    }

    /// Whether the row `id` of `chat` is the one open.
    pub(super) fn is_open(&self, chat: Entity, id: &str) -> bool {
        self.under
            .as_ref()
            .is_some_and(|(of, entry)| *of == chat && entry == id)
    }
}

/// Opens the conversation of the hire `from`, named `name`, under the
/// row `id` of `chat`, in place of any other that is open.
pub(crate) fn open_merged(
    world: &mut World,
    chat: Entity,
    id: String,
    from: protocol::AgentId,
    name: String,
) {
    close_merged(world);
    let reference = protocol::ChatRef { with: from };
    let holder = match chats::find(world, &reference) {
        Some(kept) => (kept, false),
        None => (world.spawn(chats::Chat(reference)).id(), true),
    };
    let agent = hamlet_client::realm::agent(world, from)
        .unwrap_or(Entity::PLACEHOLDER);
    *world.resource_mut::<MergedLines>() = MergedLines {
        under: Some((chat, id)),
        holder: Some(holder),
        agent: Some((agent, name)),
        reach: MERGED_PAGE,
        lines: Lines::default(),
    };
}

/// Shuts the merged conversation that is open, and stops watching it
/// if it was watched for this alone.
pub(crate) fn close_merged(world: &mut World) {
    let merged =
        std::mem::take(&mut *world.resource_mut::<MergedLines>());
    // The holder may be the chat that is gone.
    if let Some((holder, true)) = merged.holder
        && let Ok(holder) = world.get_entity_mut(holder)
    {
        holder.despawn();
    }
}

/// Builds a page more of the merged conversation, and asks the server
/// for its earlier rows once every row it has is built.
pub(crate) fn merged_earlier(world: &mut World) {
    let mut merged = world.resource_mut::<MergedLines>();
    let Some((holder, _)) = merged.holder else {
        return;
    };
    let all_built = merged.lines.earlier == 0;
    merged.reach += MERGED_PAGE;
    if all_built {
        chats::earlier(world, holder);
    }
}

/// The rows of the merged conversation that is open.
pub fn fold_merged(
    convo: Res<Convo>,
    unfolded: Res<Unfolded>,
    transcripts: Query<Ref<Transcript>>,
    people: People,
    mut merged: ResMut<MergedLines>,
) {
    let holder = merged.holder.map(|(holder, _)| holder);
    let said = holder.and_then(|at| transcripts.get(at).ok());
    let moved = merged.is_changed()
        || convo.is_changed()
        || unfolded.is_changed()
        || people.has_moved()
        || said.as_ref().is_some_and(DetectChanges::is_changed);
    let (Some(holder), Some(said), Some((agent, name)), true) =
        (holder, said, merged.agent.clone(), moved)
    else {
        return;
    };
    let (rows, cast) = timeline(
        &convo,
        (agent, Some(name.as_str())),
        holder,
        &said,
        &unfolded.0,
        &people,
    );
    let earlier = rows.len().saturating_sub(merged.reach);
    let mut now = Lines::of(rows[earlier..].to_vec(), earlier, cast);
    now.more |= said.earlier().is_some();
    if merged.lines != now {
        merged.lines = now;
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
    let Ok(transcript) = transcripts.get(convo.chat) else {
        return;
    };
    let now_from = Some((
        convo.chat,
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
        (convo.agent, convo.with.as_deref()),
        convo.chat,
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
            .get(convo.chat)
            .is_ok_and(|said| said.earlier().is_some())
        {
            reading.at_top = true;
            let chat = convo.chat;
            commands.queue(move |world: &mut World| {
                let Some(reference) = world
                    .get::<chats::Chat>(chat)
                    .map(|chat| chat.0.clone())
                else {
                    return;
                };
                let id = chats::earlier(world, chat);
                // A page refused is said under the field.
                if let Some(id) = &id {
                    super::refused(world, id.clone());
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
    .tagged(FileDrop)
    // Its rows are placed at its top in the update it is built, and
    // where its chat was left only in the next.
    .tagged(Visibility::Hidden);
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
            let own = !super::without_model(world);
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

/// The merged conversation's rows, drawn as the chat's are.
pub(super) fn merged_rows() -> impl View {
    each(
        resource::<MergedLines, _>(|m| m.lines.rows.clone()),
        |line: &Line| line.key(),
        bubble::line_view::<MergedLines>,
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
