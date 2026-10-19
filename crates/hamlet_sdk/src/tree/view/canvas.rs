//! A canvas: a box of 2D shapes, each a node of the UI that is
//! positioned, sized, rounded and turned, so they are drawn as the
//! rest of the panel is and stay sharp at any scale.

use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::math::Rot2;
use bevy::text::{FontSize, TextColor, TextFont};
use bevy::ui::widget::Text;
use bevy::ui::{
    BackgroundColor, BorderRadius, Node, Overflow, PositionType,
    UiTransform, px,
};
use bevy_fynix::views::column;
use bevy_fynix::{ViewExt, keyed};

use super::bind::At;
use crate::looks::bare;
use crate::theme::Palette;
use crate::tree::canvas::{Canvas, Shape};
use crate::{AnyView, Cx};

/// The view of the canvas at `at`. It is built again when the plugin
/// draws other shapes.
pub(super) fn view(at: &At) -> AnyView {
    keyed(at.canvas(), surface).within(bare()).boxed()
}

fn surface(canvas: &Canvas) -> AnyView {
    let canvas = canvas.clone();
    AnyView::new(move |cx: &mut Cx| {
        let palette = cx.theme().palette.clone();
        let root = cx.build(column(()));
        if let Some(mut node) = cx.world.get_mut::<Node>(root) {
            node.width = px(canvas.width);
            node.height = px(canvas.height);
            node.flex_shrink = 0.0;
            node.overflow = Overflow::clip();
        }
        for shape in &canvas.shapes {
            draw(cx, root, shape, &palette);
        }
        root
    })
}

fn draw(cx: &mut Cx, root: Entity, shape: &Shape, palette: &Palette) {
    match shape {
        Shape::Rect { x, y, w, h, paint } => {
            cx.world.spawn((
                placed(*x, *y, *w, *h),
                BackgroundColor(paint.colour(palette)),
                ChildOf(root),
            ));
        }
        Shape::Circle { x, y, r, paint } => {
            let mut node = placed(x - r, y - r, 2.0 * r, 2.0 * r);
            node.border_radius = BorderRadius::MAX;
            cx.world.spawn((
                node,
                BackgroundColor(paint.colour(palette)),
                ChildOf(root),
            ));
        }
        Shape::Line {
            from,
            to,
            width,
            paint,
        } => {
            let (dx, dy) = (to.0 - from.0, to.1 - from.1);
            let length = dx.hypot(dy);
            let middle =
                ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
            let mut node = placed(
                middle.0 - length / 2.0,
                middle.1 - width / 2.0,
                length,
                *width,
            );
            node.border_radius = BorderRadius::MAX;
            cx.world.spawn((
                node,
                UiTransform::from_rotation(Rot2::radians(
                    dy.atan2(dx),
                )),
                BackgroundColor(paint.colour(palette)),
                ChildOf(root),
            ));
        }
        Shape::Text {
            x,
            y,
            text,
            size,
            paint,
        } => {
            cx.world.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(*x),
                    top: px(*y),
                    ..Node::default()
                },
                Text::new(text.clone()),
                TextFont {
                    font_size: FontSize::Px(*size),
                    ..TextFont::default()
                },
                TextColor(paint.colour(palette)),
                ChildOf(root),
            ));
        }
    }
}

/// A box of the canvas at (`x`, `y`) of `w` by `h` pixels.
fn placed(x: f32, y: f32, w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(x),
        top: px(y),
        width: px(w),
        height: px(h),
        ..Node::default()
    }
}
