//! The effects of the panels (docs/ui-warmth.md): a control's glow
//! and press, the particles of a finished action, the shake of a
//! refusal and the highlight in a panel that waits for the chief,
//! which its tab carries while it is behind another.

mod control;
mod drag;
mod glow;
mod highlight;
mod tab;
mod underglow;

use bevy::app::{App, PostUpdate, Update};
use bevy::asset::Asset;
use bevy::color::{Color, Mix};
use bevy::ecs::component::Component;
use bevy::ecs::message::Message;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::world::World;
use bevy::reflect::TypePath;
use bevy::ui::{ColorStop, UiSystems, percent};
use hamlet_assets::{File, Tuning, TuningPlugin, follow};

pub use self::glow::{GlowPanel, glow_layer};
pub use self::highlight::{
    Attention, Highlight, Updated, highlight_layer,
};
pub use self::underglow::Underglow;
use crate::theme::HamletTheme;
use crate::theme::contrast::contrast;

/// The effects of the controls, the dock's drop hint and the
/// panels' highlights.
pub fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<EffectSettings>::default())
        .init_resource::<EffectSettings>()
        .init_resource::<Attention>()
        .add_message::<Updated>()
        .add_message::<ControlSound>()
        .add_observer(control::answer)
        .add_systems(
            Update,
            (
                (
                    control::arm,
                    control::hover_tick,
                    control::press_tap,
                    control::animate,
                    control::update_gradients,
                    control::outer_glow,
                    control::particles,
                    underglow::update_underglow,
                )
                    .chain(),
                drag::dress_drag,
                (highlight::update_highlights, tab::highlight_tabs)
                    .chain(),
            ),
        )
        // Once the clips are worked out for the frame, as that sets
        // each glow's clip back to its layer's.
        .add_systems(
            PostUpdate,
            control::cut_glows.after(UiSystems::PostLayout),
        );
}

/// A sound a control asks for. The kit plays nothing itself: the
/// app reads these and plays what it likes.
#[derive(Message, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ControlSound {
    /// The pointer came onto a control that is heard.
    Hover,
    /// A control was pressed.
    Press,
    /// A primary action was done.
    Confirm,
    /// A danger action was done.
    Deny,
}

/// The meaning of a press on a button, which sets its glow, its
/// sound and its motion.
#[derive(Component, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variant {
    /// A tab, a menu item or the like: it gives under a press and
    /// nothing more.
    #[default]
    Subtle,
    /// A standard lettered button.
    Default,
    /// Text with no fill, such as a way back: glows as a default
    /// button does, around its words and its icon alone.
    Text,
    /// The action a screen is for: a button with a dim glow at rest.
    Primary,
    /// Something refused, stopped or thrown away.
    Danger,
    /// A text input, which glows while it is typed in.
    Input,
}

impl Variant {
    /// The pointer coming onto it is heard.
    fn has_tick(self) -> bool {
        matches!(
            self,
            Self::Default | Self::Text | Self::Primary | Self::Danger
        )
    }
}

/// The tuning of the effects, as `assets/effects.toml` has it.
#[derive(
    Resource, Asset, TypePath, Clone, PartialEq, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct EffectSettings {
    /// The rates at which a glow follows the pointer, a press
    /// follows the button and a pulse or a shake dies away, each per
    /// second.
    pub glow_rate: f32,
    pub press_rate: f32,
    pub pulse_decay: f32,
    /// The share of its size a button loses while held and gains at
    /// the top of a pulse, and the depth it sinks while held, in
    /// pixels.
    pub press_scale: f32,
    pub pulse_scale: f32,
    pub press_offset: f32,
    /// The reach of a danger button's shake, in pixels, and its
    /// rate.
    pub shake_distance: f32,
    pub shake_rate: f32,
    /// The glow of a control, 0 to 1: a default one, a danger one
    /// and the primary one under the pointer, an input typed in, and
    /// the primary one at rest.
    pub glow_default: f32,
    pub glow_danger: f32,
    pub glow_primary: f32,
    pub glow_input: f32,
    pub glow_primary_idle: f32,
    /// The share of its glow a held control loses under the hand,
    /// and the share of the rest it gains as its work is done.
    pub press_dim: f32,
    pub done_boost: f32,
    /// The share of its glow that flickers.
    pub flicker: f32,
    /// The share of a control's glow that falls outside it, the
    /// share of that a danger button or an input throws, and the
    /// distance it falls, in pixels, at rest under the pointer and
    /// more at the top of a pulse.
    pub outer_glow: f32,
    pub outer_glow_danger: f32,
    pub outer_glow_radius: f32,
    pub outer_glow_pulse_radius: f32,
    /// The particles from the primary action once done: their
    /// number, their rise in pixels, their seconds and their size in
    /// pixels.
    pub particle_count: u32,
    pub particle_rise: f32,
    pub particle_lifetime: f32,
    pub particle_size: f32,
    /// The strength of the glow under a title or a tab at rest
    /// under the pointer, and the strength a press adds.
    pub underglow: f32,
    pub underglow_press: f32,
    /// Seconds a thing takes to come in, and the share of its size
    /// it starts from.
    pub enter_duration: f32,
    pub enter_scale: f32,
    /// The rate at which the dock's drop hint moves to a new place,
    /// per second, and the rate at which it pulses.
    pub drop_hint_rate: f32,
    pub drop_hint_pulse_rate: f32,
    /// Seconds the pointer rests on an agent before its card shows,
    /// the seconds the card takes to fade in and out, and the share
    /// of its size it grows from.
    pub hover_wait: f32,
    pub hover_in: f32,
    pub hover_out: f32,
    pub hover_from: f32,
    /// The rate at which a panel's highlight and its flash settle,
    /// per second, and the amount past its rest the highlight is
    /// raised as something new comes to wait.
    pub highlight_rate: f32,
    pub highlight_boost: f32,
    /// How many times as strong a glow is laid over a light panel,
    /// where the same glow shows far less than over a dark one.
    pub light_gain: f32,
    /// Seconds a popup, a menu or a tooltip takes to fade in.
    pub popup_in: f32,
}

