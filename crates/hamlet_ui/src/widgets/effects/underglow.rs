//! The glow under a title or a tab.

use bevy::color::Alpha;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::Has;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::picking::hover::Hovered;
use bevy::time::Time;
use bevy::ui::{
    BackgroundGradient, ColorStop, Gradient, LinearGradient,
    RadialGradient, RadialGradientShape, UiPosition, percent,
};

use super::control::ControlEffect;
use super::{EffectColors, EffectSettings, ease};
use crate::theme::Themed;

/// The most a light panel's gain strengthens an underglow: past it
/// the glow pools into an orange smudge in its box.
const GAIN_CAP: f32 = 1.5;

/// A title or a tab that glows from below under the pointer, at a
/// press, and for as long as it is `held`, when it also takes the
/// selected fill under the glow.
#[derive(Component, Default)]
pub struct Underglow {
    pub held: bool,
    level: f32,
    /// Whether the gradient last set has the selected fill in it.
    filled: bool,
}

impl Underglow {
    /// An underglow at rest, or held on.
    pub fn new(held: bool) -> Self {
        Self {
            held,
            level: 0.0,
            filled: false,
        }
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
        let steady = level == glow.level
            && flare == 0.0
            && glow.filled == glow.held;
        if steady && (shown || level == 0.0) {
            continue;
        }
        glow.level = level;
        glow.filled = glow.held;
        let strength = (set.underglow * level
            + set.underglow_press * flare)
            * set.gain(&theme).min(GAIN_CAP);
        let mut layers = Vec::new();
        if glow.held {
            let fill = theme.palette.selected;
            layers.push(Gradient::from(LinearGradient::to_right(
                vec![
                    ColorStop::new(fill, percent(0.0)),
                    ColorStop::new(fill, percent(100.0)),
                ],
            )));
        }
        if strength <= 0.0 && layers.is_empty() {
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
        layers.push(Gradient::from(RadialGradient::new(
            UiPosition::BOTTOM,
            RadialGradientShape::Ellipse(
                percent(80.0),
                percent(150.0 + 30.0 * flare),
            ),
            stops,
        )));
        commands.entity(node).try_insert(BackgroundGradient(layers));
    }
}
