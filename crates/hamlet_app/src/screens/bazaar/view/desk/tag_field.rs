//! The tags of a draft: a chip each with its cross, and the field a
//! tag is typed in, with the tags in use that match under it. Up and
//! Down light one, Enter or a comma adds it, Backspace in the empty
//! field takes the last tag off and Escape empties the field.

use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::text::EditableText;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, label, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_ui::styles::{bare, link_row, note, text_input};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::{AnyView, Cx};

use super::Desk;
use super::tags::{MOST_TAGS, Offered, Refused, add, offered};
use crate::bazaar::market::drafts::{Drafts, ask_tags};
use crate::roll::{either, moved};

/// The field a tag is typed in.
#[derive(Component)]
pub(super) struct TagField;

/// The tags of the draft, the field and what it offers.
pub(super) fn tags() -> AnyView {
    let chips = keyed(
        resource::<Desk, _>(|desk| {
            desk.form
                .as_ref()
                .map(|form| form.tags.clone())
                .unwrap_or_default()
        }),
        |tags| {
            let mut all = tags
                .iter()
                .map(|tag| chip(tag.clone()))
                .collect::<Vec<_>>();
            if tags.is_empty() {
                all.push(note("No tags yet.").boxed());
            }
            row(all)
                .gap(6.0)
                .align(AlignItems::Center)
                .tagged(Wraps)
                .boxed()
        },
    )
    .within(bare());
    let field = text_field(
        resource::<Desk, _>(|desk| {
            desk.form
                .as_ref()
                .map(|form| form.typing.clone())
                .unwrap_or_default()
        }),
        |world, typed| entered(world, &typed),
    )
    .rules(text_input)
    .tagged(TagField);
    let now = derived(|world: &World| {
        let used = &world.resource::<Drafts>().tags.1;
        let form = world.resource::<Desk>().form.clone()?;
        let all = offered(&form.typing, used, &form.tags);
        let last = all.len().checked_sub(1)?;
        Some((all, form.lit.min(last)))
    })
    .when(either(moved::<Desk>(), moved::<Drafts>()));
    let drop = keyed(now, |now| match now {
        None => bare().boxed(),
        Some((all, lit)) => offers(all, *lit),
    })
    .within(bare());
    column((
        chips,
        field,
        drop,
        note(format!("Enter adds a tag. Up to {MOST_TAGS}.")),
    ))
    .gap(6.0)
    .boxed()
}

/// A tag, with its cross that takes it off.
fn chip(tag: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let edge = cx.theme().palette.gold_5;
        cx.build(chip_in(tag.clone(), edge))
    })
}

fn chip_in(tag: String, edge: Color) -> AnyView {
    let off = tag.clone();
    row((
        label(tag).size(12.0).tone(Tone::Body),
        button(label("\u{d7}").size(13.0).tone(Tone::Dim))
            .padding(UiRect::horizontal(px(4.0)))
            .rules(link_row)
            .tagged(Variant::Default)
            .on_activate(move |world: &mut World| {
                if let Some(form) =
                    &mut world.resource_mut::<Desk>().form
                {
                    form.tags.retain(|tag| *tag != off);
                }
            }),
    ))
    .gap(2.0)
    .align(AlignItems::Center)
    .padding(UiRect::new(px(10.0), px(4.0), px(2.0), px(2.0)))
    .border(1.0)
    .border_color(edge)
    .radius(12.0)
    .boxed()
}

/// The tags offered, the one at `lit` lit.
fn offers(all: &[Offered], lit: usize) -> AnyView {
    let rows = all
        .iter()
        .enumerate()
        .map(|(at, one)| {
            let (words, count) = match one {
                Offered::Used(tag, count) => {
                    (tag.clone(), count.to_string())
                }
                Offered::New(tag) => {
                    (format!("+ Add \"{tag}\""), "new tag".into())
                }
            };
            let tag = one.tag().to_string();
            button(
                row((
                    label(words).size(12.0).bold(at == lit).tone(
                        match at == lit {
                            true => Tone::Accent,
                            false => Tone::Body,
                        },
                    ),
                    column(()).grow(1.0),
                    label(count)
                        .size(11.0)
                        .tone(Tone::Faint)
                        .wrap(false),
                ))
                .gap(10.0)
                .grow(1.0)
                .align(AlignItems::Center),
            )
            .padding(UiRect::axes(px(10.0), px(5.0)))
            .rules(link_row)
            .tagged(Variant::Default)
            .on_activate(move |world: &mut World| take(world, &tag))
            .boxed()
        })
        .collect::<Vec<_>>();
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (edge, fill) =
            (theme.palette.gold_5, theme.palette.input);
        cx.build(
            column(rows)
                .gap(0.0)
                .border(1.0)
                .border_color(edge)
                .fill(fill)
                .radius(theme.space.round),
        )
    })
}

