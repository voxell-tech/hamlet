//! The bazaar's house style: the pieces every stall's shelf and
//! pages are made of, so that each reads the same. Words are in the
//! body face at three steps, content, meta and the accent of a price;
//! the display face is kept for a page's title.

use agents_ui::{name, short};
use hamlet_engine_bevy::prelude::World;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Face, Fill, FrameProps, Label,
    Prop, Size, Space, Tone, UiExt, ViewExt, column, icon, label,
    pressable, row,
};

/// The most characters of what a listing is for, on its row: two
/// lines at the narrowest the panel is.
const PURPOSE_MOST: usize = 60;

/// The most characters of a page's title, which does not wrap.
const TITLE_MOST: usize = 28;

/// The controls laid on one line.
const CONTROLS_A_LINE: usize = 2;

/// A listing's row on a shelf: its cover, its name over what it is
/// for and its meta line, and its price at the end.
pub(crate) struct Line {
    /// The icon in the place of its cover.
    pub cover: &'static str,
    pub name: String,
    /// What it is for, cut to two lines at most.
    pub purpose: String,
    /// Its standing, maker and the like.
    pub meta: String,
    pub cost: Cost,
    /// What says it is already the chief's, beside its name.
    pub mark: Option<AnyView>,
}

/// A price as it is shown: the amount, and what it is for under it.
/// On a row each is one word, as the end of a row wraps at any space.
#[derive(Clone)]
pub(crate) struct Cost {
    pub amount: String,
    /// Empty when there is nothing to say under the amount.
    pub per: String,
}

impl Cost {
    pub fn new(
        amount: impl Into<String>,
        per: impl Into<String>,
    ) -> Self {
        Cost {
            amount: amount.into(),
            per: per.into(),
        }
    }

    pub fn free() -> Self {
        Cost::new("Free", "")
    }
}

impl Line {
    /// The row, a button that runs `open`. A label in a button is
    /// bold and small unless it sets another size, so the steps here
    /// are of size and tone.
    pub fn view(
        self,
        open: impl Fn(&mut World) + Send + Sync + 'static,
    ) -> AnyView {
        let mut head =
            vec![label(self.name).size(Size::Heading).boxed()];
        head.extend(self.mark);
        let mut named = vec![row(head).space(Space::Half).boxed()];
        if !self.purpose.is_empty() {
            named.push(
                meta(short(&self.purpose, PURPOSE_MOST)).boxed(),
            );
        }
        let mut text = vec![column(named).space(Space::None).boxed()];
        if !self.meta.is_empty() {
            text.push(meta(self.meta).boxed());
        }
        pressable((
            cover(self.cover),
            column(text).space(Space::Half).grow(1.0),
            price(self.cost),
        ))
        .style(ButtonStyle::Row)
        .on_click(open)
        .boxed()
    }
}

/// The place of a listing's cover, holding the icon `which`.
fn cover(which: &'static str) -> AnyView {
    row((icon(which).tone(Tone::Faint),))
        .fill(Fill::Disc)
        .boxed()
}

/// Words about words: a listing's standing, a count, how a shelf
/// stands.
pub(crate) fn meta(text: impl Into<Prop<World, String>>) -> Label {
    label(text).size(Size::Small).tone(Tone::Dim)
}

/// A paragraph of what a page is about.
pub(crate) fn prose(text: impl Into<Prop<World, String>>) -> Label {
    label(text)
}

/// A paragraph that stands behind [`prose`], or a line of a section.
pub(crate) fn aside(text: impl Into<Prop<World, String>>) -> Label {
    prose(text).tone(Tone::Dim)
}

/// A short name over what is said of it, such as a provider or an
/// agent that comes with a hamlet.
pub(crate) fn named(
    title: impl Into<String>,
    about: String,
) -> AnyView {
    let mut parts = vec![prose(title.into()).bold(true).boxed()];
    if !about.is_empty() {
        parts.push(aside(about).boxed());
    }
    column(parts).space(Space::None).boxed()
}

