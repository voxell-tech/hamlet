//! The conversation as rows, each turn's steps folded into one.

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
    AlignItems, ComputedNode, JustifyContent, Overflow,
    ScrollPosition, UiRect, Val, percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, RowSlot, VirtualList, button, column,
    frame, label, overlay, row, virtual_list,
};
use bevy_fynix::{
    ScopedExt, Signal, ViewExt, component, derived, each,
    every_frame, keyed,
};
use hamlet_client::chats::{self, Entry, Transcript};
use hamlet_client::messages::Response;
use hamlet_client::realm::Kind;
use hamlet_protocol::client as protocol;
use hamlet_realm::hand::FileDrop;
use hamlet_ui::styles::{
    Fills, Looks, Tipped, lettered, note, quiet_text_button,
    small_rules,
};
use hamlet_ui::widgets::edges::faded;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::shimmer::shimmer;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::entry::{Fold, entry_view};
use super::timeline::{Line, People, Who, timeline};
use super::{AwayFromEnd, ChatScroll, bubble};
use crate::convo::Convo;

/// The least width the conversation is laid out at.
const LEAST: f32 = 240.0;

/// The height of a row of the chat before it is laid out.
const ROW_ESTIMATE: f32 = 60.0;

/// What can be said first to the Hand.
const HAND_STARTERS: &[&str] = &[
    "What can you do for me here?",
    "Help me work on a project folder.",
    "Set up a hamlet for a new project.",
];

/// What can be said first to an agent of a hamlet.
const AGENT_STARTERS: &[&str] = &[
    "What are you working on?",
    "Look through the folder and tell me what you see.",
    "What do you remember so far?",
];

/// What can be said first to a worker.
const WORKER_STARTERS: &[&str] =
    &["How far along are you?", "What have you found so far?"];

pub(super) fn plugin(app: &mut App) {
    app.add_observer(page_received);
}

fn starters(world: &World, agent: Entity) -> &'static [&'static str] {
    match world.get::<Kind>(agent).map(|kind| kind.0) {
        Some(protocol::AgentKind::Hand) => HAND_STARTERS,
        Some(protocol::AgentKind::Worker) => WORKER_STARTERS,
        _ => AGENT_STARTERS,
    }
}

/// The frames a restored list settles for before its place is kept.
const LANDING: u8 = 3;

/// Where the chief left a chat.
#[derive(Clone)]
pub(super) enum Place {
    End,
    /// With this row the first shown.
    Row(RowKey),
}

/// Where the chief left each chat a Hub showed.
#[derive(Component, Default)]
pub(super) struct ChatPlaces(pub(super) HashMap<Entity, Place>);

/// The conversation the chat's list shows.
#[derive(Component)]
pub struct Shows {
    chat: Entity,
    /// The frames left before its place is kept.
    landing: u8,
}

/// Keeps where each Hub's chat list is as where its conversation was
/// left.
pub(super) fn remember_place(
    mut hubs: Query<(&Lines, &mut ChatPlaces)>,
    mut lists: Query<(&ChatScroll, &VirtualList, &mut Shows)>,
) {
    for (scroll, list, mut shows) in &mut lists {
        let Ok((lines, mut places)) = hubs.get_mut(scroll.0) else {
            continue;
        };
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
        places.0.insert(shows.chat, place);
    }
}

