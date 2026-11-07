//! The styles of the buttons and fields, and the view shapes the
//! panels share.

use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::world::World;
use bevy::ui::{
    AlignItems, InteractionDisabled, JustifyContent, UiRect, px,
};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    self, BehaviorExt, Frame, FrameProps, Icon, Stack, TooltipExt,
    column, frame, label, row, scroll, tinted_icon,
};
use bevy_fynix::{
    Bevy, Prop, ScopedExt, ViewExt, ViewSeq, Visual, style,
};

use crate::theme::{HamletTheme, TextSizes};
use crate::widgets::effects::Variant;
use crate::widgets::ellipsis::one_line;
use crate::widgets::icons::{Glyph, glyph};
use crate::widgets::platform::{DisplayText, MonoText, Wraps};
use crate::{AnyView, Cx, View, with_cx};

/// The least height of any button (macOS default control size, 28
/// pt).
pub const CONTROL: f32 = 28.0;

/// The padding inside a text button.
pub fn pad() -> UiRect {
    UiRect::axes(px(14.0), px(0.0))
}

/// A standard button: bordered, with gold text. It darkens while
/// pressed and is only an outline while it cannot be pressed.
pub fn button(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.button)
                .border(t.space.border)
                .border_color(t.palette.gold_5)
                .radius(t.space.round)
        })
        .label(|l, t| {
            l.tone(Tone::Faint).size(t.text.small).bold(true)
        })
        .icon(|i, _| i.tone(Tone::Faint))
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.gold_hover)
                    .border_color(t.palette.gold_3)
            })
            .tone(Tone::Body)
        })
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.input).border_color(t.palette.gold_5)
            })
            .tone(Tone::Dim)
        })
        .when::<InteractionDisabled>(|s| {
            s.frame(|f, t| {
                f.fill(Color::NONE).border_color(t.palette.gold_6)
            })
            .tone(Tone::Dim)
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// The button for the one action a screen is for: a dark amber fill
/// that brightens under the pointer, and a plain dim plate while it
/// cannot be pressed.
pub fn primary_button(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.primary_low)
                .border(t.space.border)
                .border_color(t.palette.gold_4)
                .radius(t.space.round)
        })
        .label(|l, t| {
            l.tone(Tone::Body).size(t.text.small).bold(true)
        })
        .icon(|i, _| i.tone(Tone::Body))
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.primary_low)
                    .border_color(t.palette.accent)
            })
        })
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.primary_low)
                    .border_color(t.palette.gold_5)
            })
            .tone(Tone::Faint)
        })
        .when::<InteractionDisabled>(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.button)
                    .border_color(t.palette.gold_6)
            })
            .tone(Tone::Dim)
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// A link back to the page the current one was reached from: an
/// arrow and a name with no fill, dim until the pointer is on it.
pub fn back_link(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, _| f.fill(Color::NONE))
        .label(|l, t| l.tone(Tone::Dim).size(t.text.small).bold(true))
        .icon(|i, _| i.tone(Tone::Dim))
        .hovered(|s| s.fill(|_| Color::NONE).tone(Tone::Accent))
        .pressed(|s| s.fill(|_| Color::NONE).tone(Tone::Faint))
        .transition(Motion::Interact)
        .apply(cx);
}

/// A view before it comes in and after it goes: transparent and
/// slightly small.
pub fn hidden(cx: &mut Cx) {
    cx.set::<Visual>(|visual, _| visual.opacity(0.0));
    cx.root(|cx| cx.set::<Visual>(|visual, _| visual.scale(0.96)));
}

/// On a text input whose text was refused, until it is changed.
#[derive(Component)]
pub struct Refused;

