//! The looks of the buttons and fields, and the view shapes the
//! panels share.

use bevy::color::Color;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, Checkbox, Frame, FrameProps, Icon, Label, Stack,
    button, checkbox, column, frame, label, row,
};
use bevy_fynix::{Prop, ScopedExt, ViewExt, Visual, style};

use crate::feel::Mood;
use crate::icons::{Glyph, glyph};
use crate::platform::{DisplayText, Wraps};
use crate::theme::HamletTheme;
use crate::{AnyView, Cx, View};

/// The least height of any button (macOS default control size, 28
/// pt).
pub(crate) const CONTROL: f32 = 28.0;

/// The padding inside a text button.
pub fn pad() -> UiRect {
    UiRect::axes(px(14.0), px(0.0))
}

/// An ordinary button: a wooden plate in a brass frame, lettered in
/// brass. A held one sinks into the dark.
pub fn brass(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.grey_4)
                .border(1.0)
                .border_color(t.palette.brass_5)
                .radius(t.space.round)
        })
        .label(|l, _| l.tone(Tone::Faint).size(12.0).bold(true))
        .icon(|i, _| i.tone(Tone::Faint))
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.brass_hover)
                    .border_color(t.palette.brass_3)
            })
            .tone(Tone::Body)
        })
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.well).border_color(t.palette.brass_5)
            })
            .tone(Tone::Dim)
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// The button for the one action a screen is for: a lamp that burns
/// low behind amber glass, and is turned up under the pointer.
pub fn lamp(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.lamp_low)
                .border(1.0)
                .border_color(t.palette.brass_4)
                .radius(t.space.round)
        })
        .label(|l, _| l.tone(Tone::Body).size(12.0).bold(true))
        .icon(|i, _| i.tone(Tone::Body))
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.lamp_low)
                    .border_color(t.palette.lamp)
            })
        })
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.lamp_low)
                    .border_color(t.palette.brass_5)
            })
            .tone(Tone::Faint)
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// The way back to where a page was reached from: an arrow and a name
/// with no plate, dim until the pointer is on them.
pub fn retrace(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, _| f.fill(Color::NONE))
        .label(|l, _| l.tone(Tone::Dim).size(12.0).bold(true))
        .icon(|i, _| i.tone(Tone::Dim))
        .hovered(|s| s.fill(|_| Color::NONE).tone(Tone::Accent))
        .pressed(|s| s.fill(|_| Color::NONE).tone(Tone::Faint))
        .transition(Motion::Interact)
        .apply(cx);
}

/// A view before it comes in and after it goes: clear, and a little
/// small.
pub fn away(cx: &mut Cx) {
    cx.set::<Visual>(|visual, _| visual.opacity(0.0));
    cx.root(|cx| cx.set::<Visual>(|visual, _| visual.scale(0.96)));
}

