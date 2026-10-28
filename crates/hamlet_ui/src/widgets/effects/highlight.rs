//! The highlight that falls into a panel: a glow while something
//! waits for the chief, and a flash when what it shows has changed.

use bevy::color::{Alpha, Color, Mix};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::{Message, MessageReader};
use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, Local, Query, Res};
use bevy::picking::Pickable;
use bevy::time::Time;
use bevy::ui::{
    BackgroundGradient, Gradient, Overflow, OverrideClip,
    PositionType, RadialGradient, RadialGradientShape, UiPosition,
    UiRect, Val, percent, px,
};
use bevy_fynix::views::{BehaviorExt, frame};
use hamlet_client::waits::Waiting;

use super::{EffectColors, EffectSettings, ease, flicker, pool};
use crate::View;
use crate::theme::Themed;

/// How far the attention glow reaches down from the panel's top,
/// and the flash's, in logical pixels.
const GLOW_DEEP: f32 = 110.0;
const FLASH_DEEP: f32 = 60.0;

/// The pool of a glow is this many times as far as it is deep, so
/// that it has faded out at its end.
const FADE_OUT: f32 = 1.6;

/// A panel that a highlight falls into.
#[derive(Component, Clone, Copy, Default)]
pub struct Highlight {
    /// A glow shows over it while something waits for the chief.
    pub attention: bool,
    /// It flashes when it is told that what it shows changed.
    pub flash: bool,
}

/// What a panel that flashes shows has changed, whether or not the
/// panel is in front. Its tab flashes too while it is behind another.
#[derive(Message, Clone, Copy, Debug)]
pub struct Updated;

/// How far a panel's glow is raised and how much of its flash is
/// left, kept on the node that draws them.
#[derive(Component, Default)]
pub(super) struct HighlightState {
    pub attention: f32,
    pub flash: f32,
    /// How many things wait in the panel, for its tab's badge.
    pub count: usize,
}

/// The node over the top of a panel that its highlight is drawn on,
/// under the panel's contents.
pub(crate) fn highlight_layer(highlight: Highlight) -> impl View {
    frame()
        .position(PositionType::Absolute)
        .inset(UiRect {
            left: px(0.0),
            right: px(0.0),
            top: px(0.0),
            bottom: Val::Auto,
        })
        .height(px(GLOW_DEEP * FADE_OUT))
        // Past the panel's padding, to its edge, as the glow layer.
        .overflow(Overflow::clip())
        .tagged(highlight)
        .tagged(HighlightState::default())
        .tagged(OverrideClip)
        .tagged(Pickable::IGNORE)
}

/// A pool of glow from a point just over the node's top, a third of
/// the way along, that is as deep as `deep` pixels.
fn glow_pool(deep: f32, paint: impl Fn(f32) -> Color) -> Gradient {
    Gradient::from(RadialGradient::new(
        UiPosition::top_left(percent(32.0), px(-18.0)),
        RadialGradientShape::Ellipse(
            percent(70.0 * FADE_OUT),
            px(deep * FADE_OUT),
        ),
        pool(FADE_OUT * FADE_OUT * 1.6, paint),
    ))
}

/// Shows a panel's glow while something waits, raises it for a
/// moment as something new comes, and flashes a panel when what it
/// shows changes.
pub(super) fn update_highlights(
    mut commands: Commands,
    time: Res<Time>,
    set: Res<EffectSettings>,
    theme: Themed,
    mut updated: MessageReader<Updated>,
    waiting: Query<(), With<Waiting>>,
    mut panels: Query<(
        Entity,
        &Highlight,
        &mut HighlightState,
        Option<&BackgroundGradient>,
    )>,
    mut counted: Local<usize>,
) {
    let dt = time.delta_secs();
    let changed = updated.read().count() > 0;
    let count = waiting.iter().count();
    let rung = count > *counted;
    *counted = count;
    let attention = f32::from(count > 0);
    let colors = EffectColors::of(&theme);
    // A glow is never quite still.
    let brightness =
        1.0 + set.flicker * flicker(time.elapsed_secs(), 1.0);
    for (panel, highlight, mut state, gradient) in &mut panels {
        let raised = if !highlight.attention {
            0.0
        } else if rung {
            state.attention.max(attention) + set.highlight_boost
        } else {
            ease(state.attention, attention, set.highlight_rate, dt)
        };
        let flash = if highlight.flash && changed {
            1.0
        } else {
            ease(state.flash, 0.0, set.highlight_rate, dt)
        };
        if state.attention != raised {
            state.attention = raised;
        }
        if state.flash != flash {
            state.flash = flash;
        }
        let waits = if highlight.attention { count } else { 0 };
        if state.count != waits {
            state.count = waits;
        }
        if raised <= 0.0 && flash <= 0.0 {
            if gradient.is_some() {
                commands
                    .entity(panel)
                    .try_remove::<BackgroundGradient>();
            }
            continue;
        }
        let mut glows = Vec::new();
        if raised > 0.0 {
            let call = raised * brightness;
            // Brightest close under the source, and thin over the
            // rest.
            glows.push(glow_pool(GLOW_DEEP, |near| {
                let glow = near * call;
                let strength = (glow * 0.07 + glow * glow * 0.07)
                    .clamp(0.0, 1.0);
                colors
                    .attention
                    .mix(
                        &colors.glow_core,
                        (glow * 0.35).clamp(0.0, 1.0),
                    )
                    .with_alpha(strength)
            }));
        }
        if flash > 0.0 {
            glows.push(glow_pool(FLASH_DEEP, |near| {
                colors.flash.with_alpha(flash * flash * near * 0.12)
            }));
        }
        commands.entity(panel).try_insert(BackgroundGradient(glows));
    }
}