/// A text input: darker than the panel, with a quiet border that
/// lightens under the pointer and turns to the accent while it is
/// typed in, or red while its text is [`Refused`].
pub fn text_input(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.input)
                .border(t.space.border)
                .border_color(t.palette.gold_5)
                .radius(t.space.round)
        })
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.input).border_color(t.palette.grey_1)
            })
        })
        .when::<bevy_fynix::Focused>(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.input).border_color(t.palette.gold_4)
            })
        })
        .when::<Refused>(|s| {
            s.frame(|f, t| f.border_color(t.palette.red))
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// A button for what refuses, stops or throws away: a dark red fill
/// and a red border, with a red glow under the pointer.
pub fn danger_button(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.danger)
                .border(t.space.border)
                .border_color(t.palette.danger_border)
                .radius(t.space.round)
        })
        .label(|l, t| {
            l.tone(Tone::Critical).size(t.text.small).bold(true)
        })
        .icon(|i, _| i.tone(Tone::Critical))
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.danger_hover)
                    .border_color(t.palette.red)
            })
            .tone(Tone::Body)
        })
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.input).border_color(t.palette.red)
            })
            .tone(Tone::Critical)
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// The room left over along a row or a column, taken and given back
/// when there is less than what is inside would like.
pub trait Fills: FrameProps + Sized {
    /// Takes the height left over, and may shrink below its content.
    fn fills_height(self) -> Self {
        self.grow(1.0).min_height(px(0.0))
    }

    /// Takes the width left over, and may shrink below its content.
    fn fills_width(self) -> Self {
        self.grow(1.0).min_width(px(0.0))
    }
}

impl<T: FrameProps> Fills for T {}

/// A column with no gap, for a keyed view to be built under.
pub fn bare() -> Stack<()> {
    column(()).gap(0.0)
}

/// A column with no gap that fills the height, for a keyed view to
/// be built under.
pub fn filling() -> Stack<()> {
    column(()).fills_height().gap(0.0)
}

/// A line one pixel thick, of `colour`.
pub fn hairline(colour: impl Into<Prop<Color>>) -> Frame {
    frame().height(px(1.0)).fill(colour)
}

/// `children` one under another, as far apart as the parts of a
/// panel.
pub fn parts(
    children: impl ViewSeq<Bevy, HamletTheme> + 'static,
) -> impl View {
    with_cx(move |cx| column(children).gap(cx.theme().space.panel))
}

/// `children` one under another, as far apart as the lines of one
/// part.
pub fn group(
    children: impl ViewSeq<Bevy, HamletTheme> + 'static,
) -> impl View {
    with_cx(move |cx| column(children).gap(cx.theme().space.gap))
}

/// `children` one under another, close together.
pub fn close(
    children: impl ViewSeq<Bevy, HamletTheme> + 'static,
) -> impl View {
    with_cx(move |cx| {
        column(children).gap(cx.theme().space.gap / 2.0)
    })
}

/// `children` side by side, as far apart as a panel's parts are.
pub fn beside(
    children: impl ViewSeq<Bevy, HamletTheme> + 'static,
) -> impl View {
    with_cx(move |cx| {
        row(children)
            .gap(cx.theme().space.section)
            .align(AlignItems::Center)
    })
}

/// `rows` as one list, each padded, a line between each and the
/// next.
pub fn list(rows: Vec<AnyView>) -> impl View {
    with_cx(move |cx| {
        let space = &cx.theme().space;
        let line = cx.theme().palette.gold_6;
        let rows = rows
            .into_iter()
            .enumerate()
            .map(|(at, one)| {
                let padded = column((one,))
                    .gap(0.0)
                    .padding(UiRect::vertical(px(space.row_padding)));
                if at == 0 {
                    padded.boxed()
                } else {
                    column((hairline(line), padded)).gap(0.0).boxed()
                }
            })
            .collect::<Vec<_>>();
        column(rows).gap(0.0)
    })
}

/// A command, in monospace at the body size on a block of code the
/// width of its column, wrapping where it is long.
pub fn command_block(text: impl Into<Prop<String>>) -> impl View {
    inset(label(text).tagged(MonoText))
}

/// `content` on a block of code the width of its column: what a
/// file or a command says, set apart from the panel's own words.
pub fn inset(content: impl View) -> impl View {
    with_cx(move |cx| {
        let theme = cx.theme();
        column((content,))
            .min_width(px(0.0))
            .padding(UiRect::axes(px(10.0), px(8.0)))
            .fill(theme.palette.code)
            .border(theme.space.border)
            .border_color(theme.palette.gold_6)
            .radius(theme.space.round)
    })
}

fn chip(words: impl View + Send + Sync + 'static) -> impl View {
    with_cx(move |cx| {
        let theme = cx.theme();
        row((words,))
            .min_width(px(0.0))
            .padding(UiRect::axes(px(5.0), px(0.0)))
            .fill(theme.palette.code)
            .border(theme.space.border)
            .border_color(theme.palette.gold_6)
            .radius(theme.space.round)
    })
}

