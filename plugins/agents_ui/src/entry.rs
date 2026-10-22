//! One message of a session drawn plainly: in a fold of a turn's
//! steps, in the reader of a past session, and for a reply that
//! failed, with Try again.

use hamlet_engine_bevy::AgentsRequest;
use hamlet_engine_bevy::api::agents::{
    Entry, Recipient, SessionMessage,
};
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, Face, FrameProps, Size, Space, Tone, UiExt,
    ViewExt, button, column, description, label, row,
};

use crate::change::{change_view, state};
use crate::client::AgentsWorldExt;
use crate::markdown::markdown;
use crate::words::{clip, tail};

/// The characters of a thought shown.
const THOUGHT: usize = 160;

/// The characters of a tool's command shown.
const COMMAND: usize = 90;

/// A head over words: a name in small capitals.
fn head(text: String, tone: Tone) -> AnyView {
    label(text)
        .size(Size::Small)
        .bold(true)
        .face(Face::Display)
        .tone(tone)
        .boxed()
}

/// The message `message`, said by `speaker` when it is the agent's,
/// its change folded by `key`. Try again carries on the conversation
/// with `to`.
pub fn entry_view(
    message: &SessionMessage,
    speaker: &str,
    key: String,
    to: Option<Recipient>,
) -> AnyView {
    match &message.entry {
        Entry::User { user, text } => column((
            head(user.clone(), Tone::Faint),
            label(text.clone()),
        ))
        .space(Space::None)
        .boxed(),
        Entry::Assistant(text) => column((
            head(speaker.to_uppercase(), Tone::Faint),
            markdown(text, Tone::Body),
        ))
        .space(Space::None)
        .boxed(),
        Entry::Reasoning(text) => {
            label(format!("Thinking: {}", tail(text, THOUGHT)))
                .size(Size::Small)
                .tone(Tone::Dim)
                .boxed()
        }
        Entry::Tool {
            change: Some(change),
            exit,
            ..
        } => change_view(key, change, *exit),
        Entry::Tool {
            name,
            command,
            exit,
            ..
        } => row((
            label(format!("> {name} {}", clip(command, COMMAND)))
                .size(Size::Small)
                .tone(Tone::Dim),
            state(*exit),
        ))
        .space(Space::Gap)
        .boxed(),
        Entry::Error(why) => {
            let mut parts = vec![
                label(format!(
                    "Could not finish that reply: {why}. Try again \
                     to carry on from where it ended."
                ))
                .tone(Tone::Critical)
                .boxed(),
            ];
            if let Some(to) = to {
                parts.push(
                    button("Try again")
                        .style(ButtonStyle::Quiet)
                        // Carries on with the reply that failed.
                        .on_click(move |world| {
                            world.ask(AgentsRequest::Send {
                                to: to.clone(),
                                text: "/retry".into(),
                            })
                        })
                        .boxed(),
                );
            }
            column(parts).space(Space::Half).boxed()
        }
        Entry::Agent { name, text } => column((
            head(name.to_uppercase(), Tone::Accent),
            markdown(text, Tone::Dim),
        ))
        .space(Space::None)
        .boxed(),
        Entry::Notice(text) => description(text.clone()).boxed(),
        Entry::Answered(text) => column((
            head("Your answer".into(), Tone::Faint),
            label(text.clone()),
        ))
        .space(Space::None)
        .boxed(),
    }
}