/// A price at the end of a row: the amount in the accent, and what it
/// is for under it.
fn price(cost: Cost) -> AnyView {
    let mut parts =
        vec![label(cost.amount).tone(Tone::Accent).boxed()];
    if !cost.per.is_empty() {
        parts.push(meta(cost.per).boxed());
    }
    column(parts).space(Space::None).boxed()
}

/// What says a listing is already the chief's.
pub(crate) fn owned(words: &str) -> AnyView {
    row((icon("check").tone(Tone::Faint), meta(words.to_string())))
        .space(Space::Half)
        .boxed()
}

/// The head of a page: its title in the display face, cut to what
/// fits the panel, and the meta line `byline` under it.
pub(crate) fn page_head(title: &str, byline: String) -> AnyView {
    let shown = label(short(title, TITLE_MOST))
        .size(Size::Title)
        .bold(true)
        .face(Face::Display);
    let mut parts = vec![if title.chars().count() > TITLE_MOST {
        shown.tip(title.to_string()).boxed()
    } else {
        shown.boxed()
    }];
    if !byline.is_empty() {
        parts.push(meta(byline).boxed());
    }
    column(parts).space(Space::Half).boxed()
}

/// The one box of a page a listing is had from: its price, if it is
/// still to be had, then `actions`, each on a line of its own and the
/// primary first, then `notes` of what they do.
pub(crate) fn action_box(
    cost: Option<Cost>,
    actions: Vec<AnyView>,
    notes: Vec<AnyView>,
) -> AnyView {
    let mut parts = Vec::new();
    if let Some(cost) = cost {
        let mut amount = vec![
            label(cost.amount)
                .size(Size::Title)
                .bold(true)
                .tone(Tone::Accent)
                .boxed(),
        ];
        if !cost.per.is_empty() {
            amount.push(meta(cost.per).boxed());
        }
        parts.push(row(amount).space(Space::Half).boxed());
    }
    parts.extend(
        actions.into_iter().map(|action| row((action,)).boxed()),
    );
    parts.extend(notes);
    column(parts).fill(Fill::Card).space(Space::Gap).boxed()
}

/// A part of a page: its name in the theme's section heading, over
/// `body`.
pub(crate) fn section(text: &str, body: AnyView) -> AnyView {
    column((name(text.to_string()), body))
        .space(Space::Half)
        .boxed()
}

/// The parts of a page, room between each.
pub(crate) fn sections(parts: Vec<AnyView>) -> AnyView {
    column(parts).space(Space::Double).boxed()
}

/// Lines of a section, close together.
pub(crate) fn lines(parts: Vec<AnyView>) -> AnyView {
    column(parts).space(Space::Half).boxed()
}

/// `controls` laid a few to a line, so they fit a narrow panel.
pub(crate) fn controls(controls: Vec<AnyView>) -> AnyView {
    let mut lines = Vec::new();
    let mut line = Vec::new();
    for control in controls {
        line.push(control);
        if line.len() == CONTROLS_A_LINE {
            lines.push(
                row(std::mem::take(&mut line))
                    .space(Space::Half)
                    .boxed(),
            );
        }
    }
    if !line.is_empty() {
        lines.push(row(line).space(Space::Half).boxed());
    }
    column(lines).space(Space::Half).boxed()
}

/// The one quiet line over a shelf's rows: how it stands, and
/// `action` after it.
pub(crate) fn status_line(
    text: impl Into<Prop<World, String>>,
    action: Option<AnyView>,
) -> AnyView {
    let mut parts = vec![column((meta(text),)).grow(1.0).boxed()];
    parts.extend(action);
    row(parts).space(Space::Half).boxed()
}

/// What came of the last thing done: in red where it went wrong.
pub(crate) fn notice(
    words: impl Into<String>,
    wrong: bool,
) -> AnyView {
    prose(words.into())
        .tone(if wrong { Tone::Critical } else { Tone::Body })
        .boxed()
}
