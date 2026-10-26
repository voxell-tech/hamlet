//! The field a message is typed in: its placeholder, and Escape.

use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Has, With};
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::MouseScrollPixelsPerLine;
use bevy::input_focus::FocusedInput;
use bevy::picking::Pickable;
use bevy::picking::events::PointerScroll;
use bevy::text::{EditableText, LineHeight, TextLayoutInfo};
use bevy::ui::{ComputedNode, Node, PositionType, UiRect, Val, px};
use bevy_fynix::views::{BehaviorExt, FrameProps, column};

use super::{ChatField, Draft};
use crate::fields::Resized;
use crate::platform::show;
use crate::{View, built};

/// The editable text inside the field.
fn typing(
    root: &Query<&Children, With<ChatField>>,
    texts: &Query<&EditableText>,
) -> Option<Entity> {
    root.iter()
        .flat_map(|children| children.iter().copied())
        .find(|child| texts.contains(*child))
}

/// Keeps what was typed when Escape is pressed in the field.
pub fn keep_typed(
    key: On<FocusedInput<KeyboardInput>>,
    mut draft: ResMut<Draft>,
    root: Query<&Children, With<ChatField>>,
    texts: Query<&EditableText>,
) {
    let Some(input) = typing(&root, &texts) else {
        return;
    };
    if key.focused_entity != input
        || !key.input.state.is_pressed()
        || key.input.logical_key != Key::Escape
    {
        return;
    }
    // The field puts back its bound text on Escape, so what was
    // typed is made that text.
    if let Ok(text) = texts.get(input) {
        draft.text = text.value().to_string();
    }
}

/// The most lines the field grows to before it scrolls.
const MOST_LINES: usize = 8;

/// The height of a line of the field, in whole pixels, so the height
/// given for its lines is the height they take.
const LINE: f32 = 17.0;

/// Grows each field with the lines in it, so a quote and the words
/// after it are both seen.
pub fn grow(
    root: Query<&Children, With<ChatField>>,
    mut texts: Query<(&mut EditableText, &mut LineHeight)>,
    sizes: Query<(&ComputedNode, &TextLayoutInfo)>,
    mut resized: ResMut<Resized>,
) {
    let inputs = root
        .iter()
        .flat_map(|children| children.iter().copied())
        .collect::<Vec<_>>();
    for input in inputs {
        let Ok((mut text, mut height)) = texts.get_mut(input) else {
            continue;
        };
        // A field taller than it was scrolled shows its first line
        // again.
        if let Ok((node, info)) = sizes.get(input) {
            let most =
                (info.size.y - node.content_box().size().y).max(0.0);
            if text.viewport.offset.y > most {
                text.viewport.offset.y = most;
            }
        }
        if *height != LineHeight::Px(LINE) {
            *height = LineHeight::Px(LINE);
        }
        let lines = text
            .editor
            .try_layout()
            .map_or(1, |layout| layout.len().clamp(1, MOST_LINES));
        // The input is as tall as its lines, so the field centres it.
        let want = Some(lines as f32);
        if text.visible_lines != want {
            text.visible_lines = want;
            resized.set_changed();
        }
    }
}

/// Turns the wheel over the field into scrolling its lines, when it
/// has more than it shows. Otherwise the wheel is left to the chat.
pub fn scroll_field(
    mut scroll: On<PointerScroll>,
    per_line: Res<MouseScrollPixelsPerLine>,
    root: Query<(Entity, &Children), With<ChatField>>,
    mut texts: Query<(
        &ComputedNode,
        &TextLayoutInfo,
        &mut EditableText,
    )>,
) {
    // Only a wheel over the field is the field's.
    let Some(input) = root.iter().find_map(|(field, children)| {
        children
            .iter()
            .copied()
            .find(|child| texts.contains(*child))
            .filter(|child| {
                scroll.entity == *child || scroll.entity == field
            })
    }) else {
        return;
    };
    let Ok((node, info, mut at)) = texts.get_mut(input) else {
        return;
    };
    // The scroll is in the pixels the text is laid out in.
    let turned = bevy_fynix::scroll::wheel(&scroll, *per_line).0.y
        / node.inverse_scale_factor();
    let most = (info.size.y - node.content_box().size().y).max(0.0);
    let to = (at.viewport.offset.y - turned).clamp(0.0, most).floor();
    if to != at.viewport.offset.y {
        at.viewport.offset.y = to;
        scroll.propagate(false);
    }
}

/// The thumb of the bar along a long field.
#[derive(Component)]
pub struct FieldBar;

/// The room above the field's lines: its frame and its padding.
const ABOVE: f32 = 5.5;

/// The thumb of the bar, over the right of the field.
pub fn bar() -> impl View {
    built(|cx| {
        let fill = cx.theme().palette.brass_5;
        cx.build(
            column(())
                .position(PositionType::Absolute)
                .inset(UiRect {
                    left: Val::Auto,
                    top: Val::Auto,
                    right: px(3.0),
                    bottom: Val::Auto,
                })
                .width(px(3.0))
                .height(px(0.0))
                .fill(fill)
                .with(Pickable::IGNORE)
                .tagged(FieldBar),
        )
    })
}

/// Sets the thumb to where the field's lines are scrolled to. A
/// field that shows all its lines has none.
pub fn show_bar(
    root: Query<&Children, With<ChatField>>,
    texts: Query<(&ComputedNode, &TextLayoutInfo, &EditableText)>,
    mut bars: Query<&mut Node, With<FieldBar>>,
) {
    let Some((node, info, at)) = root
        .iter()
        .flat_map(|children| children.iter().copied())
        .find_map(|child| texts.get(child).ok())
    else {
        return;
    };
    let scale = node.inverse_scale_factor();
    let seen = node.content_box().size().y * scale;
    let whole = info.size.y * scale;
    let (top, height) = if whole > seen + 0.5 {
        let thumb = (seen * seen / whole).max(12.0).min(seen);
        let place = at.viewport.offset.y * scale / (whole - seen);
        (ABOVE + place.clamp(0.0, 1.0) * (seen - thumb), thumb)
    } else {
        (ABOVE, 0.0)
    };
    for mut bar in &mut bars {
        let (top, height) = (px(top), px(height));
        if bar.top != top || bar.height != height {
            bar.top = top;
            bar.height = height;
        }
    }
}

/// The words an empty field shows to say what can be typed.
#[derive(Component)]
pub struct Placeholder;

/// Shows the placeholder only in an empty field, and keeps the
/// pointer passing through it to the field.
pub fn show_placeholder(
    mut commands: Commands,
    root: Query<&Children, With<ChatField>>,
    texts: Query<&EditableText>,
    mut words: Query<
        (Entity, &mut Node, Has<Pickable>),
        With<Placeholder>,
    >,
    under: Query<&Children>,
) {
    let blank = typing(&root, &texts)
        .and_then(|input| texts.get(input).ok())
        .is_none_or(|text| text.value() == "");
    for (entity, mut node, passes) in &mut words {
        show(&mut node, blank);
        if !passes {
            commands.entity(entity).try_insert(Pickable::IGNORE);
            for child in under.iter_descendants(entity) {
                commands.entity(child).try_insert(Pickable::IGNORE);
            }
        }
    }
}
