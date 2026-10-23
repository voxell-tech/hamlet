//! Where a page is within its panel, as the shell's breadcrumb shows
//! it.

use hamlet_engine_bevy::prelude::World;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, Face, FrameProps, Prop, Size, Space, Tone,
    UiExt, ViewExt, button, label, row,
};

/// Where a page is within its panel: a quiet button for each of
/// `trail`, the pages above it from the top, each followed by a dim
/// `/`, and then `current`, the page shown. A press hands the place
/// in `trail` of its page to `go`.
pub fn breadcrumb<F>(
    trail: Vec<String>,
    current: impl Into<Prop<World, String>>,
    go: F,
) -> AnyView
where
    F: Fn(&mut World, usize) + Clone + Send + Sync + 'static,
{
    let mut parts = Vec::new();
    for (at, text) in trail.into_iter().enumerate() {
        let go = go.clone();
        parts.push(
            button(text)
                .style(ButtonStyle::Quiet)
                .on_click(move |world| go(world, at))
                .boxed(),
        );
        parts.push(
            label("/")
                .size(Size::Small)
                .tone(Tone::Dim)
                .face(Face::Display)
                .boxed(),
        );
    }
    parts.push(
        label(current)
            .size(Size::Small)
            .bold(true)
            .tone(Tone::Faint)
            .face(Face::Display)
            .boxed(),
    );
    row(parts).space(Space::Gap).boxed()
}