/// Adds `typed` to the draft's tags and empties the field, or says
/// why not.
fn take(world: &mut World, typed: &str) {
    let mut desk = world.resource_mut::<Desk>();
    let Some(form) = &mut desk.form else {
        return;
    };
    form.problem = match add(&mut form.tags, typed) {
        Err(Refused::Full) => {
            Some(format!("Up to {MOST_TAGS} tags."))
        }
        _ => None,
    };
    form.typing.clear();
    form.lit = 0;
    form.refocus = true;
}

/// Enter in the field: adds the tag lit, else what was typed.
fn entered(world: &mut World, typed: &str) {
    let used = world.resource::<Drafts>().tags.1.clone();
    let Some(form) = world.resource::<Desk>().form.clone() else {
        return;
    };
    let all = offered(typed, &used, &form.tags);
    match all.get(form.lit.min(all.len().saturating_sub(1))) {
        Some(one) => take(world, one.tag()),
        None if typed.trim().is_empty() => {}
        None => take(world, typed),
    }
}

/// The field's editable text node.
fn field_of(world: &mut World) -> Option<Entity> {
    let mut query =
        world.query_filtered::<&Children, With<TagField>>();
    let children = query.iter(world).next()?.to_vec();
    children
        .into_iter()
        .find(|child| world.get::<EditableText>(*child).is_some())
}

/// Follows the tag field as it is typed in, and the keys pressed in
/// it.
pub(super) fn typing(world: &mut World) {
    let Some(input) = field_of(world) else {
        return;
    };
    let Some(form) = world.resource::<Desk>().form.clone() else {
        return;
    };
    let value = world
        .get::<EditableText>(input)
        .map(|text| text.value().to_string())
        .unwrap_or_default();
    let focused =
        world.get_resource::<InputFocus>().and_then(InputFocus::get)
            == Some(input);
    let keys = world.get_resource::<ButtonInput<KeyCode>>();
    let pressed =
        |key| keys.is_some_and(|keys| keys.just_pressed(key));
    let (up, down, back, escape) = (
        pressed(KeyCode::ArrowUp),
        pressed(KeyCode::ArrowDown),
        pressed(KeyCode::Backspace),
        pressed(KeyCode::Escape),
    );
    let clear = |world: &mut World| {
        if let Some(mut text) = world.get_mut::<EditableText>(input) {
            text.editor_mut().set_text("");
        }
    };
    if form.refocus {
        if let Some(form) = &mut world.resource_mut::<Desk>().form {
            form.refocus = false;
        }
        clear(world);
        if let Some(mut focus) =
            world.get_resource_mut::<InputFocus>()
        {
            focus.set(input, FocusCause::Pressed);
        }
        return;
    }
    if escape && !form.typing.is_empty() {
        clear(world);
        if let Some(form) = &mut world.resource_mut::<Desk>().form {
            form.typing.clear();
            form.lit = 0;
        }
        return;
    }
    if !focused {
        return;
    }
    if let Some((typed, _)) = value.split_once(',') {
        let typed = typed.to_string();
        entered(world, &typed);
        clear(world);
        return;
    }
    let count = offered(
        &value,
        &world.resource::<Drafts>().tags.1,
        &form.tags,
    )
    .len();
    let mut desk = world.resource_mut::<Desk>();
    let Some(edit) = &mut desk.form else {
        return;
    };
    if back && value.is_empty() && form.typing.is_empty() {
        edit.tags.pop();
    } else if down {
        edit.lit = (form.lit + 1).min(count.saturating_sub(1));
    } else if up {
        edit.lit = form.lit.saturating_sub(1);
    }
    if value != form.typing {
        edit.typing.clone_from(&value);
        edit.lit = 0;
        ask_tags(world, &value);
    }
}
