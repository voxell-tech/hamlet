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
use crate::feel::FeelSettings;

/// Loads `assets/ui.toml` and keeps the panels in step with it.
pub fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<UiTuning>::default());
}

/// The panels' tuning, as `assets/ui.toml` has it.
#[derive(Asset, TypePath, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiTuning {
    pub feel: FeelSettings,
    pub chrome: ChromeSettings,
    pub desk: DeskSettings,
    pub chat: ChatSettings,
}

impl Tuning for UiTuning {
    const FILE: File = hamlet_assets::UI;

    fn apply(&self, was: &Self, world: &mut World) {
        follow(world, &was.feel, &self.feel);
        follow(world, &was.chrome, &self.chrome);
        follow(world, &was.desk, &self.desk);
        follow(world, &was.chat, &self.chat);
    }
}

designed!(UiTuning {
    FeelSettings => feel,
    ChromeSettings => chrome,
    DeskSettings => desk,
    ChatSettings => chat,
});