/// `line` on one line, each command in it, between backticks, on a
/// chip, cut short with an ellipsis where it does not fit. A line
/// with none wraps as words do.
pub fn quoted(line: &str) -> AnyView {
    if !line.contains('`') {
        return label(line.to_string()).boxed();
    }
    let parts = line
        .split('`')
        .enumerate()
        .filter(|(_, part)| !part.trim().is_empty())
        .map(|(at, part)| {
            if at % 2 == 1 {
                chip(
                    one_line(label(part.to_string()))
                        .tagged(MonoText),
                )
                .boxed()
            } else {
                label(part.trim().to_string()).wrap(false).boxed()
            }
        })
        .collect::<Vec<_>>();
    row(parts)
        .gap(6.0)
        .align(AlignItems::Center)
        .min_width(px(0.0))
        .boxed()
}

/// Small dim words, too short to be a description: a value, or
/// what an empty list says.
pub fn note(text: impl Into<Prop<String>>) -> impl View {
    label(text).rules(note_rules)
}

/// The rules of a note: every label in the view small, not bold and
/// dim.
pub fn note_rules(cx: &mut Cx) {
    style::<HamletTheme>()
        .label(|l, t| {
            l.size(t.text.small).bold(false).tone(Tone::Dim)
        })
        .apply(cx);
}

/// The rules of small words: every label in the view small.
pub fn small_rules(cx: &mut Cx) {
    style::<HamletTheme>()
        .label(|l, t| l.size(t.text.small))
        .apply(cx);
}

/// The rules of a long description: every label in the view small,
/// not bold, dim and wrapping.
pub fn description_rules(cx: &mut Cx) {
    style::<HamletTheme>()
        .label(|l, t| {
            l.size(t.text.small)
                .bold(false)
                .tone(Tone::Dim)
                .wrap(true)
        })
        .apply(cx);
}

/// `text` as a long description, in [`description_rules`].
pub fn description(text: impl Into<Prop<String>>) -> impl View {
    label(text).rules(description_rules)
}

/// The name at the head of a page.
pub fn title(text: impl Into<Prop<String>>) -> impl View {
    label(text)
        .wrap(false)
        .rules(|cx: &mut Cx| {
            style::<HamletTheme>()
                .label(|l, t| l.size(t.text.title).bold(true))
                .apply(cx);
        })
        .tagged(DisplayText)
}

/// The name of a part of a panel: small and bold.
pub fn section(text: impl Into<Prop<String>>) -> impl View {
    label(text).rules(|cx: &mut Cx| {
        style::<HamletTheme>()
            .label(|l, t| {
                l.size(t.text.small).bold(true).tone(Tone::Faint)
            })
            .apply(cx);
    })
}

/// The name on a card, in `tone`: heading size and bold.
pub fn card_name(
    text: impl Into<Prop<String>>,
    tone: Tone,
) -> impl View {
    let text = text.into();
    with_cx(move |cx| {
        label(text)
            .size(cx.theme().text.heading)
            .bold(true)
            .tone(tone)
            .wrap(false)
    })
}

/// The line over a card that waits for the person: small and bold, in
/// `tone`.
pub fn caption(
    text: impl Into<Prop<String>>,
    tone: Tone,
) -> impl View {
    let text = text.into();
    with_cx(move |cx| {
        label(text)
            .size(cx.theme().text.small)
            .bold(true)
            .tone(tone)
    })
}

/// `children` in a column that scrolls, taking the height left.
pub fn scroll_area(
    children: impl ViewSeq<Bevy, HamletTheme> + 'static,
) -> impl View {
    with_cx(move |cx| {
        scroll(children).gap(cx.theme().space.gap).fills_height()
    })
}

/// `content` in a card: a shade above the panel, in a frame.
pub fn framed(
    content: impl ViewSeq<Bevy, HamletTheme> + 'static,
) -> impl View {
    with_cx(move |cx| {
        let theme = cx.theme();
        let space = &theme.space;
        column(content)
            .gap(space.gap / 2.0)
            .padding(UiRect::axes(
                px(space.card_padding_x),
                px(space.card_padding_y),
            ))
            .border(space.border)
            .border_color(theme.palette.gold_6)
            .radius(space.round)
            .fill(theme.palette.card)
    })
}

