//! One entry of a conversation, drawn plainly: in a fold and for a
//! reply that failed.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, Overflow, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    ghost, label, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, component, derived, every_frame, keyed,
    resource,
};
use hamlet_client::chats::{Chat, Entry, Transcript};
use hamlet_client::messages::ChiefMessage;
use hamlet_client::realm::shown_of;
use hamlet_protocol::client::{Change, ToolCall};
use hamlet_ui::kinds::{card, tile};
use hamlet_ui::markdown::document;
use hamlet_ui::styles::{
    note, small_rules, status_mark, text_button, tinted_action,
};
use hamlet_ui::theme::StatusKind;
use hamlet_ui::widgets::ellipsis::{Keep, cut, one_line};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::MonoText;
use hamlet_ui::widgets::shimmer::shimmer;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::change::{counted, diff};
use super::lines::INSET;
use crate::convo::Convo;
use crate::markdown;

fn clip(s: &str, max: usize) -> String {
    let flat: String =
        s.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = flat.chars().count();
    if n <= max {
        flat
    } else {
        format!(
            "...{}",
            flat.chars().skip(n - max).collect::<String>()
        )
    }
}

/// The letters of a request a tool's row lays out, which its line
/// cuts again to the room it has.
const TOOL_TEXT: usize = 400;

/// The request to a tool on one line: its command, its path or its
/// arguments.
fn tool_text(call: &ToolCall, summary: &str) -> String {
    let shown = match call {
        ToolCall::Command { command } => command,
        ToolCall::Path { path } => path,
        _ => summary,
    };
    cut(shown, TOOL_TEXT)
}

/// The whole request to a tool: its command, else its arguments.
fn request(call: &ToolCall, summary: &str) -> String {
    match call {
        ToolCall::Command { command } => command.clone(),
        _ => summary.to_string(),
    }
}

/// What a tool call that has not ended is doing.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Underway {
    Running,
    /// Held until the chief answers.
    Waiting,
}

/// How the tool call at entry `id` of `chat` stands, as a mark and
/// how long it took.
pub(super) fn status(chat: Entity, id: String) -> impl View {
    keyed(
        component::<Transcript, _>(chat, move |t| ran(t, &id)),
        |ran| {
            let (which, tone, time) = match ran {
                Ran::Running => (Glyph::Clock, Tone::Warning, None),
                Ran::Done(ms) => (Glyph::Check, Tone::Success, *ms),
                Ran::Failed(_, ms) => {
                    (Glyph::Cross, Tone::Critical, *ms)
                }
            };
            row((
                glyph(which, 12.0).tone(tone),
                // A call quicker than a tenth of a second says no
                // time.
                label(
                    time.filter(|ms| *ms >= 100)
                        .map(took)
                        .unwrap_or_default(),
                )
                .wrap(false)
                .tone(Tone::Faint)
                .rules(small_rules),
            ))
            .gap(4.0)
            .align(AlignItems::Center)
            .boxed()
        },
    )
}

/// The tool calls opened to show their whole request, and the cards
/// opened in place, by their conversation and entry.
#[derive(Resource, Default)]
pub struct OpenTools(pub Vec<(Entity, String)>);

/// Opens the tool call or card `key`, or shuts it while it is open.
pub(super) fn toggle(world: &mut World, key: (Entity, String)) {
    let open = &mut world.resource_mut::<OpenTools>().0;
    match open.iter().position(|one| *one == key) {
        Some(at) => {
            open.remove(at);
        }
        None => open.push(key),
    }
}

/// Where a tool call stands.
#[derive(Clone, PartialEq)]
enum Ran {
    Running,
    Done(Option<u64>),
    Failed(i64, Option<u64>),
}

fn ran(transcript: Option<&Transcript>, id: &str) -> Ran {
    match transcript.and_then(|t| t.item(id)).map(|item| &item.entry)
    {
        Some(Entry::Tool {
            exit: Some(0), ms, ..
        }) => Ran::Done(*ms),
        Some(Entry::Tool {
            exit: Some(code),
            ms,
            ..
        }) => Ran::Failed(*code, *ms),
        _ => Ran::Running,
    }
}

