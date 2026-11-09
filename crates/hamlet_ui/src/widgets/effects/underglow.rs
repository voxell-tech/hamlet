//! The glow under a title or a tab.

use core::f32::consts::TAU;

use bevy::color::{Alpha, Color};
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
use super::{EffectColors, EffectSettings, ease, lit_panels, pool};
use crate::theme::Themed;

/// The opacity of a lamp's middle at its brightest, over a dark
/// panel and over a light one. The mark sits on the middle, so these
/// are the most light that keeps the mark at 3:1.
const LAMP_PEAK_DARK: f32 = 0.34;
const LAMP_PEAK_LIGHT: f32 = 0.40;
/// How sharply a lamp's glow falls off, as in [`pool`]: to nothing
/// before its edge.
const LAMP_SHARP: f32 = 6.0;
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
    /// A lamp: a round glow on a square node around what is at
    /// work, on while it lasts and breathing slowly.
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

    /// A round glow for a square node around a mark, on for as long
    /// as it exists.
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
        if glow.lamp {
            if level != glow.level || !shown {
                glow.level = level;
                let peak = if lit_panels(&theme) {
                    LAMP_PEAK_LIGHT
                } else {
                    LAMP_PEAK_DARK
                };
                commands.entity(node).try_insert(BackgroundGradient(
                    vec![lamp_glow(theme.palette.glow, peak * level)],
                ));
            }
            continue;
        }
        let fills = glow.held;
        let steady = level == glow.level
            && flare == 0.0
            && glow.filled == fills;
        if steady && (shown || level == 0.0) {
            continue;
        }
        glow.level = level;
        glow.filled = fills;
        let strength = (set.underglow * level
            + set.underglow_press * flare)
            * set.gain(&theme);
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

/// A round glow in `hue`, `peak` opaque in the middle of its node and
/// nothing at the closest side, so that it has no rim to it.
fn lamp_glow(hue: Color, peak: f32) -> Gradient {
    Gradient::from(RadialGradient::new(
        UiPosition::CENTER,
        RadialGradientShape::ClosestSide,
        pool(LAMP_SHARP, |near| hue.with_alpha(peak * near)),
    ))
}
