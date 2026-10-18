//! The card a row of a list is, and the heading over a part of a
//! panel.

use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ScopedExt, StateExt, ViewExt};

use crate::icons::{Glyph, glyph};
use crate::looks::{Fills, ICON, away, raised};
use crate::platform::DisplayText;
use crate::{AnyView, Cx};

/// One card: a title over a dim line.
pub fn item(title: String, detail: String, tone: Tone) -> AnyView {
    card(title, detail, tone, None)
}

/// One card: a title, with `beside` it at the card's right when
/// there is something to press, over a dim line the card's whole
/// width.
pub fn card(
    title: String,
    detail: String,
    tone: Tone,
    beside: Option<AnyView>,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let name = row((label(title)
            .size(13.0)
            .bold(true)
            .tone(tone)
            .tagged(DisplayText),))
        .fills_width()
        .boxed();
        let head =
            std::iter::once(name).chain(beside).collect::<Vec<_>>();
        cx.build(
            column((
                row(head).gap(8.0).align(AlignItems::Center),
                label(detail).size(13.0).tone(Tone::Dim),
            ))
            .gap(4.0)
            .padding(UiRect::axes(px(14.0), px(10.0)))
            .border(1.0)
            .border_color(cx.theme().palette.brass_6)
            .radius(cx.theme().space.round)
            .fill(cx.theme().palette.card)
            .appear(away)
            .transition(Motion::Expand),
        )
    })
}

/// A heading over a part of the panel, behind its icon.
pub fn heading(which: Glyph, text: impl Into<String>) -> AnyView {
    row((
        raised(glyph(which, ICON).tone(Tone::Faint)),
        label(text.into())
            .size(11.0)
            .bold(true)
            .tone(Tone::Faint)
            .tagged(DisplayText),
    ))
    .gap(7.0)
    .align(AlignItems::Center)
    .boxed()
}