/// How long a call took, short: `0.4s`, `12s`, `2m 3s`.
fn took(ms: u64) -> String {
    match ms {
        0..10_000 => format!("{:.1}s", ms as f64 / 1000.0),
        10_000..60_000 => format!("{}s", ms / 1000),
        _ => format!("{}m {}s", ms / 60_000, ms / 1000 % 60),
    }
}

/// What the line of a tool call says.
#[derive(Clone, PartialEq)]
struct Step {
    glyph: Glyph,
    /// What it does while it runs, or did once it ended.
    verb: &'static str,
    running: bool,
    failed: bool,
    /// What it acts on, on one line, and whole under the pointer.
    object: String,
    whole: String,
    /// A command or a tool's own name, set as code.
    code: bool,
    /// The lines an edit adds and takes out.
    counts: Option<Change>,
}

/// The last part of `path`, the file it names.
fn file_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_string()
}

/// `to` with the name the window shows its agent by, as
/// `hire#4 (Mira)`, when the window knows a name for it.
fn whom(
    world: &World,
    to: &str,
    agent: Option<hamlet_protocol::client::AgentId>,
) -> String {
    let shown = agent
        .and_then(|agent| hamlet_client::realm::agent(world, agent))
        .map(|agent| shown_of(world, agent))
        .filter(|shown| !shown.is_empty() && shown != to);
    match shown {
        Some(shown) => format!("{to} ({shown})"),
        None => to.to_string(),
    }
}

/// The line of the tool call at entry `id` of `chat`.
fn step_of(world: &World, chat: Entity, id: &str) -> Option<Step> {
    let transcript = world.get::<Transcript>(chat)?;
    let Some(Entry::Tool {
        name,
        call,
        summary,
        exit,
        change,
        agent,
        ..
    }) = transcript.item(id).map(|item| &item.entry)
    else {
        return None;
    };
    // A call of a turn cut short never ends.
    let running = exit.is_none() && transcript.is_answering();
    let tense = |doing, done| if running { doing } else { done };
    let mut step = Step {
        glyph: Glyph::Bolt,
        verb: "",
        running,
        failed: exit.is_some_and(|code| code != 0),
        object: tool_text(call, summary),
        whole: request(call, summary),
        code: false,
        counts: change.clone(),
    };
    match (call, name.as_str()) {
        (ToolCall::Notification { to, .. }, _) => {
            step.glyph = Glyph::Talk;
            step.verb = "Notification to";
            step.object = whom(world, to, *agent);
            step.whole = step.object.clone();
        }
        (ToolCall::Request { to, .. }, _) => {
            step.glyph = Glyph::Talk;
            step.verb = "Request to";
            step.object = whom(world, to, *agent);
            step.whole = step.object.clone();
        }
        (ToolCall::Command { .. }, _) => {
            step.glyph = Glyph::Terminal;
            step.verb = tense("Running", "Ran");
            step.code = true;
        }
        (ToolCall::Path { path }, "read") => {
            step.glyph = Glyph::Notes;
            step.verb = tense("Reading", "Read");
            step.object = file_name(path);
        }
        (ToolCall::Path { path }, "edit" | "write") => {
            let created = change.as_ref().is_some_and(|c| c.created);
            step.glyph = Glyph::Rename;
            step.verb = match (created, name.as_str()) {
                (true, _) => tense("Creating", "Created"),
                (false, "edit") => tense("Editing", "Edited"),
                (false, _) => tense("Writing", "Wrote"),
            };
            step.object = file_name(path);
        }
        (_, "search" | "find") => {
            step.glyph = Glyph::Lens;
            step.verb = tense("Searching", "Searched");
            // A search's pattern tells it apart more than its path.
            if let Some(pattern) =
                serde_json::from_str::<serde_json::Value>(summary)
                    .ok()
                    .as_ref()
                    .and_then(|args| args.get("pattern"))
                    .and_then(|pattern| pattern.as_str())
            {
                step.object = pattern.to_string();
            }
            step.code = true;
        }
        _ => {
            step.verb = tense("Calling", "Called");
            step.object = format!("{name} {}", step.object);
            step.code = true;
        }
    }
    Some(step)
}

