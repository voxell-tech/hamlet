//! One entry of a conversation, drawn plainly: in a fold and for a
//! reply that failed.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ui::{AlignItems, Overflow, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, ghost, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, component, keyed, resource};
use hamlet_client::chats::{Chat, Entry, Transcript};
use hamlet_client::messages::ChiefMessage;
use hamlet_client::realm::shown_of;
use hamlet_ui::markdown::document;
use hamlet_ui::styles::{note, small_rules, status_mark};
use hamlet_ui::theme::StatusKind;
use hamlet_ui::widgets::ellipsis::{Keep, cut, one_line};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::MonoText;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::change::change_view;
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

/// The request to a tool: the shell command itself when the arguments
/// are `{"command": ...}`, the raw arguments otherwise.
fn tool_text(summary: &str) -> String {
    let command = serde_json::from_str::<serde_json::Value>(summary)
        .ok()
        .and_then(|v| v["command"].as_str().map(str::to_string));
    cut(command.as_deref().unwrap_or(summary), TOOL_TEXT)
}

/// What a tool call that has not ended is doing.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Underway {
    Running,
    /// Held until the chief answers.
    Waiting,
}

/// How the tool call at entry `id` of `session` stands, as a mark and
/// how long it took.
pub(super) fn status(session: Entity, id: String) -> impl View {
    keyed(
        component::<Transcript, _>(session, move |t| ran(t, &id)),
        |ran| {
            let (which, tone, time) = match ran {
                Ran::Running => (Glyph::Clock, Tone::Warning, None),
                Ran::Done(ms) => (Glyph::Check, Tone::Dim, *ms),
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

/// The tool calls opened to show their whole request.
#[derive(Resource, Default)]
pub struct OpenTools(pub Vec<(Entity, String)>);

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

/// A tool call as one line, its tool, its request and how it went,
/// that opens to the whole request and why it failed.
fn tool_row(session: Entity, id: String) -> impl View {
    let key = (session, id.clone());
    let is_open = {
        let key = key.clone();
        move |open: &OpenTools| open.0.contains(&key)
    };
    let read = {
        let id = id.clone();
        move |f: fn(&Entry) -> String| {
            let id = id.clone();
            component::<Transcript, _>(session, move |t| {
                t.and_then(|t| t.item(&id).map(|item| &item.entry))
                    .map(f)
                    .unwrap_or_default()
            })
        }
    };
    let status = status(session, id.clone());
    let turned = is_open.clone();
    let head = button(
        row((
            glyph(Glyph::Chevron, 10.0).tone(Tone::Faint).rotation(
                resource::<OpenTools, _>(move |open| {
                    if turned(open) { 180.0 } else { 90.0 }
                }),
            ),
            label(read(|e| match e {
                Entry::Tool { name, .. } => name.clone(),
                _ => String::new(),
            }))
            .bold(true)
            .wrap(false)
            .rules(small_rules)
            .tagged(MonoText),
            // What tells one call from the next is often at the
            // end, as the file a command reads.
            column((one_line(
                label(read(|e| match e {
                    Entry::Tool { summary, .. } => tool_text(summary),
                    _ => String::new(),
                }))
                .tone(Tone::Dim),
            )
            .keep(Keep::Ends)
            .rules(small_rules)
            .tagged(MonoText),))
            .grow(1.0)
            .min_width(px(0.0))
            .overflow(Overflow::clip_x()),
            status,
        ))
        .gap(8.0)
        .align(AlignItems::Center)
        .width(percent(100.0))
        .min_width(px(0.0)),
    )
    .width(percent(100.0))
    .min_width(px(0.0))
    .padding(UiRect::axes(px(6.0), px(2.0)))
    .rules(ghost)
    .on_activate(move |world| {
        let open = &mut world.resource_mut::<OpenTools>().0;
        match open.iter().position(|one| *one == key) {
            Some(at) => {
                open.remove(at);
            }
            None => open.push(key.clone()),
        }
    });
    let body =
        keyed(resource::<OpenTools, _>(is_open), move |open| {
            if !*open {
                return frame().boxed();
            }
            column((
                note(read(|e| match e {
                    Entry::Tool { summary, .. } => request(summary),
                    _ => String::new(),
                }))
                .tagged(MonoText),
                keyed(
                    read(|e| match e {
                        Entry::Tool {
                            exit: Some(code),
                            failure,
                            ..
                        } if *code != 0 => match failure {
                            Some(why) => {
                                format!("Failed (exit {code}): {why}")
                            }
                            None => format!("Failed (exit {code})"),
                        },
                        _ => String::new(),
                    }),
                    |why| match why.is_empty() {
                        true => frame().boxed(),
                        false => failed(why.clone()).boxed(),
                    },
                )
                .within(column(()).width(percent(100.0))),
            ))
            .gap(4.0)
            .width(percent(100.0))
            .min_width(px(0.0))
            .padding(UiRect::axes(px(26.0), px(2.0)))
            .boxed()
        });
    column((head, body))
        .width(percent(100.0))
        .min_width(px(0.0))
        .gap(0.0)
}

/// Why a tool call failed, in a red block under its request.
fn failed(why: String) -> impl View {
    with_cx(move |cx| {
        let theme = cx.theme();
        row((
            status_mark(Glyph::Cross, StatusKind::Danger, 16.0),
            row((label(why.clone())
                .tone(Tone::Critical)
                .rules(small_rules),))
            .grow(1.0)
            .min_width(px(0.0)),
        ))
        .gap(theme.space.gap)
        .align(AlignItems::Center)
        .width(percent(100.0))
        .padding(UiRect::axes(px(8.0), px(4.0)))
        .fill(theme.palette.danger)
        .border(theme.space.border)
        .border_color(theme.palette.danger_border)
        .radius(theme.space.round)
    })
}

/// The whole request to a tool: the shell command when the arguments
/// are `{"command": ...}`, else the arguments laid out.
fn request(summary: &str) -> String {
    let value =
        serde_json::from_str::<serde_json::Value>(summary).ok();
    match &value {
        Some(v) if v["command"].is_string() => {
            v["command"].as_str().unwrap_or_default().to_string()
        }
        Some(v) => serde_json::to_string_pretty(v)
            .unwrap_or_else(|_| summary.to_string()),
        None => summary.to_string(),
    }
}

/// The change of the edit at entry `id` of `session`, at the edge of
/// what holds it.
pub(super) fn edit_view(session: Entity, id: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let change = cx
            .world
            .get::<Transcript>(session)
            .and_then(|t| t.item(&id))
            .and_then(|item| match &item.entry {
                Entry::Tool {
                    change: Some(change),
                    ..
                } => Some(change.clone()),
                _ => None,
            });
        match change {
            Some(change) => cx.build(change_view(
                session,
                id,
                change,
                Underway::Running,
            )),
            None => cx.build(frame()),
        }
    })
}

pub(crate) fn entry_view(session: Entity, id: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let kind = cx
            .world
            .get::<Transcript>(session)
            .and_then(|t| t.item(&id).map(|item| &item.entry))
            .cloned();
        // Named as the chat names them: the agent by what it is
        // shown by, and the chief by "You".
        let convo = cx.world.resource::<Convo>();
        let speaker = match &convo.with {
            Some(address) => Some(shown_of(cx.world, convo.agent))
                .filter(|shown| !shown.is_empty())
                .unwrap_or_else(|| address.clone()),
            None => "The Hand".to_string(),
        };
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
            component::<Transcript, _>(session, move |t| {
                t.and_then(|t| t.item(&id).map(|item| &item.entry))
                    .map(f)
                    .unwrap_or_default()
            })
        };
        let view: AnyView = match kind {
            Some(Entry::Chief { .. }) => column((
                label(chief).size(meta).bold(true).tone(Tone::Faint),
                label(text(|e| match e {
                    Entry::Chief { text, .. } => text.clone(),
                    _ => String::new(),
                })),
            ))
            .gap(2.0)
            .padding(UiRect::axes(px(12.0), px(8.0)))
            .border(cx.theme().space.border)
            .border_color(cx.theme().palette.gold_6)
            .radius(cx.theme().space.round)
            .fill(cx.theme().palette.card)
            .boxed(),
            Some(Entry::Assistant(_)) => column((
                label(speaker.clone())
                    .size(meta)
                    .bold(true)
                    .tone(Tone::Faint),
                document(markdown::entry(session, id), Tone::Body),
            ))
            .gap(2.0)
            .padding(UiRect::axes(px(12.0), px(4.0)))
            .boxed(),
            Some(Entry::Reasoning(_)) => {
                column((note(text(|e| match e {
                    Entry::Reasoning(s) => {
                        format!("Thinking: {}", clip(s, 160))
                    }
                    _ => String::new(),
                })),))
                .padding(UiRect::axes(px(24.0), px(0.0)))
                .boxed()
            }
            Some(Entry::Tool {
                change: Some(change),
                ..
            }) => column((change_view(
                session,
                id,
                change,
                Underway::Running,
            ),))
            .width(percent(100.0))
            .padding(UiRect::axes(px(24.0), px(0.0)))
            .boxed(),
            Some(Entry::Tool { .. }) => {
                column((tool_row(session, id),))
                    .width(percent(100.0))
                    .min_width(px(0.0))
                    .padding(UiRect::axes(px(16.0), px(0.0)))
                    .boxed()
            }
            Some(Entry::Error(_)) => column((
                label(text(|e| match e {
                    Entry::Error(s) => format!(
                        "Could not finish that reply: {s}. Try \
                         again to carry on from where it ended."
                    ),
                    _ => String::new(),
                }))
                .tone(Tone::Critical),
                button(label("Try again").rules(small_rules))
                    .rules(ghost)
                    .on_activate(move |world| {
                        let Some(to) = world
                            .get::<Chat>(session)
                            .filter(|chat| chat.0.past.is_none())
                            .map(|chat| chat.0.with)
                        else {
                            return;
                        };
                        world.trigger(ChiefMessage {
                            to,
                            text: "/retry".into(),
                        });
                    }),
            ))
            .gap(4.0)
            .padding(UiRect::axes(px(12.0), px(4.0)))
            .boxed(),
            Some(Entry::Agent { .. }) => column((
                label(text(|e| match e {
                    Entry::Agent { name, .. } => name.clone(),
                    _ => String::new(),
                }))
                .size(meta)
                .bold(true)
                .tone(Tone::Accent),
                document(markdown::entry(session, id), Tone::Dim),
            ))
            .gap(2.0)
            .padding(UiRect::axes(px(24.0), px(2.0)))
            .boxed(),
            Some(Entry::Notice(_)) => {
                column((note(text(|e| match e {
                    Entry::Notice(s) => s.clone(),
                    _ => String::new(),
                })),))
                .padding(UiRect::axes(px(12.0), px(0.0)))
                .boxed()
            }
            Some(Entry::Answered(_)) => column((
                label("Your answer")
                    .size(meta)
                    .bold(true)
                    .tone(Tone::Faint),
                label(text(|e| match e {
                    Entry::Answered(s) => s.clone(),
                    _ => String::new(),
                })),
            ))
            .gap(2.0)
            .padding(UiRect::axes(px(12.0), px(8.0)))
            .border(cx.theme().space.border)
            .border_color(cx.theme().palette.gold_6)
            .radius(cx.theme().space.round)
            .fill(cx.theme().palette.card)
            .boxed(),
            None => frame().boxed(),
        };
        cx.build(view)
    })
}
