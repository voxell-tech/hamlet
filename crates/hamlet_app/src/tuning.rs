//! The sections of `assets/ui.toml`, each a settings struct kept in
//! the module it tunes.

use bevy::app::App;
use bevy::asset::Asset;
use bevy::ecs::world::World;
use bevy::reflect::TypePath;
use hamlet_assets::{File, Tuning, TuningPlugin, designed, follow};

use crate::chat::ChatSettings;
use crate::chrome::ChromeSettings;
use crate::desk::DeskSettings;

/// Loads `assets/ui.toml` and `assets/chat.toml` and keeps the
/// panels in step with them.
pub fn plugin(app: &mut App) {
    app.add_plugins((
        TuningPlugin::<UiTuning>::default(),
        TuningPlugin::<ChatSettings>::default(),
    ));
}

/// The panels' tuning, as `assets/ui.toml` has it.
#[derive(Asset, TypePath, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiTuning {
    pub chrome: ChromeSettings,
    pub desk: DeskSettings,
}

impl Tuning for UiTuning {
    const FILE: File = hamlet_assets::UI;

    fn apply(&self, was: &Self, world: &mut World) {
        follow(world, &was.chrome, &self.chrome);
        follow(world, &was.desk, &self.desk);
    }
}

designed!(UiTuning {
    ChromeSettings => chrome,
    DeskSettings => desk,
});
