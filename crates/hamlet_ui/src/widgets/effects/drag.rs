//! The dock's drop hint and the tab being dragged, dressed as the
//! panels are.

use bevy::color::Alpha;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::Without;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::math::{Vec2, Vec4};
use bevy::time::Time;
use bevy::ui::{
    BackgroundColor, BorderColor, GlobalZIndex, Node, UiRect,
    UiTransform, Val, px,
};

use super::{EffectColors, EffectSettings};
use crate::theme::Themed;

/// The layers the dock library puts its drop hint and the dragged
/// tab on, which is all that tells them from other nodes.
const HINT_LAYER: i32 = 190;
const GHOST_LAYER: i32 = 200;

/// The time a thing that comes in has been on screen.
#[derive(Component, Default)]
pub(super) struct Appearing(f32);

/// A node that glides to the place it is put at. Each is a corner and
/// a size, in pixels.
#[derive(Component)]
pub(super) struct Gliding {
    to: Vec4,
    shown: Vec4,
}

/// The place of a node, if it is placed in pixels.
fn stand(node: &Node) -> Option<Vec4> {
    match (node.left, node.top, node.width, node.height) {
        (Val::Px(x), Val::Px(y), Val::Px(w), Val::Px(h)) => {
            Some(Vec4::new(x, y, w, h))
        }
        _ => None,
    }
}

type Dragged<'a> = (
    Entity,
    &'a GlobalZIndex,
    &'a mut Node,
    &'a mut BackgroundColor,
    &'a mut BorderColor,
    Option<&'a mut Appearing>,
    Option<&'a mut UiTransform>,
    Option<&'a mut Gliding>,
);

/// Makes the dock's drop hint a glowing pane with a brighter border
/// that pulses, and the dragged tab a bordered, filled box. Each
/// fades and grows in as it appears, and the hint moves from one
/// place it could land to the next.
pub(super) fn dress_drag(
    mut commands: Commands,
    time: Res<Time>,
    set: Res<EffectSettings>,
    theme: Themed,
    mut drawn: Query<Dragged, Without<ChildOf>>,
) {
    let colors = EffectColors::of(&theme);
    let pulse = 0.5
        + 0.5
            * (time.elapsed_secs() * set.drop_hint_pulse_rate).sin();
    let dt = time.delta_secs();
    for (
        id,
        layer,
        mut node,
        mut fill,
        mut edge,
        age,
        grown,
        glide,
    ) in &mut drawn
    {
        let (pane, line) = match layer.0 {
            HINT_LAYER => (
                colors.drop_hint.with_alpha(0.06 + 0.05 * pulse),
                colors.drop_hint.with_alpha(0.5 + 0.3 * pulse),
            ),
            GHOST_LAYER => (
                colors.drag_ghost.with_alpha(0.9),
                colors.drag_border,
            ),
            _ => continue,
        };
        // Its share of the way in, 0 to 1, fast at first.
        let come = match age {
            Some(mut age) => {
                age.0 += time.delta_secs();
                let t = (age.0 / set.enter_duration).min(1.0);
                1.0 - (1.0 - t).powi(3)
            }
            None => {
                commands.entity(id).try_insert(Appearing::default());
                0.0
            }
        };
        let size = Vec2::splat(
            set.enter_scale + (1.0 - set.enter_scale) * come,
        );
        match grown {
            Some(mut grown) if grown.scale != size => {
                grown.scale = size;
            }
            Some(_) => {}
            None => {
                commands.entity(id).try_insert(UiTransform {
                    scale: size,
                    ..UiTransform::IDENTITY
                });
            }
        }
        if node.border != UiRect::all(px(1.0)) {
            node.border = UiRect::all(px(1.0));
        }
        if layer.0 == HINT_LAYER
            && let Some(put) = stand(&node)
        {
            match glide {
                Some(mut glide) => {
                    // Any place but the one it was last drawn at is
                    // where the dock has just put it.
                    if put != glide.shown {
                        glide.to = put;
                    }
                    let k = 1.0 - (-set.drop_hint_rate * dt).exp();
                    let mut next = glide.shown.lerp(glide.to, k);
                    if next.abs_diff_eq(glide.to, 0.5) {
                        next = glide.to;
                    }
                    glide.shown = next;
                    if put != next {
                        node.left = px(next.x);
                        node.top = px(next.y);
                        node.width = px(next.z);
                        node.height = px(next.w);
                    }
                }
                None => {
                    commands.entity(id).try_insert(Gliding {
                        to: put,
                        shown: put,
                    });
                }
            }
        }
        fill.0 = pane.with_alpha(pane.alpha() * come);
        *edge =
            BorderColor::all(line.with_alpha(line.alpha() * come));
    }
}