/// Puts each Hub's chat list where its conversation was left.
pub(super) fn restore_place(
    mut commands: Commands,
    hubs: Query<(&Convo, &Lines, &ChatPlaces)>,
    mut lists: Query<(
        Entity,
        &ChatScroll,
        &mut VirtualList,
        &mut Visibility,
        Option<&mut Shows>,
    )>,
) {
    for (list, scroll, mut state, mut seen, shows) in &mut lists {
        let Ok((convo, lines, places)) = hubs.get(scroll.0) else {
            continue;
        };
        if shows
            .as_ref()
            .is_some_and(|shows| shows.chat == convo.chat)
        {
            continue;
        }
        seen.set_if_neq(Visibility::Inherited);
        // The rows' heights are another conversation's.
        if shows.is_some() {
            let count = state.count();
            state.splice(0, count, lines.rows.len());
        }
        let row = match places.0.get(&convo.chat) {
            Some(Place::Row(key)) => lines.places.get(key).copied(),
            Some(Place::End) | None => None,
        };
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
    mut readings: Query<&mut Reading>,
    mut requests: ResMut<super::Requests>,
) {
    for mut reading in &mut readings {
        let Some((id, reference)) = &reading.page else {
            continue;
        };
        if id != &response.to {
            if matches!(&response.message, protocol::ServerMessage::Transcript(page) if page.chat == *reference)
            {
                requests.0.remove(id);
                *reading = Reading::default();
            }
            continue;
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
}

/// The turns that are open, by their Hub, conversation and first
/// entry.
#[derive(Resource, Default)]
pub struct Unfolded(pub Vec<Fold>);

impl Unfolded {
    /// The turns open in `hub`, by their conversation and first
    /// entry.
    fn of(&self, hub: Entity) -> Vec<(Entity, String)> {
        self.0
            .iter()
            .filter(|(of, ..)| *of == hub)
            .map(|(_, chat, first)| (*chat, first.clone()))
            .collect()
    }
}

/// A row of a conversation.
#[derive(Clone, PartialEq, Debug)]
pub(super) enum Row {
    Entry {
        chat: Entity,
        id: String,
    },
    /// An entry drawn as a card that opens in place.
    Card {
        chat: Entity,
        id: String,
    },
    /// The steps of one turn under a header.
    Fold {
        chat: Entity,
        /// The entry the turn is known by: the one before it, else
        /// its first step.
        first: String,
        steps: Vec<String>,
        failed: usize,
        /// In milliseconds since 1970.
        start: u64,
        end: u64,
        running: bool,
        thinking: bool,
        open: bool,
    },
    /// The files a turn that ended changed.
    Changes {
        chat: Entity,
        first: String,
        edits: Vec<String>,
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
    /// The place of `first` in the transcript.
    from: usize,
    /// Its steps, with the lines that came while it ran among them.
    steps: Vec<String>,
    /// How many of `steps` after its last step are such lines.
    trail: usize,
    stepped: bool,
    edits: Vec<String>,
    failed: usize,
    start: u64,
    end: u64,
    /// The last step is the agent's words, its reply unless a step
    /// follows.
    replied: bool,
    reasons: bool,
}

impl Turn {
    fn new(first: &chats::Item, from: usize) -> Self {
        Turn {
            first: first.id.clone(),
            from,
            steps: Vec::new(),
            trail: 0,
            stepped: false,
            edits: Vec::new(),
            failed: 0,
            start: first.at,
            end: first.at,
            replied: false,
            reasons: false,
        }
    }

    /// Takes a line between turns that came while it ran.
    fn hold(&mut self, item: &chats::Item) {
        self.steps.push(item.id.clone());
        self.trail += 1;
    }

    fn take(&mut self, item: &chats::Item) {
        if !self.stepped && item.at > 0 {
            self.start = item.at;
        }
        self.stepped = true;
        self.trail = 0;
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
        let after = match running {
            true => Vec::new(),
            false => {
                self.steps.split_off(self.steps.len() - self.trail)
            }
        };
        let reply = match self.replied && !running {
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
        rows.extend(
            after.into_iter().map(|id| Row::Card { chat, id }),
        );
    }
}

/// What an entry is to the turns.
#[derive(Clone, Copy)]
enum Part {
    Step,
    /// What the agent answers, which begins a turn.
    Input,
    /// A line between turns, held by a turn that goes on after it.
    Line,
    /// A reply that failed, which ends its turn.
    Fault,
}

/// A conversation's rows, each turn's steps under its header.
pub(super) fn rows(
    chat: Entity,
    transcript: &Transcript,
    open: &[(Entity, String)],
) -> Vec<Row> {
    let items = transcript.items();
    // A turn from before the answer began is not the one under way.
    let begun = transcript
        .began_after()
        .and_then(|id| items.iter().position(|item| item.id == id));
    let end = |turn: Option<Turn>, rows: &mut Vec<Row>| {
        if let Some(turn) = turn {
            let running = transcript.is_answering()
                && begun.is_none_or(|at| turn.from >= at);
            turn.close(chat, running, open, rows);
        }
    };
    let mut rows = Vec::<Row>::new();
    let mut turn = None::<Turn>;
    // The entry a turn that begins with a step is known by.
    let mut before = None::<(usize, &chats::Item)>;
    let mut queue = false;
    // A run of the same fault is one row, its last.
    let mut fault: Option<&str> = None;
    for (at, item) in items.iter().enumerate() {
        if let Entry::Error(said) = &item.entry {
            if fault == Some(said.as_str()) {
                rows.pop();
            }
            fault = Some(said.as_str());
        } else if !matches!(
            &item.entry,
            Entry::Assistant(text) if text.trim().is_empty()
        ) {
            fault = None;
        }
        let part = match &item.entry {
            // Empty words beside a tool call are no row.
            Entry::Assistant(text) if text.trim().is_empty() => {
                continue;
            }
            Entry::Tool { .. }
            | Entry::Reasoning(_)
            | Entry::Assistant(_) => Part::Step,
            Entry::Chief { .. }
            | Entry::Agent { .. }
            | Entry::Answered(_)
            | Entry::Notice(_) => Part::Input,
            Entry::Summary(_)
            | Entry::Merged { .. }
            | Entry::Event(_) => Part::Line,
            Entry::Error(_) => Part::Fault,
        };
        if item.queued {
            if !queue {
                queue = true;
                end(turn.take(), &mut rows);
            }
        } else {
            match (part, &mut turn) {
                (Part::Step, _) => {
                    let (from, first) = before.unwrap_or((at, item));
                    turn.get_or_insert_with(|| {
                        Turn::new(first, from)
                    })
                    .take(item);
                    continue;
                }
                (Part::Line, Some(going)) => {
                    going.hold(item);
                    continue;
                }
                (Part::Line | Part::Input | Part::Fault, _) => {
                    if let Some(done) = turn.take() {
                        done.close(chat, false, open, &mut rows);
                    }
                }
            }
        }
        let id = item.id.clone();
        rows.push(match is_card(&item.entry) {
            true => Row::Card { chat, id },
            false => Row::Entry { chat, id },
        });
        if !item.queued {
            before = Some((at, item));
            if matches!(part, Part::Input) {
                turn = Some(Turn::new(item, at));
            }
        }
    }
    if !queue {
        end(turn.take(), &mut rows);
    }
    rows
}

/// How far in from a fold's edge its chevron is.
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

/// What `read` gives of the rows `hub`'s board `B` holds.
fn board<B: Board, T: Default + Send + Sync + 'static>(
    hub: Entity,
    read: impl Fn(&Lines) -> T + Send + Sync + 'static,
) -> Signal<T> {
    component::<B, _>(hub, move |board| {
        board.map(|board| read(board.lines())).unwrap_or_default()
    })
}

/// The steps of a turn under its header, as `hub`'s board `B` holds
/// them.
pub(super) fn fold_view<B: Board>(
    hub: Entity,
    key: RowKey,
    chat: Entity,
    first: String,
) -> impl View {
    // Built once per fold; a new step rebuilds neither row nor
    // header.
    let running_key = key.clone();
    let head_key = key.clone();
    let head = keyed(
        board::<B, _>(hub, move |l| l.is_running(&running_key)),
        move |running| match running {
            true => working::<B>(hub, head_key.clone()).boxed(),
            false => worked::<B>(
                hub,
                head_key.clone(),
                chat,
                first.clone(),
            )
            .boxed(),
        },
    )
    .within(row(()).width(percent(100.0)));
    let inside = each(
        board::<B, _>(hub, move |l| l.shown(&key)),
        |id: &String| id.clone(),
        move |id| step_view(hub, chat, id.clone()),
    )
    .within(column(()).width(percent(100.0)).min_width(px(0.0)));
    with_cx(move |cx| {
        let theme = cx.theme();
        let rule = frame()
            .height(px(theme.space.border))
            .width(percent(100.0))
            .fill(theme.palette.hairline);
        column((head, rule, inside))
            .gap(theme.space.gap / 2.0)
            .align(AlignItems::FlexStart)
            .width(percent(100.0))
            .min_width(px(0.0))
    })
}

fn step_view(hub: Entity, chat: Entity, id: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let line = cx
            .world
            .get::<Transcript>(chat)
            .and_then(|t| t.item(&id))
            .is_some_and(|item| is_card(&item.entry));
        match line {
            true => {
                cx.build(super::card::view(hub, chat, id.clone()))
            }
            false => cx.build(entry_view(hub, chat, id.clone())),
        }
    })
}

/// The header of a turn that runs: "Working for 13s" or "Thinking".
fn working<B: Board>(hub: Entity, key: RowKey) -> impl View {
    let thinking_key = key.clone();
    let word = keyed(
        board::<B, _>(hub, move |l| l.is_thinking(&thinking_key)),
        |thinking| {
            shimmer(if *thinking { "Thinking" } else { "Working" })
                .rules(small_rules)
                .boxed()
        },
    )
    .within(row(()));
    let since = derived(move |world: &World| {
        let start = world
            .get::<B>(hub)
            .and_then(|board| board.lines().start(&key));
        match start {
            Some(start) if start > 0 => format!(
                " for {}",
                lasted(crate::clock::now_ms().saturating_sub(start))
            ),
            _ => String::new(),
        }
    })
    .when(every_frame());
    row((word, label(since).wrap(false).tone(Tone::Dim)))
        .gap(0.0)
        .padding(UiRect::axes(px(INSET), px(2.0)))
        .align(AlignItems::Center)
        .rules(small_rules)
}

/// The header of a turn that ended: "Worked for 2m 49s".
fn worked<B: Board>(
    hub: Entity,
    key: RowKey,
    chat: Entity,
    first: String,
) -> impl View {
    let (turn_key, failed_key) = (key.clone(), key.clone());
    let failed = keyed(
        board::<B, _>(hub, move |l| l.failed(&failed_key)),
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
        glyph(Glyph::Chevron, 10.0).rotation(board::<B, _>(
            hub,
            move |l| {
                if l.is_open(&turn_key) { 180.0 } else { 90.0 }
            },
        )),
        row((
            label(board::<B, _>(hub, move |l| l.heading(&key)))
                .wrap(false)
                .rules(small_rules),
            failed,
        )),
    ))
    .gap(6.0)
    .align(AlignItems::Center);
    button(words)
        .padding(UiRect::axes(px(INSET), px(2.0)))
        .rules(quiet_text_button)
        .on_activate(move |world| {
            let unfolded = &mut world.resource_mut::<Unfolded>().0;
            let fold = (hub, chat, first.clone());
            match unfolded.iter().position(|open| *open == fold) {
                Some(at) => {
                    unfolded.remove(at);
                }
                None => unfolded.push(fold),
            }
        })
}

/// The identity of a row: its agent, conversation and entry.
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

/// The rows of a Hub's chat.
#[derive(Component, Default, PartialEq)]
pub struct Lines {
    rows: Vec<Line>,
    places: HashMap<RowKey, usize>,
    /// The rows before these that are not built.
    earlier: usize,
    more: bool,
    /// Everyone who speaks in the chat, the agent it is with first.
    cast: Vec<Who>,
}

/// A Hub's component holding rows its chat draws.
pub(crate) trait Board: Component {
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

    fn shown(&self, key: &RowKey) -> Vec<String> {
        match self.fold(key) {
            Some(Row::Fold {
                steps, open: true, ..
            }) => steps.clone(),
            _ => Vec::new(),
        }
    }
}

/// Rows of a merged conversation added each time more is asked for.
const MERGED_PAGE: usize = 40;

/// An agent's conversation merged into a Hub's chat, open under its
/// row.
#[derive(Component, Default)]
pub struct MergedLines {
    /// The row it is open under.
    under: Option<(Entity, String)>,
    /// The chat watched for it.
    holder: Option<Entity>,
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
            || self.holder == Some(chat)
    }

    pub(super) fn is_open(&self, chat: Entity, id: &str) -> bool {
        self.under
            .as_ref()
            .is_some_and(|(of, entry)| *of == chat && entry == id)
    }
}

