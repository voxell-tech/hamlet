//! The feel of the panels (docs/ui-warmth.md): a control's warmth and
//! sink, the sparks of a deed, the shake of a refusal and the
//! lamplight in a panel that waits for the chief.

mod backdrop;
mod control;
mod drag;
mod foot;
mod glow;

use bevy::app::{App, Update};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ui::{ColorStop, percent};

pub use self::backdrop::Lit;
pub(crate) use self::backdrop::lamplight;
pub use self::foot::FootLight;
pub(crate) use self::glow::{GlowPanel, glow_layer};
use crate::theme::HamletTheme;

/// The feel of the controls, the dock's hint and the panels'
/// backdrops.
pub fn plugin(app: &mut App) {
    app.init_resource::<FeelSettings>()
        .add_observer(control::answer)
        .add_systems(
            Update,
            (
                (
                    control::arm,
                    control::hover_tick,
                    control::press_tap,
                    control::feel,
                    control::light,
                    control::spill,
                    control::embers,
                    foot::foot_lights,
                )
                    .chain(),
                drag::dress_drag,
                backdrop::light,
            ),
        );
}

/// The meaning of a press on a button, which sets its light, its
/// sound and its motion.
#[derive(Component, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mood {
    /// A tab, a menu item or the like: it gives under a press and
    /// nothing more.
    #[default]
    Quiet,
    /// An ordinary lettered button.
    Plain,
    /// Lettering with no plate, such as a way back: lit as a plain
    /// button is, with the light about its words and its mark alone.
    Ink,
    /// The action a screen is for: a lamp that burns low at rest.
    Good,
    /// Something refused, stopped or thrown away.
    Bad,
    /// A text field, which is lit while it is typed in.
    Field,
}

impl Mood {
    /// The pointer coming onto it is heard.
    fn has_tick(self) -> bool {
        matches!(
            self,
            Self::Plain | Self::Ink | Self::Good | Self::Bad
        )
    }
}

/// The tuning of the feel.
#[derive(Resource, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FeelSettings {
    /// The rates at which a light follows the pointer, a press
    /// follows the button and a pulse or a shake dies away, each per
    /// second.
    pub warm: f32,
    pub give: f32,
    pub fade: f32,
    /// The share of its size a button loses while held and gains at
    /// the top of a pulse, and the depth it sinks while held, in
    /// pixels.
    pub squash: f32,
    pub pop: f32,
    pub sink: f32,
    /// The reach of a refused button's shake, in pixels, and its
    /// rate.
    pub shake: f32,
    pub rattle: f32,
    /// The light of a control, 0 to 1: a plain one, a refusing one
    /// and the good one under the pointer, a field typed in, and the
    /// good one at rest.
    pub lit_plain: f32,
    pub lit_bad: f32,
    pub lit_good: f32,
    pub lit_field: f32,
    pub banked: f32,
    /// The share of its light a held control loses under the hand,
    /// and the share of the rest it gains as its work is done.
    pub cover: f32,
    pub kindle: f32,
    /// The share of its light a flame wavers by.
    pub waver: f32,
    /// The share of a control's light that falls outside it, the
    /// share of that a refusing one or a field throws, and the
    /// distance it falls, in pixels, at rest under the pointer and
    /// more at the top of a pulse.
    pub spill: f32,
    pub smoulder: f32,
    pub reach: f32,
    pub burst: f32,
    /// The sparks from the good action once done: their number,
    /// their rise in pixels, their seconds and their size in pixels.
    pub embers: u32,
    pub rise: f32,
    pub ember_life: f32,
    pub ember_size: f32,
    /// The strength of the light under a title or a tab at rest
    /// under the pointer, and the strength a press adds.
    pub foot: f32,
    pub flare: f32,
    /// Seconds a thing takes to come in, and the share of its size
    /// it starts from.
    pub arrive: f32,
    pub arrive_from: f32,
    /// The rate at which the dock's hint glides to a new place, per
    /// second, and the rate at which it breathes.
    pub glide: f32,
    pub breath: f32,
    /// Seconds the pointer rests on an agent before its card shows,
    /// the seconds the card takes to fade in and out, and the share
    /// of its size it grows from.
    pub hover_wait: f32,
    pub hover_in: f32,
    pub hover_out: f32,
    pub hover_from: f32,
    /// The rate at which a panel's lamp and its flash settle, per
    /// second, and the amount past its rest the lamp is turned as
    /// something new comes to wait.
    pub settle: f32,
    pub ring: f32,
}

/// The colours of the light.
#[derive(Clone, PartialEq)]
pub(crate) struct MagicColors {
    /// The light of a control, the heart of its flame, and the light
    /// of a control that refuses.
    pub lamp: Color,
    pub flame: Color,
    pub coal: Color,
    /// The glass of the good action, away from its flame and near
    /// it.
    pub glass_low: Color,
    pub glass_high: Color,
    /// A spark from the good action.
    pub ember: Color,
    /// The frame of the tab being dragged.
    pub plate_edge: Color,
    /// The lamplight in a panel that something waits in, and the
    /// breath of magic over one whose agent has changed.
    pub alarm: Color,
    pub flash: Color,
    /// The light under a title or a tab.
    pub foot: Color,
    /// The place a dragged tab would land, and the tab being
    /// dragged.
    pub hint: Color,
    pub ghost: Color,
}

impl MagicColors {
    /// The colours of the light in `theme`.
    pub fn of(theme: &HamletTheme) -> Self {
        let p = &theme.palette;
        Self {
            lamp: p.lamp,
            flame: p.flame,
            coal: p.coal,
            glass_low: p.lamp_low,
            glass_high: p.lamp_high,
            ember: p.ember,
            plate_edge: p.brass_5,
            alarm: p.lamp,
            flash: p.rune,
            foot: p.lamp,
            hint: p.lamp,
            ghost: p.black,
        }
    }
}

/// The stops of a radial gradient whose light falls off as a bell,
/// `exp(-sharp * t * t)` at `t` from 0 at its middle to 1 at its
/// end, each stop coloured by `paint` from how much light is left.
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
fn ease(value: f32, to: f32, rate: f32, dt: f32) -> f32 {
    let next = to + (value - to) * (-rate * dt).exp();
    if (next - to).abs() < 0.002 { to } else { next }
}

/// The unsteadiness of a flame at `now`, -1 to 1, its own for each
/// `seed`.
fn waver(now: f32, seed: f32) -> f32 {
    0.5 * (now * 5.3 + seed).sin()
        + 0.3 * (now * 8.9 + seed * 1.7).sin()
        + 0.2 * (now * 2.1 + seed * 0.6).sin()
}
