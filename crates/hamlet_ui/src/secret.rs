//! Text fields whose text is drawn as stars and kept off the
//! clipboard.

use bevy::app::{App, PostUpdate};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::query::{With, Without};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, Res};
use bevy::ecs::world::World;
use bevy::picking::Pickable;
use bevy::text::{
    EditableText, EditableTextSystems, FontSource, TextColor,
    TextEdit, TextFont,
};
use bevy::ui::widget::Text;
use bevy::ui::{
    AlignItems, ComputedNode, Node, Overflow, UiGlobalTransform, Val,
    px,
};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, label, overlay, row, text_field,
};
use bevy_fynix::{Prop, ScopedExt};

use crate::View;
use crate::platform::{ClipInside, Mono};
use crate::styles::text_input;

pub fn plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        (unshared.before(EditableTextSystems), mask),
    );
}

/// A [`text_field`] for a key or a token, drawn as stars. It takes a
/// paste but does not copy or cut.
pub(crate) fn secret_field(
    value: impl Into<Prop<String>>,
    on_commit: impl Fn(&mut World, String) + Send + Sync + 'static,
) -> impl View {
    // The field takes the room; the stars lie over it, in a box made
    // the size of its text's, which cuts them where the text is cut.
    row((
        text_field(value, on_commit)
            .grow(1.0)
            .rules(text_input)
            .tagged(Secret),
        overlay((label("").tagged(Stars).tagged(Pickable::IGNORE),))
            .align(AlignItems::Center)
            .overflow(Overflow::clip_x())
            .tagged(Cover)
            .tagged(ClipInside)
            .tagged(Pickable::IGNORE),
    ))
}

#[derive(Component)]
struct Secret;

/// The stars over a [`Secret`] field.
#[derive(Component)]
struct Stars;

/// The box the [`Stars`] lie in.
#[derive(Component)]
struct Cover;

/// Drops copying and cutting from [`Secret`] fields before they
/// apply, so a key never reaches the system's clipboard.
fn unshared(
    fields: Query<&Children, With<Secret>>,
    mut inputs: Query<&mut EditableText>,
) {
    let shares = |edit: &TextEdit| {
        matches!(edit, TextEdit::Copy | TextEdit::Cut)
    };
    for inside in &fields {
        for &child in inside {
            if let Ok(mut text) = inputs.get_mut(child)
                && text.pending_edits.iter().any(shares)
            {
                text.pending_edits.retain(|edit| !shares(edit));
            }
        }
    }
}

/// Hides a [`Secret`] field's text and draws as many stars in its
/// place. Both are set in the monospace face, where a star is as wide
/// as any letter, and the stars lie where the text lies and scroll
/// with it, so the caret and the selection fall on them.
fn mask(
    mono: Option<Res<Mono>>,
    fields: Query<(&Children, &ChildOf), With<Secret>>,
    mut inputs: Query<(
        &EditableText,
        &mut TextColor,
        &mut TextFont,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    below: Query<&Children>,
    mut stars: Query<
        (&mut Text, &mut TextFont, &mut Node, &ChildOf),
        (With<Stars>, Without<EditableText>, Without<Cover>),
    >,
    mut covers: Query<
        (&mut Node, &ComputedNode, &UiGlobalTransform),
        (With<Cover>, Without<EditableText>, Without<Stars>),
    >,
) {
    for (inside, parent) in &fields {
        let Some((typed, face, (start, end), scrolled)) =
            inside.iter().find_map(|&child| {
                let (text, mut ink, mut face, laid, place) =
                    inputs.get_mut(child).ok()?;
                // The editor keeps the text; only the stars are drawn.
                if ink.0 != Color::NONE {
                    ink.0 = Color::NONE;
                }
                if let Some(mono) = &mono
                    && !matches!(&face.font, FontSource::Handle(h) if *h == mono.0)
                {
                    face.font = FontSource::Handle(mono.0.clone());
                }
                let content = laid.content_box();
                let scale = laid.inverse_scale_factor;
                Some((
                    text.value().to_string().chars().count(),
                    face.clone(),
                    // Laid out in physical pixels; placed in logical.
                    (
                        (place.translation.x + content.min.x) * scale,
                        (place.translation.x + content.max.x) * scale,
                    ),
                    text.viewport.offset.x * scale,
                ))
            })
        else {
            continue;
        };
        let masked = "*".repeat(typed);
        for each in below.iter_descendants(parent.parent()) {
            let Ok((mut text, mut star_face, mut node, cover)) =
                stars.get_mut(each)
            else {
                continue;
            };
            if text.0 != masked {
                text.0.clone_from(&masked);
            }
            if *star_face != face {
                *star_face = face.clone();
            }
            set(&mut node.left, -scrolled);
            let Ok((mut cover, laid, place)) =
                covers.get_mut(cover.parent())
            else {
                continue;
            };
            let border = laid.border_box();
            let scale = laid.inverse_scale_factor;
            set(
                &mut cover.padding.left,
                start - (place.translation.x + border.min.x) * scale,
            );
            set(
                &mut cover.padding.right,
                (place.translation.x + border.max.x) * scale - end,
            );
        }
    }
}

/// Sets a length in logical pixels, unless it is already that.
fn set(length: &mut Val, to: f32) {
    if !matches!(*length, Val::Px(now) if (now - to).abs() < 0.1) {
        *length = px(to);
    }
}