/// Opens agent `from`'s conversation under row `id` of `chat`, in
/// `hub`.
pub(crate) fn open_merged(
    world: &mut World,
    hub: Entity,
    chat: Entity,
    id: String,
    from: protocol::AgentId,
    name: String,
) {
    close_merged(world, hub);
    let reference = protocol::ChatRef { with: from };
    let holder = match chats::find(world, &reference) {
        Some(kept) => kept,
        None => world.spawn((chats::Chat(reference), ForMerged)).id(),
    };
    let agent = hamlet_client::realm::agent(world, from)
        .unwrap_or(Entity::PLACEHOLDER);
    world.entity_mut(hub).insert(MergedLines {
        under: Some((chat, id)),
        holder: Some(holder),
        agent: Some((agent, name)),
        reach: MERGED_PAGE,
        lines: Lines::default(),
    });
}

/// A chat watched only for the merged conversations open on it.
#[derive(Component)]
struct ForMerged;

/// Keeps `chat` once no merged conversation is open on it.
pub(crate) fn keep_merged(world: &mut World, chat: Entity) {
    if world.get::<ForMerged>(chat).is_some() {
        world.entity_mut(chat).remove::<ForMerged>();
    }
}

/// Shuts the merged conversation open in `hub`, and lets its chat go
/// once no Hub has it open.
pub(crate) fn close_merged(world: &mut World, hub: Entity) {
    let Some(mut open) = world.get_mut::<MergedLines>(hub) else {
        return;
    };
    let Some(holder) = std::mem::take(&mut *open).holder else {
        return;
    };
    // The holder may be the chat that is gone.
    if world.get::<ForMerged>(holder).is_none() {
        return;
    }
    let open = world
        .query::<&MergedLines>()
        .iter(world)
        .any(|other| other.holder == Some(holder));
    if !open {
        world.despawn(holder);
    }
}

