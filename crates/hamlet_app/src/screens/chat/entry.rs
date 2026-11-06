//! One entry of a conversation, drawn plainly: in a fold, in the
//! reader of a past session, and for a reply that failed.

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
use hamlet_ui::markdown::document;
use hamlet_ui::styles::note;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::{DisplayText, MonoText};
use hamlet_ui::{AnyView, Cx, View};

use super::change::change_view;
use crate::convo::Convo;
use crate::markdown;

/// A single line cut to its first `most` characters.
pub(super) fn clip_start(text: &str, most: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(most) {
        Some((cut, _)) => format!("{}...", &flat[..cut]),
        None => flat,
    }
}

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

/// The request to a tool: the shell command itself when the arguments
/// are `{"command": ...}`, the raw arguments otherwise.
fn tool_text(summary: &str) -> String {
    let command = serde_json::from_str::<serde_json::Value>(summary)
        .ok()
        .and_then(|v| v["command"].as_str().map(str::to_string));
    clip_start(command.as_deref().unwrap_or(summary), 90)
}

/// How the tool call at entry `id` of `session` stands.
pub(super) fn state(session: Entity, id: String) -> impl View {
    let status_id = id.clone();
    label(component::<Transcript, _>(session, move |t| {
        match t
            .and_then(|t| t.item(&status_id))
            .map(|item| &item.entry)
        {
            Some(Entry::Tool { exit: Some(0), .. }) => {
                "Done".to_string()
            }
            Some(Entry::Tool {
                exit: Some(code), ..
            }) => format!("Failed (exit {code})"),
            _ => "Running".to_string(),
        }
    }))
    .size(12.0)
    .wrap(false)
    .tone(component::<Transcript, _>(
        session,
        move |t| match t
            .and_then(|t| t.item(&id))
            .map(|item| &item.entry)
        {
            Some(Entry::Tool { exit: Some(0), .. }) => Tone::Dim,
            Some(Entry::Tool { exit: Some(_), .. }) => Tone::Critical,
            _ => Tone::Accent,
        },
    ))
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
    let status_id = id.clone();
    let status = keyed(
        component::<Transcript, _>(session, move |t| {
            ran(t, &status_id)
        }),
        |ran| {
            let (which, tone, time) = match ran {
                Ran::Running => (Glyph::Clock, Tone::Accent, None),
                Ran::Done(ms) => (Glyph::Check, Tone::Dim, *ms),
                Ran::Failed(_, ms) => {
                    (Glyph::Cross, Tone::Critical, *ms)
                }
            };
            row((
                glyph(which, 12.0).tone(tone),
                label(time.map(took).unwrap_or_default())
                    .size(11.0)
                    .wrap(false)
                    .tone(Tone::Faint),
            ))
            .gap(4.0)
            .align(AlignItems::Center)
            .boxed()
        },
    );
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
            .size(12.0)
            .bold(true)
            .wrap(false)
            .tagged(MonoText),
            column((label(read(|e| match e {
                Entry::Tool { summary, .. } => tool_text(summary),
                _ => String::new(),
            }))
            .size(12.0)
            .wrap(false)
            .tone(Tone::Dim)
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
                label(read(|e| match e {
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
                }))
                .size(12.0)
                .tone(Tone::Critical),
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

pub(crate) fn entry_view(session: Entity, id: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let kind = cx
            .world
            .get::<Transcript>(session)
            .and_then(|t| t.item(&id).map(|item| &item.entry))
            .cloned();
        let speaker = match &cx.world.resource::<Convo>().with {
            Some(name) => name.to_uppercase(),
            None => "MAYOR".to_string(),
        };
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
                label(text(|e| match e {
                    Entry::Chief { name, .. } => name.clone(),
                    _ => String::new(),
                }))
                .size(11.0)
                .bold(true)
                .tone(Tone::Faint)
                .tagged(DisplayText),
                label(text(|e| match e {
                    Entry::Chief { text, .. } => text.clone(),
                    _ => String::new(),
                })),
            ))
            .gap(2.0)
            .padding(UiRect::axes(px(12.0), px(8.0)))
            .border(1.0)
            .border_color(cx.theme().palette.gold_6)
            .radius(cx.theme().space.round)
            .fill(cx.theme().palette.card)
            .boxed(),
            Some(Entry::Assistant(_)) => column((
                label(speaker.clone())
                    .size(11.0)
                    .bold(true)
                    .tone(Tone::Faint)
                    .tagged(DisplayText),
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
            }) => column((change_view(session, id, change),))
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
                button(label("Try again").size(12.0))
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
                    Entry::Agent { name, .. } => name.to_uppercase(),
                    _ => String::new(),
                }))
                .size(11.0)
                .bold(true)
                .tone(Tone::Accent)
                .tagged(DisplayText),
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
                    .size(11.0)
                    .bold(true)
                    .tone(Tone::Faint)
                    .tagged(DisplayText),
                label(text(|e| match e {
                    Entry::Answered(s) => s.clone(),
                    _ => String::new(),
                })),
            ))
            .gap(2.0)
            .padding(UiRect::axes(px(12.0), px(8.0)))
            .border(1.0)
            .border_color(cx.theme().palette.gold_6)
            .radius(cx.theme().space.round)
            .fill(cx.theme().palette.card)
            .boxed(),
            None => frame().boxed(),
        };
        cx.build(view)
    })
}
