//! The pieces the desk's pages are made of: a state's pill, a box
//! with its head, a line of a box, a listing's initial and a button
//! that cannot be pressed.

use bevy::color::Color;
use bevy::ecs::hierarchy::ChildOf;
use bevy::text::{
    FontSize, FontWeight, TextColor, TextFont, TextSpan,
};
use bevy::ui::{AlignItems, JustifyContent, UiRect, px};
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use hamlet_ui::styles::{Fills, hairline, note};
use hamlet_ui::theme::Palette;
use hamlet_ui::widgets::platform::DisplayText;
use hamlet_ui::{AnyView, Cx};

use crate::bazaar::market::versions::State;

/// The size of the words of a pill.
const PILL: f32 = 11.0;

/// `text` in the colour `ink` picks from the palette, at `size`.
pub(super) fn inked(
    text: impl Into<String>,
    ink: fn(&Palette) -> Color,
    size: f32,
    bold: bool,
) -> AnyView {
    let text = text.into();
    AnyView::new(move |cx: &mut Cx| {
        let root = cx.build(label("").size(size));
        let colour = ink(&cx.theme().palette);
        cx.world.spawn((
            TextSpan(text),
            TextFont {
                font_size: FontSize::Px(size),
                weight: match bold {
                    true => FontWeight::BOLD,
                    false => FontWeight::NORMAL,
                },
                ..TextFont::default()
            },
            TextColor(colour),
            ChildOf(root),
        ));
        root
    })
}

/// The colours of a state: its words and its frame.
fn colours(
    state: State,
) -> (fn(&Palette) -> Color, fn(&Palette) -> Color) {
    match state {
        State::Draft => (|p| p.gold_4, |p| p.gold_6),
        State::Unverified => (|p| p.gold_2, |p| p.gold_5),
        State::Checking => (|p| p.glow, |p| p.gold_4),
        State::Verified => (|p| p.green, |p| p.green_deep),
        State::Rejected => (|p| p.red, |p| p.danger_border),
    }
}

/// `state` as a pill.
pub(super) fn state_pill(state: State) -> AnyView {
    let (ink, edge) = colours(state);
    pill(state.words(), ink, edge)
}

/// `words` as a pill in `ink`, framed in `edge`.
pub(super) fn pill(
    words: impl Into<String>,
    ink: fn(&Palette) -> Color,
    edge: fn(&Palette) -> Color,
) -> AnyView {
    let words = words.into();
    AnyView::new(move |cx: &mut Cx| {
        let edge = edge(&cx.theme().palette);
        cx.build(
            row((inked(words.clone(), ink, PILL, false),))
                .padding(UiRect::axes(px(8.0), px(2.0)))
                .border(1.0)
                .border_color(edge)
                .radius(10.0)
                .align(AlignItems::Center),
        )
    })
}

/// A plain pill, such as a listing's kind.
pub(super) fn plain_pill(words: impl Into<String>) -> AnyView {
    pill(words, |p| p.gold_3, |p| p.gold_6)
}

/// A box: its `name` and `aside` in its head, over `body`.
pub(super) fn boxed(
    name: String,
    aside: AnyView,
    body: AnyView,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (edge, fill, round) = (
            theme.palette.gold_6,
            theme.palette.card,
            theme.space.round,
        );
        let head = row((
            label(name.clone())
                .size(12.0)
                .bold(true)
                .tone(Tone::Accent)
                .wrap(false)
                .tagged(DisplayText),
            column((aside,)).fills_width().align(AlignItems::FlexEnd),
        ))
        .gap(12.0)
        .align(AlignItems::Center)
        .padding(UiRect::axes(px(12.0), px(9.0)));
        cx.build(
            column((head, hairline(edge), body))
                .gap(0.0)
                .border(1.0)
                .border_color(edge)
                .radius(round)
                .fill(fill)
                .min_width(px(0.0)),
        )
    })
}

/// A box with no head about `body`.
pub(super) fn plain_box(body: AnyView) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (edge, fill, round) = (
            theme.palette.gold_6,
            theme.palette.card,
            theme.space.round,
        );
        cx.build(
            column((body,))
                .border(1.0)
                .border_color(edge)
                .radius(round)
                .fill(fill)
                .min_width(px(0.0)),
        )
    })
}

/// Dim words for the end of a box's head, such as why it is locked.
pub(super) fn aside(words: impl Into<String>) -> AnyView {
    label(words.into())
        .size(11.0)
        .tone(Tone::Faint)
        .tagged(DisplayText)
        .boxed()
}