/// A text field: a well sunk into the panel, framed in dim brass that
/// brightens under the pointer and while it is typed in.
pub fn well(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.well)
                .border(1.0)
                .border_color(t.palette.brass_6)
                .radius(t.space.round)
        })
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.well).border_color(t.palette.brass_5)
            })
        })
        .when::<bevy_fynix::Focused>(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.well).border_color(t.palette.brass_4)
            })
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// A button for what refuses, stops or throws away: a dark red plate
/// in a red frame, and a coal under the pointer.
pub fn grim(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.red_deep)
                .border(1.0)
                .border_color(t.palette.red_low)
                .radius(t.space.round)
        })
        .label(|l, _| l.tone(Tone::Critical).size(12.0).bold(true))
        .icon(|i, _| i.tone(Tone::Critical))
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.red_hover)
                    .border_color(t.palette.red)
            })
            .tone(Tone::Body)
        })
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.well).border_color(t.palette.red)
            })
            .tone(Tone::Critical)
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// The room left over along a row or a column, taken and given back
/// when there is less than what is inside would like.
pub(crate) trait Fills: FrameProps + Sized {
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
pub(crate) fn filling() -> Stack<()> {
    column(()).fills_height().gap(0.0)
}

/// A line one pixel thick, of `colour`.
pub(crate) fn hairline(colour: impl Into<Prop<Color>>) -> Frame {
    frame().height(px(1.0)).fill(colour)
}

/// Small dim words.
pub fn note(text: impl Into<Prop<String>>) -> Label {
    label(text).size(12.0).tone(Tone::Dim)
}

/// The name at the head of a page.
pub fn title(text: impl Into<Prop<String>>) -> impl View {
    label(text)
        .size(16.0)
        .bold(true)
        .wrap(false)
        .tagged(DisplayText)
}

/// A checkbox as large as a button is tall.
pub fn tick(on: impl Into<Prop<bool>>) -> Checkbox {
    checkbox(on).width(px(CONTROL)).height(px(CONTROL))
}

/// A lettered button: `text` in the display face.
pub(crate) fn lettered(
    text: impl Into<Prop<String>>,
) -> impl View + FrameProps {
    button(label(text).wrap(false).tagged(DisplayText))
        .padding(pad())
        .min_height(px(CONTROL))
}

/// The side of an icon set beside lettering.
pub const ICON: f32 = 14.0;

/// An icon set a pixel high, on the middle of the display face's
/// capitals beside it.
pub fn raised(mark: Icon) -> impl View + FrameProps {
    row((mark,)).padding(UiRect::bottom(px(2.0)))
}

/// An icon before `words` in the display face.
pub(crate) fn headed(
    which: Glyph,
    words: impl View + 'static,
) -> impl View + FrameProps {
    row((raised(glyph(which, ICON)), words))
        .gap(6.0)
        .align(AlignItems::Center)
}

/// A lettered button with an icon before its word.
pub fn marked(
    which: Glyph,
    text: &'static str,
) -> impl View + FrameProps {
    button(headed(which, label(text).wrap(false).tagged(DisplayText)))
        // Less on the icon's side, which has air of its own.
        .padding(UiRect::new(px(10.0), px(13.0), px(0.0), px(0.0)))
        .min_height(px(CONTROL))
}

/// A word of status of `size`, behind its icon, both in `tone`.
pub(crate) fn stated(
    which: Glyph,
    text: impl Into<String>,
    tone: Tone,
    size: f32,
) -> impl View {
    row((
        glyph(which, size).tone(tone),
        label(text.into()).size(size).tone(tone).wrap(false),
    ))
    .gap(5.0)
    .align(AlignItems::Center)
}

/// Buttons in a row that wraps: in a narrow panel they go on to a
/// line below and are never cut off at its edge.
pub fn buttons(buttons: Vec<AnyView>) -> impl View {
    row(buttons)
        .gap(8.0)
        .align(AlignItems::Center)
        .tagged(Wraps)
}

/// A brass button that runs `act`.
pub fn command(text: &'static str, act: fn(&mut World)) -> impl View {
    lettered(text)
        .rules(brass)
        .tagged(Mood::Plain)
        .on_activate(act)
}

/// The way back to `text`, which `act` takes.
pub fn back(text: &'static str, act: fn(&mut World)) -> impl View {
    marked(Glyph::Back, text)
        .rules(retrace)
        .tagged(Mood::Ink)
        .on_activate(act)
}

/// A button with an icon before its word, of the look `rules` and
/// the feel of `mood`, that runs `act`.
fn acting(
    which: Glyph,
    text: &'static str,
    (rules, mood): (fn(&mut Cx), Mood),
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    marked(which, text)
        .rules(rules)
        .tagged(mood)
        .on_activate(act)
}

/// A brass button with an icon before its word, that runs `act`.
pub(crate) fn action(
    which: Glyph,
    text: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    acting(which, text, (brass, Mood::Plain), act).boxed()
}

/// The button for the one action a screen is for, that runs `act`.
pub(crate) fn good_action(
    which: Glyph,
    text: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    acting(which, text, (lamp, Mood::Good), act).boxed()
}

/// A button that refuses, stops or throws away, by running `act`.
pub(crate) fn bad_action(
    which: Glyph,
    text: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    acting(which, text, (grim, Mood::Bad), act).boxed()
}

/// A row of a list that leads somewhere: no plate and no frame at
/// rest, and a faint plate under the pointer.
pub(crate) fn passage(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| f.fill(Color::NONE).radius(t.space.round))
        .label(|l, _| l.tone(Tone::Faint).size(12.0).bold(true))
        .icon(|i, _| i.tone(Tone::Faint))
        .hovered(|s| {
            s.frame(|f, t| f.fill(t.palette.grey_4)).tone(Tone::Body)
        })
        .pressed(|s| {
            s.frame(|f, t| f.fill(t.palette.well)).tone(Tone::Dim)
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// The way into a part of something, as a framed plate: its name
/// under `which`, what it holds and a mark that it opens. `act`
/// takes it.
pub fn door(
    which: Glyph,
    title: &'static str,
    holds: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    way_in(which, title, holds, brass, act).boxed()
}

/// The same as a row of a list, with no frame about it.
pub(crate) fn list_door(
    which: Glyph,
    title: &'static str,
    holds: &'static str,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    way_in(which, title, holds, passage, act).boxed()
}

fn way_in(
    which: Glyph,
    title: &'static str,
    holds: &'static str,
    look: fn(&mut Cx),
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    button(
        row((
            column((
                headed(
                    which,
                    label(title).bold(true).tagged(DisplayText),
                ),
                label(holds).size(12.0).bold(false).tone(Tone::Dim),
            ))
            .gap(4.0)
            .grow(1.0)
            .min_width(px(0.0)),
            glyph(Glyph::Into, ICON).tone(Tone::Dim),
        ))
        .gap(12.0)
        .grow(1.0)
        .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(14.0), px(10.0)))
    .rules(look)
    .tagged(Mood::Plain)
    .on_activate(act)
}

/// The parts of something as rows of one list, a hairline between
/// each and the next, over `foot`.
pub(crate) fn doors(
    cx: &Cx,
    doors: Vec<AnyView>,
    foot: AnyView,
) -> impl View + use<> {
    let mut rows = Vec::new();
    for door in doors {
        if !rows.is_empty() {
            rows.push(hairline(cx.theme().palette.brass_6).boxed());
        }
        rows.push(door);
    }
    rows.push(foot);
    column(rows).gap(0.0)
}

/// The head of a page of its own: the way back to `front`, which
/// `act` takes, and the page's `name` under `which`.
pub(crate) fn page_head(
    front: &'static str,
    act: fn(&mut World),
    which: Glyph,
    name: &'static str,
) -> impl View {
    row((
        back(front, act),
        headed(
            which,
            label(name)
                .bold(true)
                .tone(Tone::Faint)
                .tagged(DisplayText),
        ),
    ))
    .gap(12.0)
    .align(AlignItems::Center)
}

/// A row that is pressed whole: a plate under the pointer.
pub(crate) fn plate(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(Color::NONE)
                .radius(t.space.round)
                .justify(JustifyContent::FlexStart)
                .padding(UiRect::axes(px(10.0), px(5.0)))
        })
        .hovered(|s| s.fill(|t| t.palette.brass_hover))
        .pressed(|s| s.fill(|t| t.palette.black))
        .apply(cx);
}
