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

use super::{FeelSettings, MagicColors};
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

/// Makes the dock's drop hint a pane of lamplight with a brighter
/// edge that breathes, and the dragged tab a framed plate. Each fades
/// and grows in as it appears, and the hint glides from one place it
/// could land to the next.
pub(super) fn dress_drag(
    mut commands: Commands,
    time: Res<Time>,
    set: Res<FeelSettings>,
    theme: Themed,
    mut drawn: Query<Dragged, Without<ChildOf>>,
) {
    let colors = MagicColors::of(&theme);
    let breath = 0.5 + 0.5 * (time.elapsed_secs() * set.breath).sin();
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
                colors.hint.with_alpha(0.06 + 0.05 * breath),
                colors.hint.with_alpha(0.5 + 0.3 * breath),
            ),
            GHOST_LAYER => {
                (colors.ghost.with_alpha(0.9), colors.plate_edge)
            }
            _ => continue,
        };
        // Its share of the way in, 0 to 1, fast at first.
        let come = match age {
            Some(mut age) => {
                age.0 += time.delta_secs();
                let t = (age.0 / set.arrive).min(1.0);
                1.0 - (1.0 - t).powi(3)
            }
            None => {
                commands.entity(id).try_insert(Appearing::default());
                0.0
            }
        };
        let size = Vec2::splat(
            set.arrive_from + (1.0 - set.arrive_from) * come,
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
                    let k = 1.0 - (-set.glide * dt).exp();
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

#[cfg(test)]
mod tests {
    use bevy::app::{App, Update};
    use bevy::color::Color;
    use bevy_fynix::Theme;
    use hamlet_assets::Tuning;

    use super::*;
    use crate::testing::step;
    use crate::theme::HamletTheme;

    #[test]
    fn only_the_docks_hint_and_ghost_are_dressed() {
        let mut app = App::new();
        app.insert_resource(Theme(HamletTheme::designed()))
            .init_resource::<FeelSettings>()
            .init_resource::<Time>()
            .add_systems(Update, dress_drag);
        let grey = BackgroundColor(Color::WHITE);
        let hint = app
            .world_mut()
            .spawn((Node::default(), grey, GlobalZIndex(HINT_LAYER)))
            .id();
        let other = app
            .world_mut()
            .spawn((Node::default(), grey, GlobalZIndex(7)))
            .id();
        step(&mut app);
        let first =
            app.world().get::<BackgroundColor>(hint).unwrap().0;
        assert_eq!(first.alpha(), 0.0, "it comes in from nothing");
        for _ in 0..30 {
            step(&mut app);
        }
        let world = app.world();
        let pane = world.get::<BackgroundColor>(hint).unwrap().0;
        assert!(pane.alpha() > 0.0 && pane.alpha() < 0.2, "{pane:?}");
        assert_eq!(
            pane.with_alpha(1.0),
            HamletTheme::designed().palette.lamp
        );
        assert_eq!(
            world.get::<UiTransform>(hint).unwrap().scale,
            Vec2::ONE
        );
        assert_eq!(
            world.get::<Node>(hint).unwrap().border,
            UiRect::all(px(1.0))
        );
        assert_eq!(
            world.get::<BackgroundColor>(other).unwrap().0,
            Color::WHITE
        );
    }

    #[test]
    fn the_docks_hint_glides_to_where_it_is_put() {
        let mut app = App::new();
        app.insert_resource(Theme(HamletTheme::designed()))
            .init_resource::<FeelSettings>()
            .init_resource::<Time>()
            .add_systems(Update, dress_drag);
        let put = |x: f32| Node {
            left: px(x),
            top: px(0.0),
            width: px(100.0),
            height: px(50.0),
            ..Node::default()
        };
        let hint = app
            .world_mut()
            .spawn((put(0.0), GlobalZIndex(HINT_LAYER)))
            .id();
        step(&mut app);
        assert_eq!(
            app.world().get::<Node>(hint).unwrap().left,
            px(0.0)
        );

        *app.world_mut().get_mut::<Node>(hint).unwrap() = put(200.0);
        step(&mut app);
        let Val::Px(part) =
            app.world().get::<Node>(hint).unwrap().left
        else {
            panic!("placed in pixels");
        };
        assert!(part > 0.0 && part < 200.0, "{part}");
        for _ in 0..60 {
            step(&mut app);
        }
        assert_eq!(
            app.world().get::<Node>(hint).unwrap().left,
            px(200.0)
        );
    }
}
