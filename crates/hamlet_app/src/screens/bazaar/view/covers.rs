//! Pictures drawn: a listing's cover under the head of its page, and
//! the cover chosen at the seller's desk.

use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    BorderColor, BorderRadius, Node, UiRect, percent, px,
};
use bevy_fynix::views::FrameProps;
use bevy_fynix::{ViewExt, derived, keyed};
use hamlet_ui::styles::{bare, note};
use hamlet_ui::{AnyView, Cx};

use super::desk::Desk;
use super::parts::nothing;
use crate::bazaar::covers::{self, Covers, Held, Pick, Picked};
use crate::bazaar::market::Market;
use crate::pictures::Drawn;
use crate::roll::{either, moved};

/// The most a cover is drawn at, on its listing's page and at the
/// desk.
pub(super) const PAGE: Vec2 = Vec2::new(320.0, 180.0);

/// The cover at `url` on the bazaar's server, once it came.
pub(super) fn cover(url: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        covers::want(cx.world, &url);
        let read = url.clone();
        let held = derived(move |world: &World| {
            world.resource::<Covers>().get(&read).cloned()
        })
        .when(moved::<Covers>());
        cx.build(
            keyed(held, |held| match held {
                Some(Held::Shown(drawn)) => picture(drawn.clone()),
                Some(Held::Fetching) => {
                    note("Fetching its cover...").boxed()
                }
                _ => nothing(),
            })
            .within(bare()),
        )
    })
}

/// The cover chosen at the desk, else that of the listing changed
/// there.
pub(super) fn chosen() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        if let Some(url) = now_of(cx.world) {
            covers::want(cx.world, &url);
        }
        let drawn = derived(|world: &World| {
            match &world.resource::<Pick>().state {
                Picked::Got(picture) => picture.drawn.clone(),
                Picked::Nothing => now_of(world).and_then(|url| {
                    match world.resource::<Covers>().get(&url) {
                        Some(Held::Shown(drawn)) => {
                            Some(drawn.clone())
                        }
                        _ => None,
                    }
                }),
                Picked::Fetching | Picked::Failed(_) => None,
            }
        })
        .when(either(
            either(moved::<Covers>(), moved::<Pick>()),
            moved::<Desk>(),
        ));
        cx.build(
            keyed(drawn, |drawn| match drawn {
                Some(drawn) => picture(drawn.clone()),
                None => nothing(),
            })
            .within(bare()),
        )
    })
}

/// The address of the cover of the listing changed at the desk, if it
/// has one.
pub(super) fn now_of(world: &World) -> Option<String> {
    let id = world.resource::<Desk>().form.id.clone()?;
    world.resource::<Market>().lot(&id)?.cover_url.clone()
}

/// `drawn` in a frame as wide and as tall as [`PAGE`], as a picture
/// of a reply is framed, cut to fill it.
fn picture(drawn: Drawn) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let within = cx.build(bare().width(percent(100.0)));
        let theme = cx.theme();
        let edge = 1.0;
        let outer = PAGE + 2.0 * edge;
        let mut image = ImageNode::new(drawn.image.clone())
            .with_mode(NodeImageMode::Stretch);
        image.rect = Some(filled(drawn.size.as_vec2(), PAGE));
        cx.world.spawn((
            Node {
                width: px(outer.x),
                max_width: percent(100.0),
                aspect_ratio: Some(outer.x / outer.y),
                border: UiRect::all(px(edge)),
                border_radius: BorderRadius::all(px(theme
                    .space
                    .round)),
                ..Node::default()
            },
            BorderColor::all(theme.palette.gold_6),
            image,
            ChildOf(within),
        ));
        within
    })
}

/// The part of a picture of `size` that fills a frame shaped as
/// `frame`, cut evenly from both sides that overhang.
fn filled(size: Vec2, frame: Vec2) -> Rect {
    let wanted = frame.x / frame.y;
    let cut = if size.x / size.y > wanted {
        Vec2::new(size.y * wanted, size.y)
    } else {
        Vec2::new(size.x, size.x / wanted)
    };
    Rect::from_center_size(size / 2.0, cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cover_is_cut_to_fill_its_frame() {
        let tall = filled(Vec2::new(100.0, 400.0), PAGE);
        assert_eq!(tall.width(), 100.0);
        assert!((tall.height() - 56.25).abs() < 0.01);
        assert_eq!(tall.center(), Vec2::new(50.0, 200.0));
        let wide = filled(Vec2::new(1600.0, 450.0), PAGE);
        assert_eq!((wide.width(), wide.height()), (800.0, 450.0));
    }
}
