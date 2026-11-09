//! The glow under a title or a tab.

use core::f32::consts::TAU;

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

/// How much stronger a lamp's glow is than a title's: it is small
/// and meant to be seen without the pointer.
const LAMP_STRENGTH: f32 = 2.2;
/// A lamp's level swings by `LAMP_SWING` around `LAMP_LEVEL`, once
/// every `LAMP_PERIOD` seconds: a slow breath.
const LAMP_LEVEL: f32 = 0.85;
const LAMP_SWING: f32 = 0.15;
const LAMP_PERIOD: f32 = 4.0;

/// A title or a tab that glows from below under the pointer, at a
/// press, and for as long as it is `held`, when it also takes the
/// selected fill under the glow.
#[derive(Component, Default)]
pub struct Underglow {
    pub held: bool,
    /// A lamp: held on while what it marks is at work, with no
    /// selected fill, stronger, and breathing slowly.
    lamp: bool,
    level: f32,
    /// Whether the gradient last set has the selected fill in it.
    filled: bool,
}

impl Underglow {
    /// An underglow at rest, or held on.
    pub fn new(held: bool) -> Self {
        Self {
            held,
            lamp: false,
            level: 0.0,
            filled: false,
        }
    }

    /// A small glow under a mark, on for as long as it exists.
    pub fn lamp() -> Self {
        Self {
            held: true,
            lamp: true,
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
        let want = if glow.lamp {
            let turn = time.elapsed_secs() / LAMP_PERIOD * TAU;
            LAMP_LEVEL + LAMP_SWING * turn.sin()
        } else {
            f32::from(over || glow.held)
        };
        let level = ease(glow.level, want, set.glow_rate, dt);
        let flare = effect.map_or(0.0, |effect| effect.pulse);
        let fills = glow.held && !glow.lamp;
        let steady = level == glow.level
            && flare == 0.0
            && glow.filled == fills;
        if steady && (shown || level == 0.0) {
            continue;
        }
        glow.level = level;
        glow.filled = fills;
        // A lamp is small and its colour already deep, so a light
        // panel's gain would only turn it to a tan smudge.
        let (boost, gain) = if glow.lamp {
            (LAMP_STRENGTH, 1.0)
        } else {
            (1.0, set.gain(&theme))
        };
        let strength = (set.underglow * level
            + set.underglow_press * flare)
            * boost
            * gain;
        let mut layers = Vec::new();
        if fills {
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
