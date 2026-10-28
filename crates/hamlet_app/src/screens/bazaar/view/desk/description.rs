//! The draft's description: a field of at least three lines that
//! wraps its words, grows with them, and takes a new line on
//! Shift+Enter.

use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::Key;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::text::{
    EditableText, LineBreak, LineHeight, TextEdit, TextLayout,
};
use bevy::ui::{UiRect, px};
use bevy_fynix::views::{BehaviorExt, FrameProps, text_field};
use bevy_fynix::{ScopedExt, ViewExt, resource};
use hamlet_ui::AnyView;
use hamlet_ui::fields::Resized;
use hamlet_ui::styles::text_input;

use super::Desk;
use crate::chat::new_line;

/// The fewest and most lines the field shows.
const FEWEST_LINES: usize = 3;
const MOST_LINES: usize = 10;

/// The height of a line, in whole pixels, so the lines given are the
/// height they take.
const LINE: f32 = 17.0;

/// The field the description is typed in.
#[derive(Component)]
pub(super) struct Described;

/// A new line typed at the caret, to be put back in the field with
/// the caret after it this many letters in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Broken {
    pub letters: usize,
    /// Updates left before the field takes focus again.
    pub wait: u8,
}

/// The description's field.
pub(super) fn description() -> AnyView {
    text_field(
        resource::<Desk, _>(|desk| {
            desk.form
                .as_ref()
                .map(|form| form.description.clone())
                .unwrap_or_default()
        }),
        commit,
    )
    .padding(UiRect::axes(px(8.0), px(6.0)))
    .rules(text_input)
    .tagged(Described)
    .boxed()
}

/// Enter keeps the description; Shift+Enter breaks its line instead.
fn commit(world: &mut World, typed: String) {
    let input = input_of(world);
    let breaking = world
        .get_resource::<ButtonInput<Key>>()
        .is_some_and(|keys| keys.pressed(Key::Shift))
        && input.is_some_and(|input| {
            world.resource::<InputFocus>().get() == Some(input)
        });
    let (text, broken) = match breaking {
        true => {
            let range = input
                .and_then(|input| world.get::<EditableText>(input))
                .map_or(typed.len()..typed.len(), |field| {
                    field.editor().raw_selection().text_range()
                });
            let (text, letters) = new_line(&typed, range);
            (text, Some(Broken { letters, wait: 2 }))
        }
        false => (typed, None),
    };
    if let Some(form) = &mut world.resource_mut::<Desk>().form {
        form.description = text;
        form.broken = broken;
    }
}

fn input_of(world: &mut World) -> Option<Entity> {
    let mut query =
        world.query_filtered::<&Children, With<Described>>();
    let children = query.iter(world).next()?.to_vec();
    children
        .into_iter()
        .find(|child| world.get::<EditableText>(*child).is_some())
}

/// Wraps the description's words and grows its field with its lines,
/// and gives it focus again after a line is broken.
pub(super) fn shape(world: &mut World) {
    let Some(input) = input_of(world) else {
        return;
    };
    let mut resized = false;
    if let Some(mut layout) = world.get_mut::<TextLayout>(input)
        && layout.linebreak != LineBreak::WordBoundary
    {
        layout.linebreak = LineBreak::WordBoundary;
        resized = true;
    }
    if let Some(mut height) = world.get_mut::<LineHeight>(input)
        && *height != LineHeight::Px(LINE)
    {
        *height = LineHeight::Px(LINE);
    }
    if let Some(mut text) = world.get_mut::<EditableText>(input) {
        let lines = text.editor.try_layout().map_or(1, |layout| {
            layout.len().clamp(FEWEST_LINES, MOST_LINES)
        });
        let want = Some(lines.max(FEWEST_LINES) as f32);
        if text.visible_lines != want {
            text.visible_lines = want;
            resized = true;
        }
    }
    if resized
        && let Some(mut sized) = world.get_resource_mut::<Resized>()
    {
        sized.set_changed();
    }
    let broken = world
        .resource::<Desk>()
        .form
        .as_ref()
        .and_then(|form| form.broken);
    let Some(broken) = broken else {
        return;
    };
    if let Some(form) = &mut world.resource_mut::<Desk>().form {
        form.broken = (broken.wait > 0).then_some(Broken {
            wait: broken.wait - 1,
            ..broken
        });
    }
    if broken.wait > 0 {
        return;
    }
    world
        .resource_mut::<InputFocus>()
        .set(input, FocusCause::Pressed);
    if let Some(mut text) = world.get_mut::<EditableText>(input) {
        text.queue_edit(TextEdit::TextStart(false));
        for _ in 0..broken.letters {
            text.queue_edit(TextEdit::Right(false));
        }
    }
}