/// A button with `text`.
pub fn lettered(
    text: impl Into<Prop<String>>,
) -> impl View + FrameProps {
    views::button(label(text).wrap(false))
        .padding(pad())
        .min_height(px(CONTROL))
}

/// The side of an icon set beside lettering.
pub const ICON: f32 = 14.0;

/// An icon set a pixel high, on the middle of the lettering beside
/// it.
pub fn raised(mark: Icon) -> impl View + FrameProps {
    row((mark,)).padding(UiRect::bottom(px(2.0)))
}

/// An icon before `words`.
pub fn headed(
    which: Glyph,
    words: impl View + 'static,
) -> impl View + FrameProps {
    row((raised(glyph(which, ICON)), words))
        .gap(6.0)
        .align(AlignItems::Center)
}

/// A button with an icon before its text.
pub fn marked(
    which: Glyph,
    text: impl Into<Prop<String>>,
) -> impl View + FrameProps {
    views::button(headed(which, label(text).wrap(false)))
        // Less on the icon's side, which has air of its own.
        .padding(UiRect::new(px(10.0), px(13.0), px(0.0), px(0.0)))
        .min_height(px(CONTROL))
}

/// A status text in the small size, behind its icon, both in
/// `tone`.
pub fn status_label(
    which: Glyph,
    text: impl Into<String>,
    tone: Tone,
) -> impl View {
    status_label_at(which, text.into(), tone, |sizes| sizes.small)
}

/// A status text in the body size, behind its icon, both in
/// `tone`.
pub fn status_label_large(
    which: Glyph,
    text: impl Into<String>,
    tone: Tone,
) -> impl View {
    status_label_at(which, text.into(), tone, |sizes| sizes.body)
}

fn status_label_at(
    which: Glyph,
    text: String,
    tone: Tone,
    size: fn(&TextSizes) -> f32,
) -> impl View {
    with_cx(move |cx| {
        let size = size(&cx.theme().text);
        row((
            glyph(which, size).tone(tone),
            label(text).size(size).tone(tone).wrap(false),
        ))
        .gap(cx.theme().space.status_icon_gap)
        .align(AlignItems::Center)
    })
}

/// Buttons in a row that wraps: in a narrow panel they go on to a
/// line below and are never cut off at its edge.
pub fn buttons(buttons: Vec<AnyView>) -> impl View {
    with_cx(move |cx| {
        row(buttons)
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center)
            .tagged(Wraps)
    })
}

/// One answer of two to a few, as buttons joined in one track, the
/// one at `picked` lit. A press hands the place of its answer to
/// `pick`.
pub fn segmented<F>(
    options: impl IntoIterator<Item = AnyView>,
    picked: usize,
    pick: F,
) -> impl View
where
    F: Fn(&mut World, usize) + Clone + Send + Sync + 'static,
{
    let options = options
        .into_iter()
        .enumerate()
        .map(|(at, content)| {
            let pick = pick.clone();
            views::button(content)
                .rules(segment(at == picked))
                .on_activate(move |world| pick(world, at))
                .boxed()
        })
        .collect::<Vec<_>>();
    with_cx(move |cx| {
        let theme = cx.theme();
        let space = &theme.space;
        // In a row of its own, so it is as wide as its answers.
        row((row(options)
            .gap(space.track_padding)
            .padding(UiRect::all(px(space.track_padding)))
            .fill(theme.palette.input)
            .border(space.border)
            .border_color(theme.palette.gold_5)
            .radius(space.track_radius)
            .align(AlignItems::Stretch)
            .tagged(Wraps),))
    })
}

/// [`segmented`] with words for its answers.
pub fn segmented_words<F>(
    words: impl IntoIterator<Item = &'static str>,
    picked: usize,
    pick: F,
) -> impl View
where
    F: Fn(&mut World, usize) + Clone + Send + Sync + 'static,
{
    segmented(
        words
            .into_iter()
            .map(|text| label(text).wrap(false).boxed()),
        picked,
        pick,
    )
}