/// Builds a page more of the merged conversation open in `hub`.
pub(crate) fn merged_earlier(world: &mut World, hub: Entity) {
    let Some(mut merged) = world.get_mut::<MergedLines>(hub) else {
        return;
    };
    let Some(holder) = merged.holder else {
        return;
    };
    let all_built = merged.lines.earlier == 0;
    merged.reach += MERGED_PAGE;
    if all_built {
        chats::earlier(world, holder);
    }
}

/// The turns each Hub had open when its rows were last built.
type Opened = HashMap<Entity, Vec<(Entity, String)>>;

/// Whether the turns open in `hub` changed since `seen` was told, so
/// a fold in one Hub builds no other Hub's rows again.
fn folds_moved(
    unfolded: &Res<Unfolded>,
    seen: &mut Opened,
    hub: Entity,
) -> bool {
    if !unfolded.is_changed() && seen.contains_key(&hub) {
        return false;
    }
    let now = unfolded.of(hub);
    if seen.get(&hub) == Some(&now) {
        return false;
    }
    seen.insert(hub, now);
    true
}

/// The rows of the merged conversation open in each Hub.
pub fn fold_merged(
    unfolded: Res<Unfolded>,
    transcripts: Query<Ref<Transcript>>,
    people: People,
    mut hubs: Query<(Entity, Ref<Convo>, &mut MergedLines)>,
    mut seen: Local<Opened>,
) {
    let me = people.me();
    for (hub, convo, mut merged) in &mut hubs {
        let holder = merged.holder;
        let said = holder.and_then(|at| transcripts.get(at).ok());
        let folded = folds_moved(&unfolded, &mut seen, hub);
        let moved = merged.is_changed()
            || convo.is_changed()
            || folded
            || people.has_moved()
            || said.as_ref().is_some_and(DetectChanges::is_changed);
        let (Some(holder), Some(said), Some((agent, name)), true) =
            (holder, said, merged.agent.clone(), moved)
        else {
            continue;
        };
        let (rows, cast) = timeline(
            &me,
            (agent, Some(name.as_str())),
            holder,
            &said,
            &unfolded.of(hub),
            &people,
        );
        let earlier = rows.len().saturating_sub(merged.reach);
        let mut now =
            Lines::of(rows[earlier..].to_vec(), earlier, cast);
        now.more |= said.earlier().is_some();
        if merged.lines != now {
            merged.lines = now;
        }
    }
    seen.retain(|hub, _| hubs.contains(*hub));
}

