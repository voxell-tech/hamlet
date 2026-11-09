//! The theme this window shows: the chief's pick, light or dark as
//! the chief or the computer says.

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Commands, Res};
use bevy::ecs::world::World;
use bevy::input::keyboard::KeyCode;
use bevy::ui::UiScale;
use bevy::window::WindowTheme;
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, GLOBAL, Mods, ShortcutAppExt,
};
use hamlet_desktop::color_scheme::ColorScheme;
use hamlet_ui::theme::{
    self, HamletTheme, Measures, ThemeId, Themes,
};
use serde::{Deserialize, Serialize};

/// The sizes the panels can be drawn at, as a share of their
/// designed size, with their names.
pub const ZOOMS: [(f32, &str); 6] = [
    (0.8, "80%"),
    (0.9, "90%"),
    (1.0, "100%"),
    (1.1, "110%"),
    (1.25, "125%"),
    (1.5, "150%"),
];

const fn primary(key: KeyCode) -> Chord {
    Chord {
        key,
        mods: Mods::PRIMARY,
    }
}

/// The keys that draw the panels larger: the plus key, with or
/// without the Shift it is printed under.
const ZOOM_IN: [Chord; 3] = [
    primary(KeyCode::Equal),
    Chord {
        key: KeyCode::Equal,
        mods: Mods::PRIMARY.with(Mods::SHIFT),
    },
    primary(KeyCode::NumpadAdd),
];
const ZOOM_OUT: [Chord; 2] =
    [primary(KeyCode::Minus), primary(KeyCode::NumpadSubtract)];
const ZOOM_RESET: [Chord; 2] =
    [primary(KeyCode::Digit0), primary(KeyCode::Numpad0)];

pub(crate) fn plugin(app: &mut App) {
    let command = |id, label, run| CommandSpec {
        id: CommandId(id),
        label,
        scope: GLOBAL,
        run,
        enabled: |_| true,
        repeat: true,
    };
    app.init_resource::<Appearance>()
        .init_resource::<Zoom>()
        .add_command(
            command("view.zoom_in", "Zoom In", |world, _| {
                let mut zoom = world.resource_mut::<Zoom>();
                *zoom = zoom.step(1);
            }),
            &ZOOM_IN,
        )
        .add_command(
            command("view.zoom_out", "Zoom Out", |world, _| {
                let mut zoom = world.resource_mut::<Zoom>();
                *zoom = zoom.step(-1);
            }),
            &ZOOM_OUT,
        )
        .add_command(
            command("view.zoom_reset", "Actual Size", |world, _| {
                *world.resource_mut::<Zoom>() = Zoom::default();
            }),
            &ZOOM_RESET,
        )
        .add_systems(
            Update,
            (
                show.run_if(
                    resource_changed::<Appearance>
                        .or_eager(resource_changed::<Themes>)
                        .or_eager(resource_changed::<Measures>)
                        .or_eager(resource_changed::<ColorScheme>),
                ),
                scale.run_if(resource_changed::<Zoom>),
            ),
        );
}

/// How large the panels are drawn, as a share of their designed
/// size: one of [`ZOOMS`].
#[derive(
    Resource, Clone, Copy, PartialEq, Debug, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Zoom(pub f32);

impl Default for Zoom {
    fn default() -> Self {
        Self(1.0)
    }
}

impl Zoom {
    /// The place in [`ZOOMS`] nearest this.
    pub fn at(self) -> usize {
        ZOOMS
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (a.0 - self.0).abs().total_cmp(&(b.0 - self.0).abs())
            })
            .map_or(0, |(at, _)| at)
    }

    /// `by` places along [`ZOOMS`], held at its ends.
    pub fn step(self, by: isize) -> Self {
        let at =
            self.at().saturating_add_signed(by).min(ZOOMS.len() - 1);
        Self(ZOOMS[at].0)
    }
}

/// Draws the panels at the zoom.
fn scale(zoom: Res<Zoom>, mut commands: Commands) {
    commands.insert_resource(UiScale(zoom.0));
}

