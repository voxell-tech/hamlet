//! A text area: a well holding several lines to type in.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::observer::On;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusLost, FocusedInput, InputFocus};
use bevy::text::{
    EditableText, FontSize, TextColor, TextCursorStyle, TextFont,
};
use bevy::ui::{Node, UiRect, percent, px};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::{SurfaceTokens, TextTokens, Tone};
use bevy_fynix::views::{BehaviorExt, FrameProps, column};
use bevy_fynix::{EntityCursor, ScopedExt, ViewExt, keyed};

use super::bind::At;
use crate::feel::Mood;
use crate::looks::{Fills, bare, well};
use crate::tree::event::Act;
use crate::{AnyView, Cx};

/// The lines the box shows before it scrolls.
const LINES: f32 = 10.0;

/// The size of the text in the box.
const SIZE: f32 = 13.0;

/// The space between the well and its text.
const PAD: f32 = 8.0;

/// On the editable text of an area: the node it sends to, and the
/// text the plugin last knew.
#[derive(Component)]
struct Area {
    at: At,
    sent: String,
}

/// The view of the text area at `at`. The box is built again when the
/// plugin shows other text in it.
pub(super) fn view(at: &At) -> AnyView {
    let area = at.clone();
    keyed(at.text(), move |text| editor(&area, text))
        .within(bare())
        .boxed()
}

fn editor(at: &At, text: &str) -> AnyView {
    let (at, text) = (at.clone(), text.to_string());
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let ink = theme.tone(Tone::Body);
        let root = cx.build(
            column(())
                .fills_width()
                .padding(UiRect::all(px(PAD)))
                .rules(well)
                .tagged(Mood::Field),
        );
        let mut editable = EditableText::new(&text);
        editable.allow_newlines = true;
        editable.visible_lines = Some(LINES);
        cx.world
            .spawn((
                Node {
                    width: percent(100.0),
                    min_width: px(0.0),
                    ..Node::default()
                },
                editable,
                TextFont {
                    font_size: FontSize::Px(SIZE),
                    ..TextFont::default()
                },
                TextColor(ink),
                TextCursorStyle {
                    color: ink,
                    selection_color: theme.selection(),
                    ..TextCursorStyle::default()
                },
                EntityCursor(SystemCursorIcon::Text),
                TabIndex(0),
                Area { at, sent: text },
                ChildOf(root),
            ))
            .observe(lost)
            .observe(key);
        root
    })
}

fn lost(lost: On<FocusLost>, mut commands: Commands) {
    let node = lost.event_target();
    commands.queue(move |world: &mut World| commit(world, node));
}

/// Ctrl+Enter, or Cmd+Enter, commits and leaves the box. Escape puts
/// back the text the plugin showed and leaves it.
fn key(
    key: On<FocusedInput<KeyboardInput>>,
    texts: Query<&EditableText>,
    keys: Res<ButtonInput<Key>>,
    mut commands: Commands,
) {
    let node = key.focused_entity;
    let composing =
        texts.get(node).is_ok_and(EditableText::is_composing);
    if !key.input.state.is_pressed() || composing {
        return;
    }
    let held = keys.pressed(Key::Control) || keys.pressed(Key::Super);
    let enter = match key.input.logical_key {
        Key::Enter if held => true,
        Key::Escape => false,
        _ => return,
    };
    commands.queue(move |world: &mut World| {
        if enter {
            commit(world, node);
        } else {
            revert(world, node);
        }
        let mut focus = world.resource_mut::<InputFocus>();
        if focus.get() == Some(node) {
            focus.clear();
        }
    });
}

/// Tells the plugin the text, if it differs from what it knew.
fn commit(world: &mut World, node: Entity) {
    let Some(typed) = world
        .get::<EditableText>(node)
        .map(|text| text.value().to_string())
    else {
        return;
    };
    let Some(mut area) = world.get_mut::<Area>(node) else {
        return;
    };
    if area.sent == typed {
        return;
    }
    area.sent.clone_from(&typed);
    let at = area.at.clone();
    at.send(world, Act::Typed(typed));
}

fn revert(world: &mut World, node: Entity) {
    let Some(sent) =
        world.get::<Area>(node).map(|area| area.sent.clone())
    else {
        return;
    };
    if let Some(mut text) = world.get_mut::<EditableText>(node) {
        text.editor_mut().set_text(&sent);
    }
}
