//! A field of several lines a message is written in. It grows with
//! what is typed up to its most lines; Enter sends what is typed and
//! empties it, Shift+Enter adds a line, and Escape keeps what is
//! typed and leaves it.

use bevy::input::ButtonInput;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{
    FocusCause, FocusLost, FocusedInput, InputFocus,
};
use bevy::picking::events::{Pointer, Press};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::{
    EditableText, FontSize, LineBreak, TextCursorStyle, TextEdit,
};
use bevy_fynix::tokens::{TextTokens, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, row,
};
use bevy_fynix::{Prop, ScopedExt};

use crate::pointer::{NodePointer, PointerMode};
use crate::style::{Placeholder, field_rules};
use crate::{AnyView, Cx};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, grow);
}

/// On the text of a message field: the most lines it grows to, and
/// the text it was last given.
#[derive(Component)]
pub struct MessageInput {
    most: u32,
    given: String,
}

/// A field showing `value`, growing to `most` lines, with
/// `placeholder` in it while it is empty. Enter hands what is typed
/// to `send` and empties it; Escape, or leaving it, hands what is
/// typed to `keep` when it is not what it was given.
pub fn message_field(
    value: impl Into<Prop<String>>,
    placeholder: String,
    most: u32,
    send: impl Fn(&mut World, String) + Clone + Send + Sync + 'static,
    keep: impl Fn(&mut World, String) + Clone + Send + Sync + 'static,
) -> AnyView {
    let value = value.into();
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (gap, height, inset) = (
            theme.space.gap,
            theme.space.row,
            theme.space.gap + theme.space.border,
        );
        let font = TextFont {
            font_size: FontSize::Px(theme.text.body),
            ..TextFont::default()
        };
        let ink = theme.tone(Tone::Body);
        let caret = TextCursorStyle {
            color: ink,
            selection_color: *theme.colors.selection,
            ..TextCursorStyle::default()
        };
        let outer = cx.build(column(()).gap(0.0).grow(1.0));
        cx.under(outer, |cx| {
            let root = cx.build(
                row(())
                    .gap(0.0)
                    .min_height(px(height))
                    .padding(UiRect::axes(px(gap), px(gap / 2.0)))
                    .align(AlignItems::Center)
                    .rules(field_rules()),
            );
            let input = cx.under(root, |cx| {
                cx.build(frame().grow(1.0).min_width(px(0.0)))
            });
            cx.world
                .entity_mut(input)
                .insert((
                    EditableText::default(),
                    TextLayout {
                        linebreak: LineBreak::WordBoundary,
                        ..TextLayout::default()
                    },
                    font,
                    TextColor(ink),
                    caret,
                    MessageInput {
                        most: most.max(1),
                        given: String::new(),
                    },
                    NodePointer::new(PointerMode::Write),
                    TabIndex(0),
                ))
                .observe(
                    move |mut key: On<FocusedInput<KeyboardInput>>,
                          mut texts: Query<&mut EditableText>,
                          held: Res<ButtonInput<Key>>,
                          mut commands: Commands| {
                        if pressed(
                            &key,
                            &mut texts,
                            &held,
                            &mut commands,
                            &send,
                        ) {
                            key.propagate(false);
                        }
                    },
                )
                .observe(
                    move |lost: On<FocusLost>, mut commands: Commands| {
                        let input = lost.event_target();
                        let keep = keep.clone();
                        commands.queue(move |world: &mut World| {
                            if let Some((text, true)) =
                                typed(world, input)
                            {
                                keep(world, text);
                            }
                        });
                    },
                );
            // A press on the frame outside the text puts the caret in
            // it.
            cx.world.entity_mut(root).observe(
                move |press: On<Pointer<Press>>,
                      mut commands: Commands| {
                    if press.button == PointerButton::Primary {
                        commands.queue(move |world: &mut World| {
                            focus(world, input)
                        });
                    }
                },
            );
            cx.effect(input, value, |world, input, text: &String| {
                give(world, input, text)
            });
            cx.build(
                row((label(placeholder)
                    .tone(Tone::Dim)
                    .wrap(false),))
                .position(PositionType::Absolute)
                .inset(UiRect {
                    left: px(inset),
                    right: Val::Auto,
                    top: px(0.0),
                    bottom: px(0.0),
                })
                .align(AlignItems::Center)
                .tagged(Placeholder)
                .tagged(Pickable::IGNORE),
            );
        });
        outer
    })
}

/// Shows `text` in the field `input`, unless it is being typed in.
fn give(world: &mut World, input: Entity, text: &str) {
    let typing = is_focused(world, input);
    let Ok(mut entity) = world.get_entity_mut(input) else {
        return;
    };
    if let Some(mut kept) = entity.get_mut::<MessageInput>() {
        text.clone_into(&mut kept.given);
    }
    if typing {
        return;
    }
    if let Some(mut editable) = entity.get_mut::<EditableText>()
        && editable.value() != text
    {
        editable.editor_mut().set_text(text);
    }
}

fn is_focused(world: &World, node: Entity) -> bool {
    world
        .get_resource::<InputFocus>()
        .is_some_and(|focus| focus.get() == Some(node))
}

/// Gives `input` focus, with the caret at the end of its text.
fn focus(world: &mut World, input: Entity) {
    if is_focused(world, input) {
        return;
    }
    if let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
        focus.set(input, FocusCause::Pressed);
    }
    if let Some(mut text) = world.get_mut::<EditableText>(input) {
        text.queue_edit(TextEdit::TextEnd(false));
    }
}

/// What is typed in `input`, and whether it differs from what it
/// was given.
fn typed(world: &World, input: Entity) -> Option<(String, bool)> {
    let text = world.get::<EditableText>(input)?.value().to_string();
    let moved = text != world.get::<MessageInput>(input)?.given;
    Some((text, moved))
}

/// Does what Enter, Shift+Enter or Escape do in a message field:
/// whether it was one of them.
fn pressed<S>(
    key: &On<FocusedInput<KeyboardInput>>,
    texts: &mut Query<&mut EditableText>,
    held: &ButtonInput<Key>,
    commands: &mut Commands,
    send: &S,
) -> bool
where
    S: Fn(&mut World, String) + Clone + Send + Sync + 'static,
{
    let input = key.focused_entity;
    let Ok(mut editable) = texts.get_mut(input) else {
        return false;
    };
    if !key.input.state.is_pressed() || editable.is_composing() {
        return false;
    }
    match key.input.logical_key {
        Key::Enter if held.pressed(Key::Shift) => {
            editable.queue_edit(TextEdit::Insert("\n".into()));
        }
        Key::Enter => {
            let text = editable.value().to_string();
            if text.trim().is_empty() {
                return true;
            }
            editable.editor_mut().set_text("");
            let send = send.clone();
            commands
                .queue(move |world: &mut World| send(world, text));
        }
        // Leaving the field hands what is typed to `keep`.
        Key::Escape => {
            commands.queue(move |world: &mut World| {
                if let Some(mut focus) =
                    world.get_resource_mut::<InputFocus>()
                    && focus.get() == Some(input)
                {
                    focus.clear();
                }
            });
        }
        _ => return false,
    }
    true
}

/// Grows each message field with the lines typed in it.
fn grow(mut inputs: Query<(&mut EditableText, &MessageInput)>) {
    for (mut text, input) in &mut inputs {
        let lines = text.editor.try_layout().map_or(1, |layout| {
            u32::try_from(layout.len())
                .unwrap_or(u32::MAX)
                .clamp(1, input.most)
        });
        let want = Some(lines as f32);
        if text.visible_lines != want {
            text.visible_lines = want;
        }
    }
}
