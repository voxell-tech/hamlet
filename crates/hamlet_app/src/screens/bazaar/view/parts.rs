//! What the stalls are made of, each as Hamlet's own panels make it:
//! a shelf headed as the Inspector heads its list of hamlets, a
//! listing's row as a list's row that leads somewhere, and a
//! listing's page as Settings draws a page of its own, its facts in a
//! list and its buttons in a row.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, label, row,
};
use bevy_fynix::{Prop, ScopedExt, ViewExt};
use hamlet_ui::styles::{
    Fills, ICON, back, buttons, hairline, link_row, note, note_rules,
    title,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::DisplayText;
use hamlet_ui::{AnyView, Cx, View};

/// The most characters of a page's title, which does not wrap.
const TITLE_MOST: usize = 28;

/// The most characters of what a listing is for, on its row.
const PURPOSE_MOST: usize = 64;

/// `text` cut to `most` characters, with an ellipsis where it was.
pub(crate) fn short(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }
    let cut = text
        .chars()
        .take(most.saturating_sub(1))
        .collect::<String>();
    format!("{}...", cut.trim_end())
}

/// The head of a shelf: the stall's name, and what it sells beside
/// it.
pub(crate) fn shelf_head(
    name: &'static str,
    about: &'static str,
) -> AnyView {
    row((title(name), note(about)))
        .gap(12.0)
        .align(AlignItems::Center)
        .boxed()
}

/// A listing's row: its name with its price at the end, dim lines
/// under it, and the accent line of what is already the chief's.
pub(crate) struct Line {
    pub name: String,
    pub price: String,
    /// What it is for, cut to fit, if anything.
    pub purpose: String,
    /// Its standing, its maker and the like.
    pub meta: String,
    /// The accent line, which shows only while it is the chief's.
    pub mark: Option<AnyView>,
}

impl Line {
    /// The row, which runs `open` when pressed. The price is kept to
    /// one line.
    pub fn view(
        self,
        open: impl Fn(&mut World) + Send + Sync + 'static,
    ) -> AnyView {
        let mut parts = vec![
            row((
                column((label(self.name)
                    .size(13.0)
                    .bold(true)
                    .tone(Tone::Body),))
                .fills_width(),
                label(self.price)
                    .size(12.0)
                    .bold(true)
                    .tone(Tone::Faint)
                    .wrap(false),
            ))
            .gap(12.0)
            .align(AlignItems::FlexStart)
            .boxed(),
        ];
        let lines = [short(&self.purpose, PURPOSE_MOST), self.meta];
        parts.extend(
            lines
                .into_iter()
                .filter(|line| !line.is_empty())
                // A label in a button is bold unless it says so.
                .map(|line| {
                    label(line).bold(false).rules(note_rules).boxed()
                }),
        );
        parts.extend(self.mark);
        button(
            row((
                column(parts).gap(3.0).fills_width(),
                glyph(Glyph::Into, ICON).tone(Tone::Dim),
            ))
            .gap(12.0)
            .grow(1.0)
            .align(AlignItems::Center),
        )
        .padding(UiRect::axes(px(14.0), px(10.0)))
        .rules(link_row)
        .tagged(Variant::Default)
        .on_activate(open)
        .boxed()
    }
}

/// The accent line of what is the chief's.
pub(crate) fn mark(words: &str) -> AnyView {
    label(words.to_string())
        .size(12.0)
        .bold(false)
        .tone(Tone::Accent)
        .boxed()
}

/// The head of a listing's page: its name, what it is, its facts and
/// the accent line of what is the chief's.
pub(crate) fn page_head(
    name: &str,
    about: AnyView,
    facts: String,
    mark: Option<AnyView>,
) -> AnyView {
    let mut parts =
        vec![title(short(name, TITLE_MOST)).boxed(), about];
    if !facts.is_empty() {
        parts.push(note(facts).boxed());
    }
    parts.extend(mark);
    column(parts).gap(4.0).boxed()
}

/// How a listing is had: its price, as a fact under a head, its
/// buttons in a row, the first the one that has it, then what they
/// do and what came of them.
pub(crate) fn deal(
    price: Option<String>,
    pressed: Vec<AnyView>,
    notes: Vec<AnyView>,
) -> AnyView {
    let mut parts = Vec::new();
    parts.extend(
        price.map(|price| label(price).size(13.0).bold(true).boxed()),
    );
    if !pressed.is_empty() {
        parts.push(buttons(pressed).boxed());
    }
    parts.extend(notes);
    column(parts).gap(6.0).boxed()
}

