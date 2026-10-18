//! The look of the panels (docs/ui-warmth.md): warm near-black
//! surfaces, brass frames, and lamplight as the one colour that
//! glows. Its values are in `assets/theme.toml`, and nothing of the
//! panels is built until that file has loaded.

use std::ops::Deref;
use std::time::Duration;

use bevy::app::{App, Update};
use bevy::asset::Asset;
use bevy::color::{Color, LinearRgba, Luminance};
use bevy::ecs::message::Message;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, If, Res, ResMut, SystemParam};
use bevy::ecs::world::World;
use bevy::log::error;
use bevy::prelude::ClearColor;
use bevy::reflect::TypePath;
use bevy::state::app::AppExtStates;
use bevy::state::condition::in_state;
use bevy::state::state::{NextState, States};
use bevy::ui::{BoxShadow, px};
use bevy_fynix::Theme;
use bevy_fynix::tokens::{
    Curve, Motion, MotionTokens, SpacingTokens, SurfaceTokens,
    TextTokens, Tone,
};
use hamlet_assets::{
    File, Loaded, Loading, Tuning, TuningPlugin, tint,
};
use serde::{Deserialize, Serialize};

pub fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<HamletTheme>::default())
        .add_message::<ThemeChanged>()
        .init_state::<Look>()
        .add_systems(Update, dress.run_if(in_state(Look::Loading)));
}

/// The theme, for a system: one that takes it waits for the theme to
/// load.
#[derive(SystemParam)]
pub struct Themed<'w> {
    theme: If<Res<'w, Theme<HamletTheme>>>,
}

impl Deref for Themed<'_> {
    type Target = HamletTheme;

    fn deref(&self) -> &HamletTheme {
        &self.theme.0.0
    }
}

/// Whether the theme has loaded. The panels are built on
/// [`Look::Ready`].
#[derive(
    States, Clone, Copy, PartialEq, Eq, Hash, Debug, Default,
)]
pub enum Look {
    #[default]
    Loading,
    Ready,
}

/// The panels' theme, as `assets/theme.toml` has it.
#[derive(
    Asset, TypePath, Clone, PartialEq, Debug, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct HamletTheme {
    pub palette: Palette,
    pub text: TextSizes,
    pub space: Spacing,
    pub shadow: Shadow,
    pub motion: Timing,
}

impl Tuning for HamletTheme {
    const FILE: File = hamlet_assets::THEME;

    fn apply(&self, was: &Self, world: &mut World) {
        // Until the panels are built, the theme comes in by `dress`.
        if self != was && world.contains_resource::<Theme<Self>>() {
            insert_theme(world, self.clone());
            world.write_message(ThemeChanged);
        }
    }
}