/// What a Hub's rows were last built from: its chat, the count of
/// its entries, their shape, and whether a turn ran.
type Built = (Entity, usize, u64, bool);

/// Each Hub's chat rows, spliced into its list so the others keep
/// their place.
pub fn fold_lines(
    unfolded: Res<Unfolded>,
    transcripts: Query<Ref<Transcript>>,
    people: People,
    mut hubs: Query<(Entity, Ref<Convo>, &mut Lines)>,
    mut lists: Query<(&ChatScroll, &mut VirtualList)>,
    mut from: Local<HashMap<Entity, Built>>,
    mut seen: Local<Opened>,
) {
    let me = people.me();
    for (hub, convo, mut lines) in &mut hubs {
        let folded = folds_moved(&unfolded, &mut seen, hub);
        let Ok(transcript) = transcripts.get(convo.chat) else {
            continue;
        };
        // Whether a turn runs changes no shape.
        let now_from = (
            convo.chat,
            transcript.items().len(),
            transcript.shape(),
            transcript.is_answering(),
        );
        if !convo.is_changed()
            && !folded
            && !people.has_moved()
            && from.get(&hub) == Some(&now_from)
        {
            continue;
        }
        from.insert(hub, now_from);
        let (rows, cast) = timeline(
            &me,
            (convo.agent, convo.with.as_deref()),
            convo.chat,
            &transcript,
            &unfolded.of(hub),
            &people,
        );
        let mut now = Lines::of(rows, 0, cast);
        now.more |= transcript.earlier().is_some();
        if *lines == now {
            continue;
        }
        for (scroll, mut list) in &mut lists {
            if scroll.0 == hub {
                splice(&mut list, &lines.rows, &now.rows);
            }
        }
        *lines = now;
    }
    from.retain(|hub, _| hubs.contains(*hub));
    seen.retain(|hub, _| hubs.contains(*hub));
}