/// The top of a page: its head, and under it `rest`, such as the
/// way it is had.
pub(crate) fn top(parts: Vec<AnyView>) -> AnyView {
    column(parts).gap(12.0).boxed()
}

/// A part of a page: its name, as Settings names a field, over
/// `body`.
pub(crate) fn section(name: &'static str, body: AnyView) -> AnyView {
    column((
        label(name)
            .size(12.0)
            .bold(true)
            .tone(Tone::Faint)
            .tagged(DisplayText),
        body,
    ))
    .gap(6.0)
    .boxed()
}

/// The parts of a page, apart.
pub(crate) fn sections(parts: Vec<AnyView>) -> AnyView {
    column(parts).gap(20.0).boxed()
}

/// `rows` as a list, a hairline between each and the next.
pub(crate) fn rows(rows: Vec<AnyView>) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let line = cx.theme().palette.gold_6;
        let mut all = Vec::new();
        for one in rows {
            if !all.is_empty() {
                all.push(hairline(line).boxed());
            }
            all.push(
                column((one,))
                    .padding(UiRect::vertical(px(8.0)))
                    .boxed(),
            );
        }
        cx.build(column(all).gap(0.0))
    })
}

/// A fact: its name, and what it is at the end of its line.
pub(crate) fn fact(
    name: impl Into<String>,
    value: impl Into<String>,
) -> AnyView {
    row((
        column((label(name.into()).size(13.0),)).fills_width(),
        label(value.into()).size(12.0).tone(Tone::Dim),
    ))
    .gap(12.0)
    .align(AlignItems::Center)
    .boxed()
}

/// A fact with dim words under it, such as an agent and what it does.
pub(crate) fn told(
    fact: AnyView,
    about: impl Into<String>,
) -> AnyView {
    let about = about.into();
    if about.is_empty() {
        return fact;
    }
    column((fact, note(about))).gap(3.0).boxed()
}

/// Why something could not be done, small and in red, as Settings
/// says it under its controls.
pub(crate) fn problem(words: impl Into<Prop<String>>) -> AnyView {
    label(words).size(12.0).tone(Tone::Critical).boxed()
}

/// What came of something done: why, in red, where it went wrong.
pub(crate) fn outcome(words: String, wrong: bool) -> AnyView {
    match wrong {
        true => problem(words),
        false => mark(&words),
    }
}

/// Something that waits for the chief, as the Needs You panel shows
/// one: its caption, what it is about and its answers, in a card.
pub(crate) fn waiting(
    caption: &'static str,
    about: &'static str,
    answers: Vec<AnyView>,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (edge, round, fill) = (
            theme.palette.gold_6,
            theme.space.round,
            theme.palette.card,
        );
        cx.build(
            column((
                label(caption)
                    .size(11.0)
                    .bold(true)
                    .tone(Tone::Accent)
                    .tagged(DisplayText),
                label(about).size(13.0),
                buttons(answers),
            ))
            .gap(6.0)
            .padding(UiRect::axes(px(14.0), px(10.0)))
            .border(1.0)
            .border_color(edge)
            .radius(round)
            .fill(fill),
        )
    })
}

/// `control`, such as a dropdown, as wide as the part it is in, as
/// the shelf lays its order and filters.
pub(crate) fn wide(control: impl View + 'static) -> AnyView {
    column((control,))
        .align(AlignItems::Stretch)
        .width(percent(100.0))
        .boxed()
}

/// A view of nothing, where a part has nothing to show.
pub(crate) fn nothing() -> AnyView {
    column(()).gap(0.0).boxed()
}

/// The way back to the shelf `front`, which `act` takes, and the
/// name of the page open over it.
pub(crate) fn crumb(
    front: &'static str,
    act: fn(&mut World),
    name: String,
) -> impl View {
    row((
        back(front, act),
        label(name)
            .size(12.0)
            .bold(true)
            .tone(Tone::Faint)
            .wrap(false)
            .tagged(DisplayText),
    ))
    .gap(12.0)
    .align(AlignItems::Center)
}
