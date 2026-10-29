//! The looks of the buttons and fields, and the view shapes the
//! panels share.

use bevy::color::Color;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, Frame, FrameProps, Icon, Stack, button, column,
    frame, label, row, scroll, tinted_icon,
};
use bevy_fynix::{
    Bevy, Prop, ScopedExt, ViewExt, ViewSeq, Visual, style,
};

use crate::fade::{FadeSurface, ScrollFade};
use crate::feel::Mood;
use crate::icons::{Glyph, glyph};
use crate::platform::{DisplayText, Wraps};
use crate::theme::{HamletTheme, TextSizes};
use crate::{AnyView, Cx, View, with_cx};

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
                .border(t.space.border)
                .border_color(t.palette.brass_5)
                .radius(t.space.round)
        })
        .label(|l, t| {
            l.tone(Tone::Faint).size(t.text.small).bold(true)
        })
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
                .border(t.space.border)
                .border_color(t.palette.brass_4)
                .radius(t.space.round)
        })
        .label(|l, t| {
            l.tone(Tone::Body).size(t.text.small).bold(true)
        })
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
        .label(|l, t| l.tone(Tone::Dim).size(t.text.small).bold(true))
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
                .border(t.space.border)
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
                .border(t.space.border)
                .border_color(t.palette.red_low)
                .radius(t.space.round)
        })
        .label(|l, t| {
            l.tone(Tone::Critical).size(t.text.small).bold(true)
        })
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
        let line = cx.theme().palette.brass_6;
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

/// Small dim words, too short to be a description: a value, or
/// what an empty list says.
pub fn note(text: impl Into<Prop<String>>) -> impl View {
    label(text).rules(note_rules)
}

/// The rules of a note: every label in the view small and dim.
pub fn note_rules(cx: &mut Cx) {
    style::<HamletTheme>()
        .label(|l, t| l.size(t.text.small).tone(Tone::Dim))
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

/// The name of a part of a panel: small, in the display face.
pub fn section(text: impl Into<Prop<String>>) -> impl View {
    label(text)
        .rules(|cx: &mut Cx| {
            style::<HamletTheme>()
                .label(|l, t| {
                    l.size(t.text.small).bold(true).tone(Tone::Faint)
                })
                .apply(cx);
        })
        .tagged(DisplayText)
}

/// The name on a card, in `tone`: heading size, in the display face.
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
            .tagged(DisplayText)
    })
}

/// The line over a card that waits for the person: small capitals in
/// `tone`.
pub fn caption(
    text: impl Into<Prop<String>>,
    tone: Tone,
) -> impl View {
    let text = text.into();
    with_cx(move |cx| {
        label(text)
            .size(cx.theme().text.caption)
            .bold(true)
            .tone(tone)
            .tagged(DisplayText)
    })
}

/// `children` in a column that scrolls, taking the height left. Its
/// edges fade into the panel while more lies past them.
pub fn scroll_area(
    children: impl ViewSeq<Bevy, HamletTheme> + 'static,
) -> impl View {
    scroll_area_on(FadeSurface::default(), children)
}

