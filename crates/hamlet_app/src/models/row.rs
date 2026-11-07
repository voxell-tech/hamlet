//! The one row a model is listed by, wherever models are listed.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, icon, label, menu_item,
    row,
};
use bevy_fynix::{Focused, ScopedExt, ViewExt, derived, style};
use hamlet_client::realm::Providers;
use hamlet_ui::styles::{ICON, note_rules, small_rules};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::providers::provider_mark;
use hamlet_ui::{AnyView, Cx, with_cx};

use super::{model_name, provider_of, source_of};

/// The side of a provider's mark in a row.
const MARK: f32 = 16.0;

/// The room between a row's edge and its mark, and between its parts.
const PAD: f32 = 8.0;
const GAP: f32 = 10.0;

/// How far in from a row's edge its name starts, for what is laid
/// under a row to line up with it.
pub(crate) const WORDS_AT: f32 = PAD + MARK + GAP;

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

/// `model` as one row that `pick` chooses: its provider's mark, its
/// name with a check after it while picked, its provider under it,
/// its tag and its end.
pub(crate) fn model_row(
    model: ModelRow,
    pick: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    let ModelRow {
        name,
        picked,
        tag,
        end,
    } = model;
    let check = if picked {
        glyph(Glyph::Check, ICON)
            .tone(Tone::Accent)
            .opacity(1.0)
            .boxed()
    } else {
        frame().boxed()
    };
    let title =
        row((one_line(label(model_name(&name).to_string())), check))
            .gap(6.0)
            .align(AlignItems::Center)
            .min_width(px(0.0));
    let id = name.clone();
    let source = one_line(label(
        derived(move |world: &World| source_of(world, &id))
            .when(crate::roll::moved::<Providers>()),
    ))
    .rules(note_rules);
    let words = column((title, source))
        .gap(1.0)
        .grow(1.0)
        .min_width(px(0.0));
    let tag = match tag {
        Some(tag) => label(tag)
            .tone(Tone::Faint)
            .wrap(false)
            .rules(small_rules)
            .boxed(),
        None => frame().boxed(),
    };
    menu_item(
        row((
            mark(provider_of(&name), MARK),
            words,
            tag,
            end.unwrap_or_else(|| frame().boxed()),
        ))
        .gap(GAP)
        .align(AlignItems::Center)
        .width(percent(100.0)),
    )
    .padding(UiRect::axes(px(PAD), px(5.0)))
    .rules(row_look)
    .on_activate(pick)
    .boxed()
}

/// A row's icons left unset are hidden until the row is under the
/// pointer or has the focus, which also lights it.
fn row_look(cx: &mut Cx) {
    style::<HamletTheme>()
        .icon(|i, _| i.opacity(0.0))
        .hovered(|s| s.icon(|i, _| i.opacity(1.0)))
        .when::<Focused>(|s| {
            s.icon(|i, _| i.opacity(1.0))
                .fill(|t| t.palette.button_hover)
        })
        .transition(Motion::Interact)
        .apply(cx);
}