/// The rules of a segment of a [`segmented`] track: a button's
/// plate, and the picked one lit as the primary action is.
fn segment(
    picked: bool,
) -> impl FnOnce(&mut Cx) + Send + Sync + 'static {
    let base = style::<HamletTheme>()
        .label(|l, t| l.size(t.text.small).bold(true))
        .frame(|f, t| {
            // As tall in its track as a lone button is.
            let inside =
                2.0 * (t.space.track_padding + t.space.border);
            f.border(t.space.border)
                .radius(
                    (t.space.track_radius - t.space.track_padding)
                        .max(0.0),
                )
                .min_height(px(CONTROL - inside))
                .padding(UiRect::axes(px(t.space.gap), px(0.0)))
        });
    let rules = if picked {
        base.frame(|f, t| {
            f.fill(t.palette.primary_low)
                .border_color(t.palette.gold_4)
        })
        .tone(Tone::Body)
        .icon(|i, _| i.tone(Tone::Body))
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.primary_low)
                    .border_color(t.palette.accent)
            })
        })
    } else {
        base.frame(|f, t| {
            f.fill(t.palette.button).border_color(t.palette.button)
        })
        .tone(Tone::Faint)
        .icon(|i, _| i.tone(Tone::Faint))
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.button_hover)
                    .border_color(t.palette.gold_5)
            })
            .tone(Tone::Body)
        })
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.input).border_color(t.palette.gold_5)
            })
        })
    };
    rules.transition(Motion::Interact).bundle()
}

/// The rules of an answer to pick: no fill until the pointer is on
/// it, and the one picked filled and bordered.
pub fn choice(
    picked: bool,
) -> impl FnOnce(&mut Cx) + Send + Sync + 'static {
    let base = style::<HamletTheme>()
        .label(|l, t| l.size(t.text.small).bold(true))
        .frame(|f, t| {
            f.border(t.space.border)
                .radius(t.space.round)
                .min_height(px(t.space.row))
                .padding(UiRect::axes(px(t.space.gap), px(0.0)))
        });
    let rules = if picked {
        base.frame(|f, t| {
            f.fill(t.palette.primary_low)
                .border_color(t.palette.gold_4)
        })
        .tone(Tone::Body)
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.primary_low)
                    .border_color(t.palette.accent)
            })
        })
    } else {
        base.frame(|f, _| {
            f.fill(Color::NONE).border_color(Color::NONE)
        })
        .tone(Tone::Faint)
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.button_hover)
                    .border_color(Color::NONE)
            })
            .tone(Tone::Body)
        })
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.input).border_color(Color::NONE)
            })
        })
    };
    rules.transition(Motion::Interact).bundle()
}

/// A standard button that runs `act`.
pub fn command(text: &'static str, act: fn(&mut World)) -> impl View {
    lettered(text)
        .rules(button)
        .tagged(Variant::Default)
        .on_activate(act)
}

/// The way back to `text`, which `act` takes.
pub fn back(text: &'static str, act: fn(&mut World)) -> impl View {
    marked(Glyph::Back, text)
        .rules(back_link)
        .tagged(Variant::Text)
        .on_activate(act)
}

/// A button with an icon before its text, in the style `rules` and
/// the effects of `variant`, that runs `act`.
fn acting(
    which: Glyph,
    text: &'static str,
    (rules, variant): (fn(&mut Cx), Variant),
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    marked(which, text)
        .rules(rules)
        .tagged(variant)
        .on_activate(act)
}

/// A small "x" that closes or drops something by running `act`: dim,
/// and red under the pointer, as a dock tab's is.
pub fn close_button(
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    tinted_icon(Glyph::Cross.image(ICON))
        .hover(Tone::Critical)
        .on_activate(act)
}

/// A standard button with an icon before its text, that runs `act`.
pub fn action(
    which: Glyph,
    text: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    acting(which, text, (button, Variant::Default), act).boxed()
}

/// A "..." button, as tall as a standard one, that runs `act`, with
/// `tip`, what it offers, as its tooltip.
pub fn more_button(
    tip: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    views::button(glyph(Glyph::More, ICON))
        .padding(UiRect::axes(px(8.0), px(4.0)))
        .min_height(px(CONTROL))
        .rules(button)
        .tagged(Variant::Default)
        .tooltip(move || note(tip))
        .on_activate(act)
        .boxed()
}

/// The button for the one action a screen is for, that runs `act`.
pub fn good_action(
    which: Glyph,
    text: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    acting(which, text, (primary_button, Variant::Primary), act)
        .boxed()
}