/// Tells `list` which rows changed between `was` and `now`.
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

/// The earlier page a Hub's chat is receiving.
#[derive(Component, Default)]
pub struct Reading {
    page: Option<(protocol::RequestId, protocol::ChatRef)>,
    at_top: bool,
}

/// Asks for earlier rows once a Hub's chat is at its top or all fits.
pub fn ask_earlier(
    mut commands: Commands,
    transcripts: Query<&Transcript>,
    scrolls: Query<(&ChatScroll, &ComputedNode, &ScrollPosition)>,
    mut hubs: Query<(Ref<Convo>, &mut Reading)>,
) {
    for (scroll, node, at) in &scrolls {
        let hub = scroll.0;
        let Ok((convo, mut reading)) = hubs.get_mut(hub) else {
            continue;
        };
        if convo.is_changed() {
            *reading = Reading::default();
        }
        let furthest = super::furthest(node);
        // Reaching the top again asks again.
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
                if let Some(id) = &id {
                    super::rejected(world, hub, id.clone());
                }
                if let Some(mut reading) =
                    world.get_mut::<Reading>(hub)
                {
                    reading.page = id.map(|id| (id, reference));
                }
            });
        }
    }
}

/// The room between two rows of one run.
const GAP: f32 = 2.0;

/// The chat of `hub` with whoever its `Convo` points at.
pub fn transcript(hub: Entity) -> impl View {
    let said = virtual_list(
        component::<Lines, _>(hub, |l| l.map_or(0, |l| l.rows.len())),
        ROW_ESTIMATE,
        move |slot| chat_row(hub, slot),
    )
    .follow_bottom(true)
    .tagged(ChatScroll(hub))
    .tagged(FileDrop)
    // Shown once put where its chat was left.
    .tagged(Visibility::Hidden);
    let back = each(
        component::<AwayFromEnd, _>(hub, |away| {
            if away.is_some_and(|away| away.0) {
                vec![()]
            } else {
                Vec::new()
            }
        }),
        |_: &()| 0u8,
        move |_| {
            button(hamlet_ui::styles::raised(glyph(
                Glyph::ToEnd,
                hamlet_ui::styles::ICON,
            )))
            .padding(UiRect::all(px(8.0)))
            .variant(hamlet_ui::widgets::effects::Variant::Text)
            .tip("Scroll to the latest")
            .on_activate(move |world| super::to_end(world, hub))
            .boxed()
        },
    )
    .within(column(()).padding(UiRect::all(px(12.0))));
    let corner = overlay((back,))
        .justify(JustifyContent::FlexEnd)
        .align(AlignItems::FlexEnd);
    column((faded(said), quiet(hub), corner)).fills_height()
}