impl EffectSettings {
    /// How strongly a glow is laid over `theme`'s panels:
    /// [`Self::light_gain`] times over a light one.
    pub fn gain(&self, theme: &HamletTheme) -> f32 {
        if lit_panels(theme) {
            self.light_gain
        } else {
            1.0
        }
    }
}

impl Tuning for EffectSettings {
    const FILE: File = hamlet_assets::EFFECTS;

    fn apply(&self, was: &Self, world: &mut World) {
        follow(world, was, self);
    }
}

impl Default for EffectSettings {
    fn default() -> Self {
        Self::designed()
    }
}

/// The colours of the effects.
#[derive(Clone, PartialEq)]
pub struct EffectColors {
    /// The glow of a control, the bright centre of that glow, and
    /// the glow of a danger button.
    pub glow: Color,
    pub glow_core: Color,
    pub glow_danger: Color,
    /// The fill of the primary button, away from its glow and near
    /// it.
    pub primary_low: Color,
    pub primary_high: Color,
    /// A particle from the primary action.
    pub particle: Color,
    /// The border of the tab being dragged.
    pub drag_border: Color,
    /// The highlight in a panel that something waits in, and the
    /// flash over one whose agent has changed.
    pub attention: Color,
    pub flash: Color,
    /// The glow under a title or a tab.
    pub underglow: Color,
    /// The place a dragged tab would land, and the tab being
    /// dragged.
    pub drop_hint: Color,
    pub drag_ghost: Color,
}

impl EffectColors {
    /// The colours of the effects in `theme`. Over a light panel the
    /// glows are deepened toward the accent: a pale glow on a pale
    /// panel is all but unseen.
    pub fn of(theme: &HamletTheme) -> Self {
        let p = &theme.palette;
        let lamp = |glow: Color| {
            if lit_panels(theme) {
                glow.mix(&p.accent, LIGHT_DEEPEN)
            } else {
                glow
            }
        };
        Self {
            glow: lamp(p.glow),
            glow_core: lamp(p.glow_core),
            glow_danger: p.danger_glow,
            primary_low: p.primary_low,
            primary_high: p.primary_high,
            particle: p.particle,
            drag_border: p.gold_5,
            attention: lamp(p.glow),
            flash: p.agent,
            underglow: if lit_panels(theme) {
                p.glow.mix(&p.accent, UNDERGLOW_DEEPEN)
            } else {
                p.glow
            },
            drop_hint: lamp(p.glow),
            drag_ghost: p.surface,
        }
    }
}

/// How far a glow over a light panel is taken toward the accent.
const LIGHT_DEEPEN: f32 = 0.45;

/// The same for the glow under a title or a tab, which marks the tab
/// shown and so is taken further.
const UNDERGLOW_DEEPEN: f32 = 0.75;

/// Whether `theme`'s panels are light: dark words read better on
/// them than light ones.
pub fn lit_panels(theme: &HamletTheme) -> bool {
    let surface = theme.palette.surface;
    contrast(surface, Color::BLACK) > contrast(surface, Color::WHITE)
}

/// The stops of a radial gradient whose glow falls off as a bell,
/// `exp(-sharp * t * t)` at `t` from 0 at its middle to 1 at its
/// end, each stop coloured by `paint` from how much glow is left.
fn pool(sharp: f32, paint: impl Fn(f32) -> Color) -> Vec<ColorStop> {
    const STOPS: u8 = 8;
    (0..=STOPS)
        .map(|step| {
            let t = f32::from(step) / f32::from(STOPS);
            ColorStop::new(
                paint((-sharp * t * t).exp()),
                percent(t * 100.0),
            )
        })
        .collect()
}

/// Moves `value` toward `to` at `rate` per second.
pub(crate) fn ease(value: f32, to: f32, rate: f32, dt: f32) -> f32 {
    let next = to + (value - to) * (-rate * dt).exp();
    if (next - to).abs() < 0.002 { to } else { next }
}

/// The flicker of a glow at `now`, -1 to 1, its own for each
/// `seed`.
fn flicker(now: f32, seed: f32) -> f32 {
    0.5 * (now * 5.3 + seed).sin()
        + 0.3 * (now * 8.9 + seed * 1.7).sin()
        + 0.2 * (now * 2.1 + seed * 0.6).sin()
}