/// Every colour of the panels.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    /// A panel: soot, the darkest warm surface but for a well.
    #[serde(with = "tint")]
    pub black: Color,
    /// Text that stands back.
    #[serde(with = "tint")]
    pub grey_1: Color,
    /// The wood of a button, and that wood under the pointer.
    #[serde(with = "tint")]
    pub grey_4: Color,
    #[serde(with = "tint")]
    pub wood_hover: Color,
    /// A seam between two surfaces, and the tint of what is
    /// selected.
    #[serde(with = "tint")]
    pub seam: Color,
    /// Parchment, for what is read.
    #[serde(with = "tint")]
    pub brass_1: Color,
    /// Brass, from what is lettered in it down to a hairline.
    #[serde(with = "tint")]
    pub brass_2: Color,
    #[serde(with = "tint")]
    pub brass_3: Color,
    #[serde(with = "tint")]
    pub brass_4: Color,
    #[serde(with = "tint")]
    pub brass_5: Color,
    #[serde(with = "tint")]
    pub brass_6: Color,
    /// A button's fill under the pointer.
    #[serde(with = "tint")]
    pub brass_hover: Color,
    /// HamletTheme: what is live, what waits, and every glow.
    #[serde(with = "tint")]
    pub lamp: Color,
    /// The heart of a flame.
    #[serde(with = "tint")]
    pub flame: Color,
    /// A spark that rises from what was just done.
    #[serde(with = "tint")]
    pub ember: Color,
    /// The glass of a lamp that burns low, away from its flame and
    /// near it.
    #[serde(with = "tint")]
    pub lamp_low: Color,
    #[serde(with = "tint")]
    pub lamp_high: Color,
    /// The agents' own magic, as the world draws it.
    #[serde(with = "tint")]
    pub rune: Color,
    /// A card in a list: a shade above the panel, and no more.
    #[serde(with = "tint")]
    pub card: Color,
    /// The floor of a text field, sunk below the panel.
    #[serde(with = "tint")]
    pub well: Color,
    /// The plate under code in a reply.
    #[serde(with = "tint")]
    pub plate: Color,
    /// Words selected in a message, lit behind them.
    #[serde(with = "tint")]
    pub chosen: Color,
    /// The warmth that gathers at the foot of a surface.
    #[serde(with = "tint")]
    pub hearth: Color,
    /// Red, for what is refused, stopped or at fault.
    #[serde(with = "tint")]
    pub red: Color,
    /// A refusing button's plate, and its glow.
    #[serde(with = "tint")]
    pub red_deep: Color,
    /// The frame of a button that ends something, while it rests,
    /// and its plate under the pointer.
    #[serde(with = "tint")]
    pub red_low: Color,
    #[serde(with = "tint")]
    pub red_hover: Color,
    #[serde(with = "tint")]
    pub coal: Color,
    /// Moss, for a line put in a file, and the plate under it.
    #[serde(with = "tint")]
    pub moss: Color,
    #[serde(with = "tint")]
    pub moss_deep: Color,
}

/// The sizes of the lettering.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSizes {
    pub body: f32,
    pub small: f32,
}

/// The gaps, heights and corners of the panels.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spacing {
    pub gap: f32,
    /// The height of a row.
    pub row: f32,
    /// The corner of a control and of a card, theme round. Frames
    /// stay square.
    pub round: f32,
    /// The corner of what floats: a menu, a card over the world.
    pub soft: f32,
    pub divider: f32,
    /// How far either side of a divider the pointer takes hold of
    /// it.
    pub divider_grip: f32,
    pub tab_padding: f32,
}

/// The shadow under what floats over the panels and the world, cast
/// by the lamps.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shadow {
    #[serde(with = "tint")]
    pub color: Color,
    /// How far below what floats it falls.
    pub drop: f32,
    pub blur: f32,
}

/// How long the panels take to move.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    /// A control answering the hand.
    pub interact: f32,
    /// Something opening or folding away.
    pub expand: f32,
}

/// A theme saved while the app runs has replaced the one the panels
/// were built with. Fynix lends [`Theme`] out each update, which
/// marks it changed, so this is how a change is told.
#[derive(Message, Clone, Copy, Debug)]
pub struct ThemeChanged;

/// Puts `theme` in, for the views and the window behind them.
pub fn insert_theme(world: &mut World, theme: HamletTheme) {
    world.insert_resource(ClearColor(theme.background()));
    world.insert_resource(Theme(theme));
}

/// Puts the theme in once its file has loaded, and so lets the
/// panels be built. A theme not found or not readable leaves the one
/// built into the app.
fn dress(
    file: Loading<HamletTheme>,
    mut look: ResMut<NextState<Look>>,
    mut commands: Commands,
) {
    let theme = match file.loaded() {
        Loaded::Waiting => return,
        Loaded::Read(theme) => theme,
        Loaded::Failed(why) => {
            error!(
                "assets/{}: {why}; the panels take the theme built \
                 in",
                hamlet_assets::THEME.name
            );
            HamletTheme::designed()
        }
    };
    commands.queue(|world: &mut World| insert_theme(world, theme));
    look.set(Look::Ready);
}

/// The contrast of two colours, as WCAG 2 has it.
pub fn contrast(one: Color, other: Color) -> f32 {
    let light =
        |color: Color| LinearRgba::from(color).luminance() + 0.05;
    let (a, b) = (light(one), light(other));
    a.max(b) / a.min(b)
}

