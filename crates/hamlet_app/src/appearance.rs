//! The theme this window shows: the chief's pick, light or dark as
//! the chief or the computer says.

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Commands, Res};
use bevy::ecs::world::World;
use bevy::window::WindowTheme;
use hamlet_desktop::color_scheme::ColorScheme;
use hamlet_ui::theme::{
    self, HamletTheme, Measures, ThemeId, Themes,
};
use serde::{Deserialize, Serialize};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Appearance>().add_systems(
        Update,
        show.run_if(
            resource_changed::<Appearance>
                .or_eager(resource_changed::<Themes>)
                .or_eager(resource_changed::<Measures>)
                .or_eager(resource_changed::<ColorScheme>),
        ),
    );
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
    use bevy::picking::pointer::PointerInput;
    use bevy::state::app::StatesPlugin;
    use bevy::time::TimePlugin;
    use bevy::window::WindowFocused;
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
        .add_message::<WindowFocused>()
        .add_message::<PointerInput>()
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
