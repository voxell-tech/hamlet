//! Markdown drawn as the chat draws it.

use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::label;
use hamlet_sdk::{AnyView, with_cx};

use crate::chat::Views;

/// `text` as markdown in `tone`, laid out once. Plain words where the
/// chat is not there to draw it.
pub fn fixed(text: &str, tone: Tone) -> AnyView {
    let text = text.to_string();
    with_cx(move |cx| match cx.world.get_resource::<Views>() {
        Some(views) => (views.markdown)(&text, tone),
        None => label(text).tone(tone).boxed(),
    })
}