/// A button that refuses, stops or throws away, by running `act`.
pub fn bad_action(
    which: Glyph,
    text: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    acting(which, text, (danger_button, Variant::Danger), act).boxed()
}

/// A row of a list that leads somewhere: no fill and no border at
/// rest, and a faint fill under the pointer.
pub fn link_row(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, _| f.fill(Color::NONE).radius(0.0))
        .label(|l, t| {
            l.tone(Tone::Faint).size(t.text.small).bold(true)
        })
        .icon(|i, _| i.tone(Tone::Faint))
        .hovered(|s| {
            s.frame(|f, t| f.fill(t.palette.button)).tone(Tone::Body)
        })
        .pressed(|s| {
            s.frame(|f, t| f.fill(t.palette.input)).tone(Tone::Dim)
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// A card that leads into a part of something: its name under
/// `which`, what it holds and a mark that it opens. `act` takes it.
pub fn nav_card(
    which: Glyph,
    title: &'static str,
    holds: impl Into<Prop<String>>,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    nav_item(which, title, None, holds.into(), button, act).boxed()
}

/// A [`nav_card`] for the way most should take: outlined in gold,
/// with `tag` as a [`pill`] after its name.
pub fn lead_card(
    which: Glyph,
    title: &'static str,
    tag: &'static str,
    holds: impl Into<Prop<String>>,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    nav_item(which, title, Some(tag), holds.into(), lead_button, act)
        .boxed()
}

/// A [`button`] outlined in gold.
fn lead_button(cx: &mut Cx) {
    style::<HamletTheme>()
        .with(button)
        .frame(|f, t| f.border_color(t.palette.gold_4))
        .hovered(|s| s.frame(|f, t| f.border_color(t.palette.accent)))
        .apply(cx);
}

/// The same as a row of a list, with no border about it.
pub fn nav_row(
    which: Glyph,
    title: &'static str,
    holds: impl Into<Prop<String>>,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    nav_item(which, title, None, holds.into(), link_row, act).boxed()
}

fn nav_item(
    which: Glyph,
    title: &'static str,
    tag: Option<&'static str>,
    holds: Prop<String>,
    rules: fn(&mut Cx),
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    with_cx(move |cx| {
        let space = &cx.theme().space;
        views::button(
            row((
                column((
                    row((
                        headed(
                            which,
                            label(title).wrap(false).rules(
                                |cx: &mut Cx| {
                                    style::<HamletTheme>()
                                        .label(|l, t| {
                                            l.size(t.text.heading)
                                                .bold(true)
                                        })
                                        .apply(cx);
                                },
                            ),
                        ),
                        match tag {
                            Some(tag) => pill(tag).boxed(),
                            None => frame().boxed(),
                        },
                    ))
                    .gap(space.gap)
                    .align(AlignItems::Center),
                    description(holds),
                ))
                .gap(space.gap / 2.0)
                .grow(1.0)
                .min_width(px(0.0)),
                glyph(Glyph::Into, ICON).tone(Tone::Dim),
            ))
            .gap(space.section)
            .grow(1.0)
            .align(AlignItems::Center),
        )
        .padding(UiRect::axes(
            px(space.card_padding_x),
            px(space.card_padding_y),
        ))
        .rules(rules)
        .tagged(Variant::Default)
        .on_activate(act)
    })
}

/// The parts of something as rows of one list, a hairline between
/// each and the next.
pub fn nav_list(cx: &Cx, items: Vec<AnyView>) -> impl View + use<> {
    let mut rows = Vec::new();
    for item in items {
        if !rows.is_empty() {
            rows.push(hairline(cx.theme().palette.gold_6).boxed());
        }
        rows.push(item);
    }
    column(rows).gap(0.0)
}

/// The head of a page: `path`, its [`breadcrumb`], over `name`, its
/// [`title`].
pub fn page_head(path: impl View, name: impl View) -> impl View {
    with_cx(move |cx| {
        column((path, name))
            .gap(cx.theme().space.gap)
            .align(AlignItems::FlexStart)
    })
}

/// Where a page is within a panel that has pages under it: each page
/// above it, from the top, as a [`crumb`], a [`crumb_slash`] after
/// each, and then `current`, the page shown, as a [`crumb_here`]. A
/// press hands `go` the place in `trail` of its page.
pub fn breadcrumb(
    trail: Vec<String>,
    current: impl Into<Prop<String>>,
    go: impl Fn(&mut World, usize) + Clone + Send + Sync + 'static,
) -> impl View {
    let mut parts = Vec::new();
    for (at, text) in trail.into_iter().enumerate() {
        let go = go.clone();
        parts.push(crumb(text, move |world| go(world, at)).boxed());
        parts.push(crumb_slash().boxed());
    }
    parts.push(crumb_here(current).boxed());
    crumbs(parts)
}

/// The parts of a breadcrumb in a row, wrapping in a narrow panel.
pub fn crumbs(parts: Vec<AnyView>) -> impl View {
    with_cx(move |cx| {
        row(parts)
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center)
            .tagged(Wraps)
    })
}

/// A page above the one shown in a breadcrumb, which `act` turns to.
pub fn crumb(
    text: impl Into<Prop<String>>,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    views::button(label(text).wrap(false))
        // Bare, so that the path starts at the panel's edge and its
        // parts sit evenly apart.
        .padding(UiRect::all(px(0.0)))
        .rules(back_link)
        .tagged(Variant::Text)
        .on_activate(act)
}

/// The dim slash between two parts of a breadcrumb.
pub fn crumb_slash() -> impl View {
    label("/").wrap(false).rules(note_rules)
}

/// The page shown, at the end of a breadcrumb.
pub fn crumb_here(text: impl Into<Prop<String>>) -> impl View {
    label(text).wrap(false).rules(|cx: &mut Cx| {
        style::<HamletTheme>()
            .label(|l, t| {
                l.size(t.text.small).bold(true).tone(Tone::Body)
            })
            .apply(cx);
    })
}

/// A card that is pressed whole: its border brightens under the
/// pointer.
pub fn card_button(cx: &mut Cx) {
    card_choice(false)(cx);
}

/// A [`card_button`] that is one answer of several, its border lit
/// while it is the one picked.
pub fn card_choice(
    picked: bool,
) -> impl FnOnce(&mut Cx) + Send + Sync + 'static {
    move |cx| {
        style::<HamletTheme>()
            .frame(move |f, t| {
                f.fill(t.palette.card)
                    .border(t.space.border)
                    .border_color(if picked {
                        t.palette.accent
                    } else {
                        t.palette.gold_6
                    })
                    .radius(t.space.round)
                    .justify(JustifyContent::FlexStart)
                    .align(AlignItems::FlexStart)
                    .padding(UiRect::axes(
                        px(t.space.card_padding_x),
                        px(t.space.card_padding_y),
                    ))
            })
            .hovered(|s| {
                s.frame(move |f, t| {
                    f.fill(t.palette.card).border_color(if picked {
                        t.palette.accent
                    } else {
                        t.palette.gold_4
                    })
                })
            })
            .pressed(|s| {
                s.frame(|f, t| {
                    f.fill(t.palette.card)
                        .border_color(t.palette.gold_5)
                })
            })
            .transition(Motion::Interact)
            .apply(cx);
    }
}

/// A ring, filled while it is the one chosen, as a radio button's.
pub fn radio(on: bool) -> impl View {
    with_cx(move |cx| {
        let palette = &cx.theme().palette;
        let dot = frame()
            .width(px(6.0))
            .height(px(6.0))
            .radius(3.0)
            .fill(if on { palette.accent } else { Color::NONE });
        column((dot,))
            .width(px(14.0))
            .height(px(14.0))
            .radius(7.0)
            .border(1.5)
            .border_color(if on {
                palette.accent
            } else {
                palette.gold_5
            })
            .align(AlignItems::Center)
            .justify(JustifyContent::Center)
    })
}

/// A word that marks a state, such as "In use" or "Default": small
/// amber letters on a filled amber plate with no border, so it does
/// not read as a button.
pub fn pill(text: impl Into<Prop<String>>) -> impl View {
    let text = text.into();
    with_cx(move |cx| {
        let theme = cx.theme();
        row((label(text)
            .size(theme.text.small)
            .bold(true)
            .tone(Tone::Accent)
            .wrap(false),))
        .padding(UiRect::axes(px(8.0), px(2.0)))
        .shrink(0.0)
        .fill(theme.palette.primary_low)
        .radius(theme.space.round)
    })
}
