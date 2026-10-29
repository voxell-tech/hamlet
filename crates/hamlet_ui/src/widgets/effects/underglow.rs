//! The glow under a title or a tab.

use bevy::color::Alpha;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::Has;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::picking::hover::Hovered;
use bevy::time::Time;
use bevy::ui::{
    BackgroundGradient, ColorStop, RadialGradient,
    RadialGradientShape, UiPosition, percent,
};

use super::control::ControlEffect;
use super::{EffectColors, EffectSettings, ease};
use crate::theme::Themed;

/// A title or a tab that glows from below under the pointer, at a
/// press, and for as long as it is `held`.
#[derive(Component, Default)]
pub struct Underglow {
    pub held: bool,
    level: f32,
}

impl Underglow {
    /// An underglow at rest, or held on.
    pub fn new(held: bool) -> Self {
        Self { held, level: 0.0 }
    }
}

type Glowing<'a> = (
    Entity,
    &'a mut Underglow,
    Option<&'a Hovered>,
    Option<&'a ControlEffect>,
    Has<BackgroundGradient>,
);

/// Eases each underglow toward the pointer and its hold, and
/// flares it at a press. The glow is a pool from under the middle
/// of the word.
pub(super) fn update_underglow(
    mut commands: Commands,
    time: Res<Time>,
    set: Res<EffectSettings>,
    theme: Themed,
    mut glows: Query<Glowing>,
) {
    let colors = EffectColors::of(&theme);
    let dt = time.delta_secs();
    for (node, mut glow, hovered, effect, shown) in &mut glows {
        let over = hovered.is_some_and(|h| h.0);
        let want = f32::from(over || glow.held);
        let level = ease(glow.level, want, set.glow_rate, dt);
        let flare = effect.map_or(0.0, |effect| effect.pulse);
        let steady = level == glow.level && flare == 0.0;
        if steady && (shown || level == 0.0) {
            continue;
        }
        glow.level = level;
        let strength =
            set.underglow * level + set.underglow_press * flare;
        if strength <= 0.0 {
            commands.entity(node).try_remove::<BackgroundGradient>();
            continue;
        }
        let hue = colors.underglow;
        // Wider than the node and faint by its edges, so that the
        // glow has no rim to it.
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