/// A line of a box: `name`, and `value` at the end of it.
pub(super) fn line(name: &str, value: AnyView) -> AnyView {
    row((
        label(name.to_string()).size(13.0).tone(Tone::Dim),
        column((value,)).fills_width().align(AlignItems::FlexEnd),
    ))
    .gap(16.0)
    .align(AlignItems::Center)
    .padding(UiRect::axes(px(12.0), px(8.0)))
    .boxed()
}

/// `lines` one under another, a hairline between each.
pub(super) fn lines(lines: Vec<AnyView>) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let edge = cx.theme().palette.gold_6;
        let mut all = Vec::new();
        for one in lines {
            if !all.is_empty() {
                all.push(hairline(edge).boxed());
            }
            all.push(one);
        }
        cx.build(column(all).gap(0.0))
    })
}

/// Words that may run long, at the end of a line.
pub(super) fn value(words: impl Into<String>) -> AnyView {
    label(words.into()).size(13.0).boxed()
}

/// A tile with the first letter of `name`, `side` pixels a side.
pub(super) fn initial(name: &str, side: f32) -> AnyView {
    let letter = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or('?', |c| c.to_ascii_uppercase())
        .to_string();
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        cx.build(
            column((label(letter.clone())
                .size(side * 0.45)
                .bold(true)
                .tone(Tone::Accent)
                .tagged(DisplayText),))
            .width(px(side))
            .height(px(side))
            .min_width(px(side))
            .justify(JustifyContent::Center)
            .align(AlignItems::Center)
            .fill(theme.palette.primary_low)
            .border(1.0)
            .border_color(theme.palette.gold_5)
            .radius(theme.space.round),
        )
    })
}

/// A button's words in its frame, faint, that does nothing when
/// pressed, with why beside it.
pub(super) fn idle(
    words: &'static str,
    why: Option<&'static str>,
) -> AnyView {
    let plate = AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        cx.build(
            row((label(words)
                .size(theme.text.small)
                .bold(true)
                .tone(Tone::Faint)
                .wrap(false)
                .tagged(DisplayText),))
            .padding(hamlet_ui::styles::pad())
            .min_height(px(hamlet_ui::styles::CONTROL))
            .align(AlignItems::Center)
            .justify(JustifyContent::Center)
            .border(theme.space.border)
            .border_color(theme.palette.gold_6)
            .radius(theme.space.round)
            .opacity(0.6),
        )
    });
    match why {
        Some(why) => column((plate, note(why))).gap(4.0).boxed(),
        None => plate,
    }
}

/// The rules of a card that can be chosen, lit while it is.
pub(super) fn card(
    lit: bool,
) -> impl FnOnce(&mut Cx) + Send + Sync + 'static {
    move |cx: &mut Cx| {
        hamlet_ui::styles::card_button(cx);
        if lit {
            bevy_fynix::style::<hamlet_ui::theme::HamletTheme>()
                .frame(|f, t| {
                    f.fill(t.palette.primary_low)
                        .border_color(t.palette.glow)
                })
                .hovered(|s| {
                    s.frame(|f, t| {
                        f.fill(t.palette.primary_low)
                            .border_color(t.palette.glow)
                    })
                })
                .apply(cx);
        }
    }
}

/// `body` in a card that cannot be pressed, dimmed.
pub(super) fn dimmed(body: AnyView) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        cx.build(
            column((body,))
                .grow(1.0)
                .padding(UiRect::axes(
                    px(theme.space.card_padding_x),
                    px(theme.space.card_padding_y),
                ))
                .fill(theme.palette.card)
                .border(theme.space.border)
                .border_color(theme.palette.gold_6)
                .radius(theme.space.round)
                .opacity(0.55),
        )
    })
}

/// `cards` in rows of `across`, each as wide as the others.
pub(super) fn grid(cards: Vec<AnyView>, across: usize) -> AnyView {
    let across = across.max(1);
    let mut rows = Vec::new();
    let mut cards = cards.into_iter().peekable();
    while cards.peek().is_some() {
        let cells = (0..across)
            .map(|_| {
                let card = cards
                    .next()
                    .unwrap_or_else(|| column(()).boxed());
                column((card,))
                    .width(px(0.0))
                    .grow(1.0)
                    .align(AlignItems::Stretch)
                    .boxed()
            })
            .collect::<Vec<_>>();
        rows.push(
            row(cells).gap(10.0).align(AlignItems::Stretch).boxed(),
        );
    }
    column(rows).gap(10.0).boxed()
}
