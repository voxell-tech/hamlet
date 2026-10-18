//! The light under a title or a tab.

use bevy::color::Alpha;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::Has;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::picking::hover::Hovered;
use bevy::time::Time;
use bevy::ui::{
    BackgroundGradient, ColorStop, RadialGradient,
    RadialGradientShape, UiPosition, percent,
};

use super::control::Feel;
use super::{FeelSettings, MagicColors, ease};
use crate::feel::FootLight;
use crate::theme::Themed;

type FootLit<'a> = (
    Entity,
    &'a mut FootLight,
    Option<&'a Hovered>,
    Option<&'a Feel>,
    Has<BackgroundGradient>,
);

/// Eases each foot light toward the pointer and its hold, and
/// flares it at a press. The light is a pool from under the middle
/// of the word, as from a candle set below it.
pub(super) fn foot_lights(
    mut commands: Commands,
    time: Res<Time>,
    set: Res<FeelSettings>,
    theme: Themed,
    mut lights: Query<FootLit>,
) {
    let colors = MagicColors::of(&theme);
    let dt = time.delta_secs();
    for (node, mut light, hovered, feel, lit) in &mut lights {
        let over = hovered.is_some_and(|h| h.0);
        let want = f32::from(over || light.held);
        let level = ease(light.level, want, set.warm, dt);
        let flare = feel.map_or(0.0, |feel| feel.pulse);
        let steady = level == light.level && flare == 0.0;
        if steady && (lit || level == 0.0) {
            continue;
        }
        light.level = level;
        let strength = set.foot * level + set.flare * flare;
        if strength <= 0.0 {
            commands.entity(node).try_remove::<BackgroundGradient>();
            continue;
        }
        let hue = colors.foot;
        // Wider than the node and faint by its edges, so that the
        // light has no rim to it.
        let stops = vec![
            ColorStop::new(hue.with_alpha(strength), percent(0.0)),
            ColorStop::new(
                hue.with_alpha(strength * 0.5),
                percent(22.0),
            ),
            ColorStop::new(
                hue.with_alpha(strength * 0.2),
                percent(45.0),
            ),
            ColorStop::new(
                hue.with_alpha(strength * 0.06),
                percent(70.0),
            ),
            ColorStop::new(hue.with_alpha(0.0), percent(100.0)),
        ];
        commands.entity(node).try_insert(BackgroundGradient::from(
            RadialGradient::new(
                UiPosition::BOTTOM,
                RadialGradientShape::Ellipse(
                    percent(80.0),
                    percent(150.0 + 30.0 * flare),
                ),
                stops,
            ),
        ));
    }
}