/// A step's line: its icon, its verb, which shimmers while it runs,
/// what it acts on, cut in its middle, and an edit's count.
fn step_line(step: &Step) -> AnyView {
    let step = step.clone();
    with_cx(move |cx| {
        let tone = if step.failed {
            Tone::Critical
        } else {
            Tone::Dim
        };
        let verb = match step.running {
            true => shimmer(step.verb).boxed(),
            false => label(step.verb).wrap(false).tone(tone).boxed(),
        };
        let whole = step.whole.clone();
        // What tells one call from the next is often at the end, as
        // the file a command reads.
        let object =
            one_line(label(step.object.clone()).tone(Tone::Faint))
                .keep(Keep::Ends);
        let object = match step.code {
            true => object.tagged(MonoText).boxed(),
            false => object.boxed(),
        };
        let object = column((object,))
            .grow(1.0)
            .min_width(px(0.0))
            .overflow(Overflow::clip_x())
            .tooltip(move || note(whole.clone()));
        let mut parts = vec![
            glyph(step.glyph, 12.0).tone(tone).boxed(),
            verb,
            object.boxed(),
        ];
        if let Some(change) = &step.counts {
            parts.push(counted(change));
        }
        if step.failed {
            parts.push(glyph(Glyph::Cross, 12.0).tone(tone).boxed());
        }
        row(parts)
            .gap(cx.theme().space.gap / 2.0)
            .align(AlignItems::Center)
            .width(percent(100.0))
            .min_width(px(0.0))
            .rules(small_rules)
    })
    .boxed()
}

/// A tool call as one quiet line that opens to the whole request,
/// why it failed, the words of a notification or a request, and the
/// lines an edit changed.
fn step_row(chat: Entity, id: String) -> impl View {
    let key = (chat, id.clone());
    let is_open = {
        let key = key.clone();
        move |open: &OpenTools| open.0.contains(&key)
    };
    let line = {
        let id = id.clone();
        keyed(
            derived(move |world: &World| step_of(world, chat, &id))
                .when(every_frame()),
            |step| match step {
                Some(step) => step_line(step),
                None => frame().boxed(),
            },
        )
        .within(row(()).grow(1.0).min_width(px(0.0)))
    };
    let head = button(line)
        .width(percent(100.0))
        .min_width(px(0.0))
        .padding(UiRect::axes(px(INSET), px(2.0)))
        .rules(ghost)
        .on_activate(move |world| toggle(world, key.clone()));
    let read = {
        let id = id.clone();
        move |f: fn(&Entry) -> String| {
            let id = id.clone();
            component::<Transcript, _>(chat, move |t| {
                t.and_then(|t| t.item(&id).map(|item| &item.entry))
                    .map(f)
                    .unwrap_or_default()
            })
        }
    };
    let body =
        keyed(resource::<OpenTools, _>(is_open), move |open| {
            if !*open {
                return frame().boxed();
            }
            let (id, read) = (id.clone(), read.clone());
            AnyView::new(move |cx: &mut Cx| {
                let entry = cx
                    .world
                    .get::<Transcript>(chat)
                    .and_then(|t| t.item(&id))
                    .map(|item| item.entry.clone());
                let mut parts = vec![
                    note(read(|e| match e {
                        Entry::Tool {
                            call:
                                ToolCall::Notification { text, .. }
                                | ToolCall::Request { text, .. },
                            ..
                        } => text.clone(),
                        Entry::Tool { call, summary, .. } => {
                            request(call, summary)
                        }
                        _ => String::new(),
                    }))
                    .tagged(MonoText)
                    .boxed(),
                    keyed(
                        read(|e| match e {
                            Entry::Tool {
                                exit: Some(code),
                                failure,
                                ..
                            } if *code != 0 => match failure {
                                Some(why) => {
                                    format!(
                                        "Failed (exit {code}): {why}"
                                    )
                                }
                                None => {
                                    format!("Failed (exit {code})")
                                }
                            },
                            _ => String::new(),
                        }),
                        |why| match why.is_empty() {
                            true => frame().boxed(),
                            false => failed(why.clone()).boxed(),
                        },
                    )
                    .within(column(()).width(percent(100.0)))
                    .boxed(),
                ];
                if let Some(Entry::Tool {
                    change: Some(change),
                    ..
                }) = entry
                {
                    parts.push(diff(chat, id.clone(), change));
                }
                let theme = cx.theme();
                cx.build(
                    column(parts)
                        .gap(theme.space.gap / 2.0)
                        .width(percent(100.0))
                        .min_width(px(0.0))
                        .padding(UiRect::new(
                            px(INSET + theme.space.gap * 2.0),
                            px(0.0),
                            px(2.0),
                            px(2.0),
                        )),
                )
            })
            .boxed()
        });
    column((head, body))
        .width(percent(100.0))
        .min_width(px(0.0))
        .gap(0.0)
}

