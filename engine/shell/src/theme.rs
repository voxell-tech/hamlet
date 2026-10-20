//! The engine's theme, from `engine/tuning/theme.toml`: the colours
//! by role and the values every style is built from. Dark only.

use std::time::Duration;

use bevy::prelude::*;
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
    apply_theme(app.world_mut(), theme);
}

/// The theme, as `engine/tuning/theme.toml` has it.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineTheme {
    pub colors: Colors,
    pub text: TextSizes,
    pub space: Spacing,
    pub shadow: Shadow,
    pub glow: Glow,
    pub motion: Timing,
}

impl Tuning for EngineTheme {
    const FILE: TuningFile = THEME;
    const SECTION: Option<&'static str> = None;
}

/// Every colour of the shell, by role.
#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Colors {
    /// The window, a panel and the bars.
    pub background: ThemeColor,
    /// What the background shades to at the bottom of a panel or a
    /// bar.
    pub background_bottom: ThemeColor,
    /// A card in a list, a shade above the background.
    pub surface: ThemeColor,
    /// A background set on a panel, such as one under code.
    pub surface_raised: ThemeColor,
    /// The floor of a field, and a button held down.
    pub surface_sunken: ThemeColor,
    /// A plain button at rest, and a row under the pointer.
    pub fill: ThemeColor,
    /// A control of the toolkit under the pointer.
    pub hover: ThemeColor,
    /// A plain button, a frame or a row pressed whole under the
    /// pointer.
    pub accent_hover: ThemeColor,
    /// The line between a bar and the dock.
    pub separator: ThemeColor,
    /// Borders of cards, fields and dock areas, and dividers.
    pub border: ThemeColor,
    /// The frame of a button at rest, and of what floats.
    pub border_strong: ThemeColor,
    /// What is read.
    pub text: ThemeColor,
    /// Text that stands back.
    pub text_muted: ThemeColor,
    /// The text of what can be pressed.
    pub text_accent: ThemeColor,
    /// The frame of the primary button and of a field typed in, and
    /// a solid accent surface.
    pub accent: ThemeColor,
    /// The frame of a button under the pointer, and the line under
    /// the tab shown.
    pub accent_soft: ThemeColor,
    /// What is live or waits, and every glow.
    pub active: ThemeColor,
    /// The core of a glow.
    pub active_strong: ThemeColor,
    /// The background of the primary button, and the same lit.
    pub active_low: ThemeColor,
    pub active_high: ThemeColor,
    /// Words selected, lit behind them.
    pub selection: ThemeColor,
    /// What is told, apart from what is live.
    pub info: ThemeColor,
    /// What is refused, stopped or at fault.
    pub critical: ThemeColor,
    /// The glow of a critical button, and its core.
    pub critical_glow: ThemeColor,
    pub critical_strong: ThemeColor,
    /// The frame of a critical button at rest.
    pub critical_muted: ThemeColor,
    /// The background of a critical button under the pointer, and at
    /// rest.
    pub critical_hover: ThemeColor,
    pub critical_surface: ThemeColor,
    /// What was added or went well, and the background under it.
    pub success: ThemeColor,
    pub success_surface: ThemeColor,
    /// The shadow under what floats.
    pub shadow: ThemeColor,
}

/// A [`Color`] of the theme, written `#rrggbbaa` in its file.
#[derive(Deref, DerefMut, Clone, Copy, PartialEq, Debug)]
pub struct ThemeColor(pub Color);

impl<'de> Deserialize<'de> for ThemeColor {
    fn deserialize<D: Deserializer<'de>>(
        from: D,
    ) -> Result<Self, D::Error> {
        let text = String::deserialize(from)?;
        let digits = text
            .strip_prefix('#')
            .filter(|digits| digits.len() == 8)
            .ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "{text:?} is not #rrggbbaa"
                ))
            })?;
        Srgba::hex(digits).map(|srgba| Self(srgba.into())).map_err(
            |e| serde::de::Error::custom(format!("{text:?}: {e}")),
        )
    }
}

/// The sizes of the text.
#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSizes {
    pub body: f32,
    /// Notes, buttons, tabs and the status line.
    pub small: f32,
    /// A menu's title and a card's.
    pub heading: f32,
    /// The name at the head of a page.
    pub title: f32,
    /// The line over a card that waits for the person.
    pub caption: f32,
    /// The space between the letters of the display face.
    pub tracking: f32,
}

