//! The one row a model is listed by, wherever models are listed.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::{MotionTokens, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, icon, label, menu_item,
    row,
};
use bevy_fynix::{Focused, ScopedExt, ViewExt, derived, style};
use hamlet_client::realm::Providers;
use hamlet_protocol::client as protocol;
use hamlet_ui::kinds::chip;
use hamlet_ui::styles::{ICON, note, note_rules, small_rules};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::providers::provider_mark;
use hamlet_ui::{AnyView, Cx, with_cx};

use super::{
    Shelf, context_words, model_name, model_title, provider_of,
    source_of,
};
use crate::roll::moved;

/// The side of a provider's mark in a row.
const MARK: f32 = 16.0;

/// The room between a row's edge and its mark, and between its parts.
const PAD: f32 = 8.0;
const GAP: f32 = 8.0;

/// A model as a row shows it.
pub(crate) struct ModelRow {
    /// Its name in the catalogue, `provider::id`.
    pub name: String,
    /// Checked: the model in use.
    pub picked: bool,
    /// Muted words after it, such as "Default".
    pub tag: Option<&'static str>,
    /// What stands at its end. An icon there shows only under the
    /// pointer or the focus, unless it sets its own opacity.
    pub end: Option<AnyView>,
    pub detail: Detail,
}

/// What a row shows of where its model comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Detail {
    /// Its provider's mark, and its provider and id under its name,
    /// for a list that mixes providers.
    Source,
    /// Its id under its name where that says more, for a list of one
    /// provider's models, whose provider is named beside the list.
    Id,
}

/// The mark of the provider `id` at `size`, or its first letter in a
/// small frame for a provider with no mark.
pub(crate) fn mark(id: &str, size: f32) -> AnyView {
    if let Some(image) = provider_mark(id, size) {
        return icon(image).size(size).opacity(1.0).boxed();
    }
    let letter = id
        .chars()
        .next()
        .map(|first| first.to_uppercase().to_string())
        .unwrap_or_default();
    with_cx(move |cx| {
        let theme = cx.theme();
        column((label(letter)
            .size(size * 0.62)
            .bold(true)
            .tone(Tone::Faint)
            .wrap(false),))
        .width(px(size))
        .height(px(size))
        .align(AlignItems::Center)
        .justify(JustifyContent::Center)
        .border(theme.space.border)
        .border_color(theme.palette.gold_5)
        .radius(size / 4.0)
    })
    .boxed()
}

/// A quiet word beside a model's name, such as "Default".
pub(crate) fn tag(text: &'static str) -> AnyView {
    chip(
        (label(text).tone(Tone::Dim).wrap(false).rules(small_rules),),
    )
    .shrink(0.0)
    .boxed()
    .boxed()
}

/// `text` muted over rows that show `detail`, in line with their
/// names.
pub(crate) fn over_rows(
    text: &'static str,
    detail: Detail,
) -> AnyView {
    let lead = match detail {
        Detail::Source => MARK + GAP,
        Detail::Id => GAP,
    };
    column((note(text),))
        .padding(UiRect::left(px(PAD + lead)))
        .boxed()
}

/// The muted line over rows that show `detail` and says what their
/// numbers are.
pub(crate) fn legend(detail: Detail) -> AnyView {
    over_rows("In / out per 1M tokens · context", detail)
}

/// What `model` costs per million tokens in and out, and how many
/// tokens it holds, as far as the catalogue knows.
fn hint(model: &protocol::Listed) -> String {
    let dollars = |value: f64| match value.fract() == 0.0 {
        true => format!("${value:.0}"),
        false => format!("${value:.2}"),
    };
    let price = model
        .price
        .as_ref()
        .filter(|_| model.priced != protocol::ListedPriced::Unknown)
        .map(|price| match (price.input, price.output) {
            (0.0, 0.0) => "Free".to_string(),
            (input, output) => {
                format!("{}/{}", dollars(input), dollars(output))
            }
        });
    price
        .into_iter()
        .chain(model.context.map(context_words))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// `model` as one row that `pick` chooses: its name with a check
/// after it while picked and its tag, and under it as much of where
/// it comes from as its [`Detail`] says, with its price and context
/// at the right.
pub(crate) fn model_row(
    model: ModelRow,
    pick: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    let ModelRow {
        name,
        picked,
        tag: word,
        end,
        detail,
    } = model;
    let check = if picked {
        glyph(Glyph::Check, ICON)
            .tone(Tone::Accent)
            .opacity(1.0)
            .boxed()
    } else {
        frame().boxed()
    };
    let called = name.clone();
    let title = row((
        one_line(label(
            derived(move |world: &World| model_title(world, &called))
                .when(moved::<Shelf>()),
        )),
        check,
        word.map_or_else(|| frame().boxed(), tag),
    ))
    .gap(6.0)
    .align(AlignItems::Center)
    .min_width(px(0.0));
    let id = name.clone();
    let source = match detail {
        Detail::Source => one_line(label(
            derived(move |world: &World| source_of(world, &id))
                .when(moved::<Providers>()),
        ))
        .boxed(),
        Detail::Id => {
            let at =
                id.split_once("::").map_or(id.as_str(), |(_, at)| at);
            match at == model_name(&id) {
                true => frame().boxed(),
                false => one_line(label(at.to_string())).boxed(),
            }
        }
    };
    let card = name.clone();
    let costs = label(
        derived(move |world: &World| {
            world
                .get_resource::<Shelf>()
                .and_then(|shelf| {
                    shelf
                        .models
                        .iter()
                        .find(|model| model.name == card)
                })
                .map(hint)
                .unwrap_or_default()
        })
        .when(moved::<Shelf>()),
    )
    .wrap(false);
    let under = row((
        row((source,)).grow(1.0).min_width(px(0.0)),
        row((costs,)).shrink(0.0),
    ))
    .gap(6.0)
    .align(AlignItems::Center)
    .rules(note_rules);
    let words =
        column((title, under)).gap(2.0).grow(1.0).min_width(px(0.0));
    let lead = match detail {
        Detail::Source => mark(provider_of(&name), MARK),
        Detail::Id => frame().boxed(),
    };
    menu_item(
        row((lead, words, end.unwrap_or_else(|| frame().boxed())))
            .gap(GAP)
            .align(AlignItems::Center)
            .width(percent(100.0)),
    )
    .padding(UiRect::axes(px(PAD), px(6.0)))
    .rules(row_look)
    .on_activate(pick)
    .boxed()
}

/// A row's icons left unset are hidden until the row is under the
/// pointer or has the focus, which also lights it.
fn row_look(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| f.radius(t.space.round))
        .icon(|i, _| i.opacity(0.0))
        .hovered(|s| s.icon(|i, _| i.opacity(1.0)))
        .when::<Focused>(|s| {
            s.icon(|i, _| i.opacity(1.0))
                .fill(|t| t.palette.button_hover)
        })
        .transition(HamletTheme::interact)
        .apply(cx);
}