/// How long the model reasoned for the thinking at entry `id`: the
/// time to its next message, less the tools that ran between.
fn thought(transcript: &Transcript, id: &str) -> Option<u64> {
    let items = transcript.items();
    let at = items.iter().position(|item| item.id == id)?;
    let began = items[at].at;
    let mut ran = 0;
    for item in &items[at + 1..] {
        if item.at != began {
            return item.at.checked_sub(began + ran);
        }
        if let Entry::Tool { ms, .. } = &item.entry {
            ran += ms.unwrap_or(0);
        }
    }
    None
}

/// Whether the model may still be writing the thinking at `id`: its
/// turn runs, and no input after it was taken up.
fn is_thinking(transcript: &Transcript, id: &str) -> bool {
    transcript.is_answering()
        && transcript
            .items()
            .iter()
            .skip_while(|item| item.id != id)
            .skip(1)
            .all(|item| {
                item.queued
                    || !matches!(
                        item.entry,
                        Entry::Chief { .. }
                            | Entry::Agent { .. }
                            | Entry::Answered(_)
                    )
            })
}

/// The model's thinking: the end of its words on one dim line while
/// it thinks, then "Thought for 6s", which opens to them.
fn reasoning_row(chat: Entity, id: String) -> impl View {
    let read_id = id.clone();
    let stands = component::<Transcript, _>(chat, move |t| {
        t.map(|t| (is_thinking(t, &read_id), thought(t, &read_id)))
            .unwrap_or_default()
    });
    keyed(stands, move |(live, took)| {
        let id = id.clone();
        match live {
            true => {
                let words = component::<Transcript, _>(
                    chat,
                    move |t| match t
                        .and_then(|t| t.item(&id))
                        .map(|i| &i.entry)
                    {
                        Some(Entry::Reasoning(said)) => {
                            clip(said, 160)
                        }
                        _ => String::new(),
                    },
                );
                column((one_line(label(words).tone(Tone::Dim))
                    .keep(Keep::Ends)
                    .rules(small_rules),))
                .width(percent(100.0))
                .min_width(px(0.0))
                .overflow(Overflow::clip_x())
                .padding(UiRect::axes(px(INSET), px(2.0)))
                .boxed()
            }
            false => thought_row(chat, id, *took).boxed(),
        }
    })
    .within(column(()).width(percent(100.0)).min_width(px(0.0)))
}

/// "Thought for 6s", which opens to what the model thought.
fn thought_row(
    chat: Entity,
    id: String,
    took: Option<u64>,
) -> impl View {
    let key = (chat, id.clone());
    let (turned, pressed) = (key.clone(), key.clone());
    let words = match took.filter(|ms| *ms >= 1000) {
        Some(ms) => {
            format!("Thought for {}", super::lines::lasted(ms))
        }
        None => "Thought".to_string(),
    };
    let head = button(
        row((
            glyph(Glyph::Chevron, 10.0).rotation(resource::<
                OpenTools,
                _,
            >(
                move |open| {
                    if open.0.contains(&turned) {
                        180.0
                    } else {
                        90.0
                    }
                },
            )),
            label(words).wrap(false).rules(small_rules),
        ))
        .gap(6.0)
        .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(INSET), px(2.0)))
    .rules(text_button)
    .on_activate(move |world| toggle(world, pressed.clone()));
    let body = keyed(
        resource::<OpenTools, _>(move |open| open.0.contains(&key)),
        move |open| match open {
            true => with_cx({
                let id = id.clone();
                move |cx| {
                    column((document(
                        markdown::entry(chat, id.clone()),
                        Tone::Dim,
                    ),))
                    .width(percent(100.0))
                    .min_width(px(0.0))
                    .padding(UiRect::axes(
                        px(INSET + cx.theme().space.gap * 2.0),
                        px(2.0),
                    ))
                }
            })
            .boxed(),
            false => frame().boxed(),
        },
    )
    .within(column(()).width(percent(100.0)).min_width(px(0.0)));
    column((head, body))
        .width(percent(100.0))
        .min_width(px(0.0))
}

