//! One entry of a conversation, drawn plainly: in a fold, in the
//! reader of a past session, and for a reply that failed.

use bevy::ecs::entity::Entity;
use bevy::ui::{UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, ghost, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, component};
use hamlet_core::session::{
    Entry, SessionKey, Transcript, UserMessage,
};
use hamlet_rules::words;

use super::change::change_view;
use crate::looks::{Fills, note};
use crate::me::Convo;
use crate::platform::{DisplayText, MonoText};
use crate::{AnyView, Cx, View, markdown};

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
    words::clip(command.as_deref().unwrap_or(summary), 90)
}

/// How the tool call at entry `id` of `session` stands.
pub(super) fn state(session: Entity, id: u64) -> impl View {
    let exit = move |t: Option<&Transcript>| match t
        .and_then(|t| t.get(id))
    {
        Some(Entry::Tool { exit, .. }) => *exit,
        _ => None,
    };
    label(component::<Transcript, _>(session, move |t| {
        match exit(t) {
            Some(0) => "Done".to_string(),
            Some(code) => format!("Failed (exit {code})"),
            None => "Running".to_string(),
        }
    }))
    .size(12.0)
    .wrap(false)
    .tone(component::<Transcript, _>(session, move |t| {
        match exit(t) {
            Some(0) => Tone::Dim,
            Some(_) => Tone::Critical,
            None => Tone::Accent,
        }
    }))
}

pub(crate) fn entry_view(session: Entity, id: u64) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let kind = cx
            .world
            .get::<Transcript>(session)
            .and_then(|t| t.get(id))
            .cloned();
        let speaker = match &cx.world.resource::<Convo>().with {
            Some(name) => name.to_uppercase(),
            None => "MAYOR".to_string(),
        };
        let text = move |f: fn(&Entry) -> String| {
            component::<Transcript, _>(session, move |t| {
                t.and_then(|t| t.get(id)).map(f).unwrap_or_default()
            })
        };
        let view: AnyView = match kind {
            Some(Entry::User { .. }) => column((
                label(text(|e| match e {
                    Entry::User { user, .. } => user.clone(),
                    _ => String::new(),
                }))
                .size(11.0)
                .bold(true)
                .tone(Tone::Faint)
                .tagged(DisplayText),
                label(text(|e| match e {
                    Entry::User { text, .. } => text.clone(),
                    _ => String::new(),
                })),
            ))
            .gap(2.0)
            .padding(UiRect::axes(px(12.0), px(8.0)))
            .border(1.0)
            .border_color(cx.theme().palette.brass_6)
            .radius(cx.theme().space.round)
            .fill(cx.theme().palette.card)
            .boxed(),
            Some(Entry::Assistant(_)) => column((
                label(speaker.clone())
                    .size(11.0)
                    .bold(true)
                    .tone(Tone::Faint)
                    .tagged(DisplayText),
                markdown::document(
                    markdown::entry(session, id),
                    Tone::Body,
                ),
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
            Some(Entry::Tool { .. }) => row((
                column((note(text(|e| match e {
                    Entry::Tool { name, summary, .. } => {
                        format!("> {name} {}", tool_text(summary))
                    }
                    _ => String::new(),
                }))
                .tagged(MonoText),))
                .fills_width(),
                state(session, id),
            ))
            .gap(12.0)
            .padding(UiRect::axes(px(24.0), px(0.0)))
            .boxed(),
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
                        let Some(key) =
                            world.get::<SessionKey>(session).cloned()
                        else {
                            return;
                        };
                        let user =
                            world.resource::<Convo>().user.clone();
                        world.trigger(UserMessage {
                            key,
                            user,
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
                markdown::document(
                    markdown::entry(session, id),
                    Tone::Dim,
                ),
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
            None => frame().boxed(),
        };
        cx.build(view)
    })
}