impl HamletTheme {
    /// The window background, behind the panels.
    pub fn background(&self) -> Color {
        self.palette.black
    }

    /// The shadow of what floats.
    pub fn shadow(&self) -> BoxShadow {
        let shadow = &self.shadow;
        BoxShadow::new(
            shadow.color,
            px(0.0),
            px(shadow.drop),
            px(0.0),
            px(shadow.blur),
        )
    }
}

impl TextTokens for HamletTheme {
    fn tone(&self, tone: Tone) -> Color {
        let palette = &self.palette;
        match tone {
            Tone::Body => palette.brass_1,
            Tone::Dim => palette.grey_1,
            // Brass 2 marks what can be pressed.
            Tone::Faint => palette.brass_2,
            Tone::Accent => palette.lamp,
            Tone::OnAccent => palette.black,
            Tone::Critical => palette.red,
        }
    }

    fn body_size(&self) -> f32 {
        self.text.body
    }

    fn small_size(&self) -> f32 {
        self.text.small
    }
}

impl SurfaceTokens for HamletTheme {
    fn fill(&self) -> Color {
        self.palette.grey_4
    }

    fn hover(&self) -> Color {
        self.palette.wood_hover
    }

    fn panel(&self) -> Color {
        self.palette.black
    }

    fn hairline(&self) -> Color {
        self.palette.brass_6
    }

    fn selection(&self) -> Color {
        self.palette.brass_6
    }

    fn accent(&self) -> Color {
        self.palette.brass_4
    }
}

impl SpacingTokens for HamletTheme {
    fn gap(&self) -> f32 {
        self.space.gap
    }

    fn row(&self) -> f32 {
        self.space.row
    }

    fn radius(&self) -> f32 {
        self.space.round
    }

    fn divider(&self) -> f32 {
        self.space.divider
    }

    fn divider_grip(&self) -> f32 {
        self.space.divider_grip
    }

    fn tab_padding(&self) -> f32 {
        self.space.tab_padding
    }

    fn menu_radius(&self) -> f32 {
        self.space.soft
    }
}

impl MotionTokens for HamletTheme {
    fn motion(&self, motion: Motion) -> Curve {
        let seconds = match motion {
            Motion::Interact => self.motion.interact,
            Motion::Expand => self.motion.expand,
        };
        Curve {
            duration: Duration::from_secs_f32(seconds),
            ease: |t| 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tone_reads_on_every_surface_it_meets() {
        let theme = HamletTheme::designed();
        let p = &theme.palette;
        let tones = [
            Tone::Body,
            Tone::Dim,
            Tone::Faint,
            Tone::Accent,
            Tone::Critical,
        ];
        let surfaces = [
            p.black, p.well, p.card, p.plate, p.grey_4, p.hearth,
            p.red_deep, p.seam,
        ];
        for tone in tones {
            for surface in surfaces {
                let ratio = contrast(theme.tone(tone), surface);
                assert!(
                    ratio >= 4.5,
                    "{tone:?} on {surface:?}: {ratio}"
                );
            }
        }
        // Under the pointer a button is lettered in parchment, and a
        // row keeps its dim words.
        assert!(contrast(p.brass_1, p.brass_hover) >= 4.5);
        assert!(contrast(p.grey_1, p.brass_hover) >= 4.5);
        assert!(contrast(p.brass_1, theme.selection()) >= 4.5);
        // The good action is lettered on its glass, lit or not.
        assert!(contrast(p.brass_1, p.lamp_low) >= 4.5);
        assert!(contrast(p.brass_1, p.lamp_high) >= 4.5);
        assert!(contrast(p.brass_2, p.lamp_low) >= 4.5);
        assert!(contrast(p.rune, p.black) >= 4.5);
        // A line put in a file, on its own plate and on that of the
        // change.
        assert!(contrast(p.moss, p.moss_deep) >= 4.5);
        assert!(contrast(p.moss, p.plate) >= 4.5);
        // The chief's own words in the chat, and their hour.
        assert!(contrast(p.brass_1, p.brass_6) >= 4.5);
        assert!(contrast(p.brass_2, p.brass_6) >= 4.5);
    }
}