/// A [`scroll_area`] on `surface`, its edges fading into it.
pub fn scroll_area_on(
    surface: FadeSurface,
    children: impl ViewSeq<Bevy, HamletTheme> + 'static,
) -> impl View {
    with_cx(move |cx| {
        scroll(children)
            .gap(cx.theme().space.gap)
            .fills_height()
            .tagged(ScrollFade { surface })
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
            .border_color(theme.palette.brass_6)
            .radius(space.round)
            .fill(theme.palette.card)
    })
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

/// A word of status in the small size, behind its icon, both in
/// `tone`.
pub(crate) fn stated(
    which: Glyph,
    text: impl Into<String>,
    tone: Tone,
) -> impl View {
    stated_at(which, text.into(), tone, |sizes| sizes.small)
}

/// A word of status in the body size, behind its icon, both in
/// `tone`.
pub(crate) fn stated_large(
    which: Glyph,
    text: impl Into<String>,
    tone: Tone,
) -> impl View {
    stated_at(which, text.into(), tone, |sizes| sizes.body)
}

fn stated_at(
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

/// One answer of several to pick from, each showing `labels`, the
/// one at `picked` lit. A press hands the place of its answer to
/// `pick`.
pub fn choices<F>(
    labels: impl IntoIterator<Item = &'static str>,
    picked: usize,
    pick: F,
) -> impl View
where
    F: Fn(&mut World, usize) + Clone + Send + Sync + 'static,
{
    let options = labels
        .into_iter()
        .enumerate()
        .map(|(at, text)| {
            let pick = pick.clone();
            button(label(text).wrap(false))
                .rules(choice(at == picked))
                .on_activate(move |world| pick(world, at))
                .boxed()
        })
        .collect::<Vec<_>>();
    with_cx(move |cx| {
        row(options)
            .gap(cx.theme().space.gap / 2.0)
            .align(AlignItems::Center)
            .tagged(Wraps)
    })
}

/// The rules of an answer to pick: bare until the pointer is on it,
/// and the one picked a lit plate in a frame.
pub(crate) fn choice(
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
            f.fill(t.palette.lamp_low).border_color(t.palette.brass_4)
        })
        .tone(Tone::Body)
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.lamp_low)
                    .border_color(t.palette.lamp)
            })
        })
    } else {
        base.frame(|f, _| {
            f.fill(Color::NONE).border_color(Color::NONE)
        })
        .tone(Tone::Faint)
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.wood_hover).border_color(Color::NONE)
            })
            .tone(Tone::Body)
        })
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.well).border_color(Color::NONE)
            })
        })
    };
    rules.transition(Motion::Interact).bundle()
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

/// A small "x" that closes or drops something by running `act`: dim,
/// and red under the pointer, as a dock tab's is.
pub(crate) fn close_mark(
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    tinted_icon(Glyph::Cross.image(ICON))
        .hover(Tone::Critical)
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
        .frame(|f, _| f.fill(Color::NONE).radius(0.0))
        .label(|l, t| {
            l.tone(Tone::Faint).size(t.text.small).bold(true)
        })
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
    with_cx(move |cx| {
        let space = &cx.theme().space;
        button(
            row((
                column((
                    headed(
                        which,
                        label(title)
                            .bold(true)
                            .wrap(false)
                            .tagged(DisplayText),
                    ),
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
        .rules(look)
        .tagged(Mood::Plain)
        .on_activate(act)
    })
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

/// Where a page is within a panel that has pages under it: each page
/// above it, from the top, as a bare button, a dim `/` after each,
/// and then `current`, the page shown, which is not a button. A press
/// hands `go` the place in `trail` of its page.
pub fn breadcrumb(
    trail: Vec<String>,
    current: impl Into<Prop<String>>,
    go: impl Fn(&mut World, usize) + Clone + Send + Sync + 'static,
) -> impl View {
    let current = current.into();
    with_cx(move |cx| {
        let size = cx.theme().text.small;
        let mut parts = Vec::new();
        for (at, text) in trail.into_iter().enumerate() {
            let go = go.clone();
            parts.push(
                button(label(text).wrap(false).tagged(DisplayText))
                    // Bare, so that the path starts at the panel's
                    // edge and its parts sit evenly apart.
                    .padding(UiRect::all(px(0.0)))
                    .rules(retrace)
                    .tagged(Mood::Ink)
                    .on_activate(move |world| go(world, at))
                    .boxed(),
            );
            parts.push(
                label("/")
                    .size(size)
                    .tone(Tone::Dim)
                    .wrap(false)
                    .tagged(DisplayText)
                    .boxed(),
            );
        }
        parts.push(
            label(current)
                .size(size)
                .bold(true)
                .tone(Tone::Faint)
                .wrap(false)
                .tagged(DisplayText)
                .boxed(),
        );
        row(parts)
            .gap(cx.theme().space.gap)
            .align(AlignItems::Center)
            .tagged(Wraps)
    })
}

/// A card that is pressed whole: its frame lights under the pointer.
pub(crate) fn card_button(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.card)
                .border(t.space.border)
                .border_color(t.palette.brass_6)
                .radius(t.space.round)
                .justify(JustifyContent::FlexStart)
                .align(AlignItems::FlexStart)
                .padding(UiRect::axes(
                    px(t.space.card_padding_x),
                    px(t.space.card_padding_y),
                ))
        })
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.card).border_color(t.palette.brass_4)
            })
        })
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.card).border_color(t.palette.brass_5)
            })
        })
        .transition(Motion::Interact)
        .apply(cx);
}