/// Why a tool call failed, in a red block under its request.
fn failed(why: String) -> impl View {
    with_cx(move |cx| {
        tile((row((
            status_mark(Glyph::Cross, StatusKind::Danger, 16.0),
            row((label(why.clone())
                .tone(Tone::Critical)
                .rules(small_rules),))
            .grow(1.0)
            .min_width(px(0.0)),
        ))
        .gap(cx.theme().space.gap)
        .align(AlignItems::Center),))
        .status(StatusKind::Danger)
        .width(percent(100.0))
    })
}

pub(crate) fn entry_view(chat: Entity, id: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let kind = cx
            .world
            .get::<Transcript>(chat)
            .and_then(|t| t.item(&id).map(|item| &item.entry))
            .cloned();
        // The chief by "You".
        let convo = cx.world.resource::<Convo>();
        let chief = match &kind {
            Some(Entry::Chief { name, .. })
                if *name == convo.name =>
            {
                "You".to_string()
            }
            Some(Entry::Chief { name, .. }) => name.clone(),
            _ => String::new(),
        };
        let meta = cx.theme().text.small;
        let text_id = id.clone();
        let text = move |f: fn(&Entry) -> String| {
            let id = text_id.clone();
            component::<Transcript, _>(chat, move |t| {
                t.and_then(|t| t.item(&id).map(|item| &item.entry))
                    .map(f)
                    .unwrap_or_default()
            })
        };
        let view: AnyView = match kind {
            Some(Entry::Chief { .. }) => card((
                label(chief).size(meta).bold(true).tone(Tone::Faint),
                label(text(|e| match e {
                    Entry::Chief { text, .. } => text.clone(),
                    _ => String::new(),
                })),
            ))
            .boxed(),
            // Words the agent wrote between the steps of a turn.
            Some(Entry::Assistant(_)) => column((document(
                markdown::entry(chat, id),
                Tone::Body,
            ),))
            .width(percent(100.0))
            .min_width(px(0.0))
            .padding(UiRect::axes(px(INSET), px(2.0)))
            .boxed(),
            Some(Entry::Reasoning(_)) => {
                reasoning_row(chat, id).boxed()
            }
            Some(Entry::Tool {
                call: ToolCall::Worker { .. },
                ..
            }) => super::card::view(chat, id),
            Some(Entry::Tool { .. }) => step_row(chat, id).boxed(),
            Some(Entry::Error(_)) => column((
                label(text(|e| match e {
                    Entry::Error(s) => format!(
                        "Could not finish that reply: {}. Try again \
                         to carry on from where it ended.",
                        s.trim_end_matches('.')
                    ),
                    _ => String::new(),
                }))
                .tone(Tone::Critical),
                // In a row, so that it is as wide as its words.
                row((tinted_action(
                    StatusKind::Warning,
                    Glyph::Restore,
                    "Try Again",
                    move |world| {
                        let Some(to) = world
                            .get::<Chat>(chat)
                            .map(|chat| chat.0.with)
                        else {
                            return;
                        };
                        world.trigger(ChiefMessage::words(
                            to,
                            "/retry".into(),
                        ));
                    },
                ),)),
            ))
            .gap(8.0)
            .padding(UiRect::axes(px(12.0), px(4.0)))
            .boxed(),
            Some(Entry::Notice(_)) => {
                column((note(text(|e| match e {
                    Entry::Notice(s) => s.clone(),
                    _ => String::new(),
                })),))
                .padding(UiRect::axes(px(INSET), px(2.0)))
                .boxed()
            }
            Some(Entry::Answered(_)) => card((
                label("Your answer")
                    .size(meta)
                    .bold(true)
                    .tone(Tone::Faint),
                label(text(|e| match e {
                    Entry::Answered(s) => s.clone(),
                    _ => String::new(),
                })),
            ))
            .boxed(),
            // Rows of their own.
            Some(
                Entry::Agent { .. }
                | Entry::Summary(_)
                | Entry::Merged { .. }
                | Entry::Event(_),
            )
            | None => frame().boxed(),
        };
        cx.build(view)
    })
}