/// The greeting and starters over `hub`'s chat while it is empty.
fn quiet(hub: Entity) -> impl View {
    let empty = each(
        derived(move |world: &World| {
            let Some(lines) = world.get::<Lines>(hub) else {
                return Vec::new();
            };
            // No starter is offered while nothing could answer it.
            let own = !super::without_model(world, hub);
            match (
                lines.rows.is_empty() && !lines.more,
                lines.cast.first(),
            ) {
                (true, Some(agent)) => vec![(
                    agent.name.clone(),
                    own,
                    starters(
                        world,
                        crate::convo::agent_of(world, hub),
                    ),
                )],
                _ => Vec::new(),
            }
        })
        .when(every_frame()),
        |shown: &(String, bool, &'static [&'static str])| {
            shown.clone()
        },
        move |(name, own, starters)| {
            let (name, own, starters) =
                (name.clone(), *own, *starters);
            with_cx(move |cx| {
                let theme = cx.theme();
                let starters = starters
                    .iter()
                    .filter(|_| own)
                    .map(|said| {
                        lettered(*said)
                            .rules(hamlet_ui::styles::quiet_button)
                            .on_activate(move |world| {
                                super::field::set(
                                    world,
                                    hub,
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
                        .bold(true)
                        .tone(Tone::Dim),
                    column(starters)
                        .gap(theme.space.gap)
                        .align(AlignItems::Center),
                    note(
                        "Agents can make mistakes, so check \
                         anything important.",
                    )
                    .tagged(TextLayout::justify(Justify::Center)),
                ))
                .gap(theme.space.section)
                .align(AlignItems::Center)
                .padding(UiRect::all(px(theme.space.section)))
                // Centred while it fits; taller than the chat, it
                // keeps its top and is cut at the bottom.
                .margin(UiRect::vertical(Val::Auto))
            })
            .boxed()
        },
    );
    overlay((empty,))
        .justify(JustifyContent::Center)
        .align(AlignItems::FlexStart)
        .overflow(Overflow::clip())
}

/// The rows of the merged conversation open in `hub`.
pub(super) fn merged_rows(hub: Entity) -> impl View {
    each(
        board::<MergedLines, _>(hub, |lines| lines.rows.clone()),
        |line: &Line| line.key(),
        move |line| bubble::line_view::<MergedLines>(hub, line),
    )
    .within(
        column(())
            .gap(GAP)
            .width(percent(100.0))
            .min_width(px(LEAST)),
    )
}

/// The row of `hub`'s chat `slot` shows, built again only when it
/// shows another.
fn chat_row(hub: Entity, slot: RowSlot) -> AnyView {
    let index = slot.index();
    let mut moved = slot.index();
    let mut changed = component::<Lines, _>(hub, |_| ());
    let key = derived(move |world: &World| {
        let lines = world.get::<Lines>(hub)?;
        lines.rows.get(index.get(world)).map(Line::key)
    })
    .when(move |world: &World| {
        // Both checks run, as each takes in what it saw.
        moved.changed(world) | changed.changed(world)
    });
    keyed(key, move |key: &Option<RowKey>| {
        let key = key.clone();
        AnyView::new(move |cx: &mut Cx| {
            let line = key
                .as_ref()
                .and_then(|key| cx.world.get::<Lines>(hub)?.line(key))
                .cloned();
            match line {
                Some(line) => cx.build(
                    column((bubble::line_view::<Lines>(hub, &line),))
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

/// Lets a word wider than the conversation break inside itself.
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
