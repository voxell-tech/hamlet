//! What the stalls are made of, each as Hamlet's own panels make it:
//! a shelf headed as the Inspector heads its list of hamlets, a
//! listing's row as the Plugins list draws a plugin's, and a
//! listing's page as the Plugins panel draws a plugin's page, its
//! buttons in a row as the Inspector lays a hamlet's.

pub(crate) use agents_ui::note;
use agents_ui::{link_row, name, short};
use hamlet_engine_bevy::prelude::World;
use hamlet_engine_bevy::ui::{
    AnyView, Face, Fill, FrameProps, Prop, Size, Space, Tone, UiExt,
    ViewExt, column, description, heading, label, list, row,
};

/// The most characters of a page's title, which does not wrap.
const TITLE_MOST: usize = 28;

/// The most characters of what a listing is for, on its row.
const PURPOSE_MOST: usize = 64;

/// The head of a shelf: the stall's name, and what it sells beside
/// it.
pub(crate) fn shelf_head(title: &str, about: &str) -> AnyView {
    row((heading(title.to_string()), description(about.to_string())))
        .space(Space::Gap)
        .boxed()
}

/// A listing's row: its name with its price at the end, where a
/// plugin's row has its switch, dim lines under it, and the accent
/// line of what is already the chief's.
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
    /// The row, which runs `open`. A label in a button is small,
    /// bold and in its tone unless it says otherwise, so the name
    /// and the price say. The price is kept to one line, as the end
    /// of a row wraps at any space.
    pub fn view(
        self,
        open: impl Fn(&mut World) + Send + Sync + 'static,
    ) -> AnyView {
        let mut parts = vec![
            row((
                column((label(self.name)
                    .size(Size::Body)
                    .bold(true)
                    .tone(Tone::Body),))
                .grow(1.0),
                label(self.price.replace(' ', "\u{a0}"))
                    .size(Size::Small)
                    .tone(Tone::Faint),
            ))
            .space(Space::Gap)
            .boxed(),
        ];
        let lines = [short(&self.purpose, PURPOSE_MOST), self.meta];
        parts.extend(
            lines
                .into_iter()
                .filter(|line| !line.is_empty())
                .map(|line| description(line).boxed()),
        );
        parts.extend(self.mark);
        link_row(column(parts).space(Space::Half).grow(1.0).boxed())
            .on_click(open)
            .boxed()
    }
}

/// The accent line of what is the chief's, as the Plugins list says
/// a plugin comes with Hamlet.
pub(crate) fn mark(words: &str) -> AnyView {
    label(words.to_string())
        .size(Size::Small)
        .bold(false)
        .tone(Tone::Accent)
        .boxed()
}

/// The head of a listing's page: its name, what it is, its facts and
/// the accent line of what is the chief's.
pub(crate) fn page_head(
    title: &str,
    about: AnyView,
    facts: String,
    mark: Option<AnyView>,
) -> AnyView {
    let shown = heading(short(title, TITLE_MOST));
    let mut parts = vec![if title.chars().count() > TITLE_MOST {
        shown.tip(title.to_string()).boxed()
    } else {
        shown.boxed()
    }];
    parts.push(about);
    if !facts.is_empty() {
        parts.push(description(facts).boxed());
    }
    parts.extend(mark);
    column(parts).space(Space::Half).boxed()
}

/// How a listing is had: its price, as a fact the Inspector shows
/// under a head, its buttons in a row, the first the one that has
/// it, then what they do and what came of them.
pub(crate) fn deal(
    price: Option<String>,
    buttons: Vec<AnyView>,
    notes: Vec<AnyView>,
) -> AnyView {
    let mut parts = Vec::new();
    parts.extend(price.map(|price| label(price).bold(true).boxed()));
    if !buttons.is_empty() {
        parts.push(row(buttons).space(Space::Half).boxed());
    }
    parts.extend(notes);
    column(parts).space(Space::Half).boxed()
}

/// The top of a page: its head, and under it `rest`, such as the
/// way it is had.
pub(crate) fn top(parts: Vec<AnyView>) -> AnyView {
    column(parts).space(Space::Gap).boxed()
}

/// A part of a page: its name, as the Plugins panel names one, over
/// `body`.
pub(crate) fn section(title: &str, body: AnyView) -> AnyView {
    column((name(title.to_string()), body))
        .space(Space::Half)
        .boxed()
}

/// The parts of a page, apart as the Plugins panel sets them.
pub(crate) fn sections(parts: Vec<AnyView>) -> AnyView {
    column(parts).space(Space::Double).boxed()
}

/// `rows` as a list, a divider between each.
pub(crate) fn rows(rows: Vec<AnyView>) -> AnyView {
    list(rows).boxed()
}

/// A fact: its name, and what it is at the end of its line, as the
/// Plugins panel lists what a plugin needs.
pub(crate) fn fact(
    name: impl Into<String>,
    value: impl Into<String>,
) -> AnyView {
    row((
        column((label(name.into()),)).grow(1.0),
        description(value.into()),
    ))
    .space(Space::Gap)
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
    column((fact, description(about)))
        .space(Space::Half)
        .boxed()
}

/// Why something could not be done, small and in red, as Settings
/// says it under the controls.
pub(crate) fn problem(
    words: impl Into<Prop<World, String>>,
) -> AnyView {
    label(words).size(Size::Small).tone(Tone::Critical).boxed()
}

/// What came of something done: why, in red, where it went wrong.
pub(crate) fn outcome(words: String, wrong: bool) -> AnyView {
    match wrong {
        true => problem(words),
        false => mark(&words),
    }
}

/// Something that waits for the chief, as Petitions shows one: its
/// caption, what it is about and its answers, in a card.
pub(crate) fn waiting(
    caption: &str,
    about: &str,
    answers: Vec<AnyView>,
) -> AnyView {
    column((
        label(caption.to_string())
            .size(Size::Small)
            .bold(true)
            .face(Face::Display)
            .tone(Tone::Accent),
        description(about.to_string()),
        row(answers).space(Space::Half),
    ))
    .space(Space::Half)
    .fill(Fill::Card)
    .boxed()
}