/// The gaps, heights, corners and lines of the shell.
#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spacing {
    pub gap: f32,
    /// The height of a row and the least height of a button.
    pub row: f32,
    /// The corner of a control and of a card.
    pub radius: f32,
    /// The corner of what floats, as a menu.
    pub menu_radius: f32,
    /// The width of a control's frame and a card's.
    pub border: f32,
    pub divider: f32,
    /// How far either side of a divider the pointer takes hold of
    /// it.
    pub divider_grip: f32,
    /// The space at each side of a dock tab's name.
    pub tab_padding: f32,
    /// The side of an icon beside text, the space between, and how
    /// far it is set above the text's middle.
    pub icon: f32,
    pub icon_gap: f32,
    pub icon_offset: f32,
    /// The space at each side of a button's text.
    pub button_padding: f32,
    /// The space before the icon of a button that has one, and after
    /// its text.
    pub mark_start: f32,
    pub mark_end: f32,
    /// The space inside a panel, around its content.
    pub panel: f32,
    /// The space between the parts of a panel.
    pub section: f32,
    /// The space inside a card, across and down.
    pub card_padding_x: f32,
    pub card_padding_y: f32,
    /// The space down inside a row of a list.
    pub row_padding: f32,
    /// The height of the menu bar, and the space at its ends.
    pub bar: f32,
    pub bar_padding: f32,
    /// The space at each side of a menu's title.
    pub title_padding: f32,
    /// The least width of an open menu, and the space between an
    /// entry's words and its keys.
    pub menu_width: f32,
    pub menu_gap: f32,
    /// The space across and down inside the status line, between its
    /// parts, the side of its icons and the space after each.
    pub status_padding_x: f32,
    pub status_padding_y: f32,
    pub status_gap: f32,
    pub status_icon: f32,
    pub status_icon_gap: f32,
    /// The widest a page of Settings is drawn.
    pub page_width: f32,
    /// The least height of a tab within a panel, and the line under
    /// the one shown.
    pub tab_height: f32,
    pub underline: f32,
    /// The share of a panel's height its background keeps before it
    /// shades to `background_bottom`.
    pub bottom_start: f32,
}

/// The shadow under what floats.
#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shadow {
    /// How far below what floats it falls.
    pub drop: f32,
    pub blur: f32,
}

/// How a control glows and gives under the hand.
#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Glow {
    /// The rates at which a glow follows the pointer, a press
    /// follows the button and a pulse or a shake dies away, each per
    /// second.
    pub hover_rate: f32,
    pub press_rate: f32,
    pub fade_rate: f32,
    /// The share of its size a button loses while held and gains at
    /// the top of a pulse, and how far down it moves while held.
    pub press_shrink: f32,
    pub pulse_grow: f32,
    pub press_offset: f32,
    /// The reach of a critical button's shake, and its rate.
    pub shake: f32,
    pub shake_rate: f32,
    /// The glow, 0 to 1, of a plain button, a critical one and the
    /// primary one under the pointer, of a field typed in, and of
    /// the primary button at rest.
    pub plain: f32,
    pub critical: f32,
    pub primary: f32,
    pub field: f32,
    pub primary_rest: f32,
    /// The share of its glow a held control loses under the hand,
    /// and the share of the rest it gains as its work is done.
    pub press_dim: f32,
    pub done_gain: f32,
    /// The share of its glow a glow flickers by.
    pub flicker: f32,
    /// The share of a control's glow that shows around it, the share
    /// of that a critical button or a field shows, and how far it
    /// reaches, at rest and more at the top of a pulse.
    pub spread: f32,
    pub spread_muted: f32,
    pub reach: f32,
    pub pulse_reach: f32,
    /// The share of the reach of the glow's core.
    pub core: f32,
    /// The share of the reach of the glow about text with no
    /// background.
    pub text_reach: f32,
    /// How far below a control its glow sits, as a share of its
    /// reach.
    pub drop: f32,
    /// The opacity at the bottom of the gradient lighting the inside
    /// of the primary action and a field being typed in, at full
    /// glow.
    pub inner: f32,
    /// The opacity at the bottom of the gradient that lights a title
    /// or a tab under the pointer, and the opacity a press adds.
    pub bottom: f32,
    pub bottom_press: f32,
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

impl EngineTheme {
    /// The shadow under what floats.
    pub fn drop_shadow(&self) -> BoxShadow {
        BoxShadow::new(
            *self.colors.shadow,
            px(0.0),
            px(self.shadow.drop),
            px(0.0),
            px(self.shadow.blur),
        )
    }
}

/// Puts `theme` in, for the views and the window behind them.
fn apply_theme(world: &mut World, theme: EngineTheme) {
    world.insert_resource(ClearColor(*theme.colors.background));
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
    commands.queue(|world: &mut World| apply_theme(world, theme));
    changed.write(ThemeChanged);
}

impl TextTokens for EngineTheme {
    fn tone(&self, tone: Tone) -> Color {
        let colors = &self.colors;
        *match tone {
            Tone::Body => colors.text,
            Tone::Dim => colors.text_muted,
            Tone::Faint => colors.text_accent,
            Tone::Accent => colors.active,
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
        *self.colors.fill
    }

    fn hover(&self) -> Color {
        *self.colors.hover
    }

    fn panel(&self) -> Color {
        *self.colors.background
    }

    fn hairline(&self) -> Color {
        *self.colors.border
    }

    // Text selected in a field, and where a dragged tab would land.
    fn selection(&self) -> Color {
        *self.colors.border
    }

    fn accent(&self) -> Color {
        *self.colors.accent
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
        self.space.radius
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
        self.space.menu_radius
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_file_holds_every_role_and_refuses_one_unknown_or_missing()
     {
        let text = THEME.text;
        let theme = toml::from_str::<EngineTheme>(text);
        assert!(theme.is_ok(), "{theme:?}");

        let unknown = text.replace(
            "[colors]\n",
            "[colors]\nbrass = \"#000000ff\"\n",
        );
        let why = toml::from_str::<EngineTheme>(&unknown)
            .expect_err("an unknown role")
            .to_string();
        assert!(why.contains("brass"), "{why}");

        let missing = text
            .lines()
            .filter(|line| !line.starts_with("text_accent "))
            .collect::<Vec<_>>()
            .join("\n");
        let why = toml::from_str::<EngineTheme>(&missing)
            .expect_err("a missing role")
            .to_string();
        assert!(why.contains("text_accent"), "{why}");
    }
}