/// The chief's choice, none until the chief picks. Kept in view.toml
/// only once picked, so a window that never picked follows whatever
/// default a later build ships.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub struct Appearance(pub Option<Pick>);

/// A family, and whether it is shown light or dark.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub struct Pick {
    pub theme: ThemeId,
    pub mode: Mode,
}

#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// As the computer is.
    #[default]
    System,
    Light,
    Dark,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::System => "System",
            Mode::Light => "Light",
            Mode::Dark => "Dark",
        }
    }
}

impl Appearance {
    /// The pick, or the default: Hamlet, following the computer.
    pub fn pick(&self) -> Pick {
        self.0.clone().unwrap_or_default()
    }

    /// Takes `theme`, in the mode picked before.
    pub fn pick_theme(&mut self, theme: ThemeId) {
        self.0 = Some(Pick {
            theme,
            ..self.pick()
        });
    }

    /// Takes `mode`, for the theme picked before.
    pub fn pick_mode(&mut self, mode: Mode) {
        self.0 = Some(Pick {
            mode,
            ..self.pick()
        });
    }

    /// Light or dark: the mode's, or the computer's, dark while it is
    /// silent. None while the computer's answer is awaited.
    pub fn light_or_dark(
        &self,
        system: ColorScheme,
    ) -> Option<WindowTheme> {
        match (self.pick().mode, system) {
            (Mode::Light, _) => Some(WindowTheme::Light),
            (Mode::Dark, _) | (Mode::System, ColorScheme::Silent) => {
                Some(WindowTheme::Dark)
            }
            (Mode::System, ColorScheme::Says(said)) => Some(said),
            (Mode::System, ColorScheme::Asking) => None,
        }
    }

    /// The theme to show; none while the computer's answer is
    /// awaited.
    pub fn resolve(
        &self,
        themes: &Themes,
        measures: &Measures,
        system: ColorScheme,
    ) -> Option<HamletTheme> {
        let light_or_dark = self.light_or_dark(system)?;
        let variant = themes
            .get(&self.pick().theme)
            .variants
            .get(light_or_dark);
        Some(HamletTheme::new(variant.palette.clone(), measures))
    }
}

/// The one path a theme takes to the panels.
fn show(
    appearance: Res<Appearance>,
    themes: Res<Themes>,
    measures: Res<Measures>,
    system: Res<ColorScheme>,
    mut commands: Commands,
) {
    if let Some(theme) =
        appearance.resolve(&themes, &measures, *system)
    {
        commands.queue(move |world: &mut World| {
            theme::show(world, theme);
        });
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::{Assets, Handle};
    use bevy::image::Image;
    use bevy::state::app::StatesPlugin;
    use bevy::time::TimePlugin;
    use bevy_fynix::Theme;
    use hamlet_ui::theme::Variants;

    use super::*;
    use crate::cursor::{self, Cursors};

    #[test]
    fn a_picked_light_appearance_repaints_the_panels_and_the_cursors()
    {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            StatesPlugin,
            theme::plugin,
            plugin,
            cursor::plugin,
        ))
        .init_resource::<Assets<Image>>()
        .insert_resource(ColorScheme::Silent);
        let pointer = |app: &App| -> Handle<Image> {
            app.world()
                .resource::<Cursors>()
                .at(cursor::Mode::Point, 0.0, false)
                .unwrap()
                .clone()
        };
        let themes = Themes::shipped();
        let Variants::Both { light, dark } =
            &themes.get(&ThemeId::default()).variants
        else {
            panic!("Hamlet has a light and a dark variant");
        };
        let surface = |app: &App| {
            app.world()
                .resource::<Theme<HamletTheme>>()
                .0
                .palette
                .surface
        };
        app.update();
        app.update();
        assert_eq!(surface(&app), dark.palette.surface);
        let lamplit = pointer(&app);

        app.world_mut().resource_mut::<Appearance>().0 = Some(Pick {
            theme: ThemeId::default(),
            mode: Mode::Light,
        });
        app.update();
        app.update();

        assert_eq!(surface(&app), light.palette.surface);
        assert_ne!(pointer(&app), lamplit);
    }
}
