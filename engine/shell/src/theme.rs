//! The engine's theme, from `engine/assets/theme.toml`: warm
//! near-black surfaces, brass, and lamplight for what is live. Dark
//! only.

use std::time::Duration;

use bevy::app::{App, PreUpdate};
use bevy::color::{Color, Srgba};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::message::{Message, MessageWriter};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Commands, Res};
use bevy::ecs::world::World;
use bevy::prelude::ClearColor;
use bevy_fynix::Theme;
use bevy_fynix::tokens::{
    Curve, Motion, MotionTokens, SpacingTokens, SurfaceTokens,
    TextTokens, Tone,
};
use hamlet_engine_core::TuningPlugin;
use hamlet_engine_core::tuning::{THEME, Tuning, TuningFile};
use serde::{Deserialize, Deserializer};

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<EngineTheme>::default())
        .add_message::<ThemeChanged>()
        .add_systems(
            PreUpdate,
            restyle.run_if(resource_changed::<EngineTheme>),
        );
    let theme = app.world().resource::<EngineTheme>().clone();
    dress(app.world_mut(), theme);
}

/// The theme, as `engine/assets/theme.toml` has it.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineTheme {
    pub colors: Colors,
    pub text: TextSizes,
    pub space: Spacing,
    pub motion: Timing,
}

impl Tuning for EngineTheme {
    const FILE: TuningFile = THEME;
    const SECTION: Option<&'static str> = None;
}

/// Every colour of the shell.
#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Colors {
    /// The window behind everything, and a panel.
    #[serde(deserialize_with = "hex")]
    pub background: Color,
    /// The menu bar and the status line.
    #[serde(deserialize_with = "hex")]
    pub bar: Color,
    /// The line between a bar and the dock.
    #[serde(deserialize_with = "hex")]
    pub seam: Color,
    /// A control at rest, and under the pointer.
    #[serde(deserialize_with = "hex")]
    pub fill: Color,
    #[serde(deserialize_with = "hex")]
    pub hover: Color,
    /// Borders, dividers and what is selected.
    #[serde(deserialize_with = "hex")]
    pub hairline: Color,
    /// A solid accent surface, as the workspace shown.
    #[serde(deserialize_with = "hex")]
    pub accent: Color,
    /// What is read.
    #[serde(deserialize_with = "hex")]
    pub text: Color,
    /// Text that stands back.
    #[serde(deserialize_with = "hex")]
    pub dim: Color,
    /// What can be pressed.
    #[serde(deserialize_with = "hex")]
    pub faint: Color,
    /// What is live, and every glow.
    #[serde(deserialize_with = "hex")]
    pub lit: Color,
    /// What is refused, stopped or at fault.
    #[serde(deserialize_with = "hex")]
    pub critical: Color,
}

/// The sizes of the lettering.
#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSizes {
    pub body: f32,
    pub small: f32,
    /// A menu's title and a workspace's name in the bar.
    pub title: f32,
}

/// The gaps, heights and corners of the shell.
#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spacing {
    pub gap: f32,
    /// The height of a row.
    pub row: f32,
    /// The corner of a control.
    pub round: f32,
    /// The corner of what floats, as a menu.
    pub soft: f32,
    pub divider: f32,
    /// How far either side of a divider the pointer takes hold of
    /// it.
    pub divider_grip: f32,
    pub tab_padding: f32,
    /// The height of the menu bar.
    pub bar: f32,
    /// The height of the status line.
    pub status: f32,
    /// The space inside a panel, around its content.
    pub panel: f32,
}

/// How long the shell takes to move.
#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    /// A control answering the hand.
    pub interact: f32,
    /// Something opening or folding away.
    pub expand: f32,
}

/// A saved theme replaced the one the views were built with, so they
/// are built again.
#[derive(Message, Clone, Copy, Debug)]
pub struct ThemeChanged;

/// A colour written `#rrggbbaa`.
fn hex<'de, D: Deserializer<'de>>(
    from: D,
) -> Result<Color, D::Error> {
    let text = String::deserialize(from)?;
    let digits = text
        .strip_prefix('#')
        .filter(|digits| digits.len() == 8)
        .ok_or_else(|| {
            serde::de::Error::custom(format!(
                "{text:?} is not #rrggbbaa"
            ))
        })?;
    Srgba::hex(digits).map(Color::from).map_err(|e| {
        serde::de::Error::custom(format!("{text:?}: {e}"))
    })
}

/// Puts `theme` in, for the views and the window behind them.
fn dress(world: &mut World, theme: EngineTheme) {
    world.insert_resource(ClearColor(theme.colors.background));
    world.insert_resource(Theme(theme));
}

fn restyle(
    theme: Res<EngineTheme>,
    mut changed: MessageWriter<ThemeChanged>,
    mut commands: Commands,
) {
    // The theme was put in as the app was built.
    if theme.is_added() {
        return;
    }
    let theme = theme.clone();
    commands.queue(|world: &mut World| dress(world, theme));
    changed.write(ThemeChanged);
}

impl TextTokens for EngineTheme {
    fn tone(&self, tone: Tone) -> Color {
        let colors = &self.colors;
        match tone {
            Tone::Body => colors.text,
            Tone::Dim => colors.dim,
            Tone::Faint => colors.faint,
            Tone::Accent => colors.lit,
            Tone::OnAccent => colors.background,
            Tone::Critical => colors.critical,
        }
    }

    fn body_size(&self) -> f32 {
        self.text.body
    }

    fn small_size(&self) -> f32 {
        self.text.small
    }
}

impl SurfaceTokens for EngineTheme {
    fn fill(&self) -> Color {
        self.colors.fill
    }

    fn hover(&self) -> Color {
        self.colors.hover
    }

    fn panel(&self) -> Color {
        self.colors.background
    }

    fn hairline(&self) -> Color {
        self.colors.hairline
    }

    fn selection(&self) -> Color {
        self.colors.hairline
    }

    fn accent(&self) -> Color {
        self.colors.accent
    }
}

impl SpacingTokens for EngineTheme {
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

impl MotionTokens for EngineTheme {
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
