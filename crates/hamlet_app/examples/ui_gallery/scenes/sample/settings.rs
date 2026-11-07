//! The settings, charter, workspace, sight and appearance gallery
//! scenes.

use bevy::ecs::world::World;
use chrono::Timelike;
use hamlet_app::settings;
use hamlet_assets::Tuning;
use hamlet_client::realm::Settings;
use hamlet_desktop::color_scheme::ColorScheme;
use hamlet_protocol::client as protocol;
use hamlet_realm::sky::{
    DistanceFogSettings, SkySettings, VolumetricFogSettings,
};
use hamlet_realm::{North, Sky};
use hamlet_ui::theme::{
    self, HamletTheme, Measures, ThemeId, Themes,
};
use hamlet_vfx::wind::WindChoice;

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    if !matches!(
        scene,
        Scene::Settings
            | Scene::SettingsModels
            | Scene::Charter
            | Scene::Workspace
            | Scene::Sight
            | Scene::Appearance(_)
    ) {
        return;
    }
    world.resource_mut::<Settings>().0.endpoints.push(
        protocol::SettingsViewEndpoints {
            name: "far".into(),
            base_url: "https://models.example/api/v1".into(),
            key: true,
        },
    );
    world.init_resource::<hamlet_desktop::awake::KeepAwake>();
    world.init_resource::<hamlet_realm::sky::Quality>();
    if scene == Scene::Charter {
        world.insert_resource(settings::Charter {
            text: include_str!("../../../../../../assets/charter.md")
                .into(),
            own: true,
        });
        world.insert_resource(settings::Pane::Charter);
    }
    if scene == Scene::SettingsModels {
        world.resource_mut::<hamlet_app::models::Shelf>().models =
            super::hub::catalogue();
        world.insert_resource(settings::Pane::Models);
    }
    if scene == Scene::Workspace {
        world.resource_mut::<Settings>().0.workspace =
            protocol::SettingsViewWorkspace {
                url: Some("https://workspace.example".into()),
                running: None,
                moving: None,
            };
        world.insert_resource(settings::Pane::Workspace);
    }
    if scene == Scene::Sight {
        world.init_resource::<SkySettings>();
        world.init_resource::<VolumetricFogSettings>();
        world.init_resource::<DistanceFogSettings>();
        world.init_resource::<WindChoice>();
        // Settled: the sky at the clock's hour and north where it
        // is set, as they are but for a moment after a change.
        let now = chrono::Local::now();
        world.insert_resource(Sky {
            hour: now.hour() as f32 + now.minute() as f32 / 60.0,
            ..Sky::default()
        });
        world.insert_resource(North::default());
        world.insert_resource(settings::Pane::Sight);
    }
    if let Scene::Appearance(shade) = scene {
        // The computer's, which the panels follow by default.
        world.insert_resource(ColorScheme::Says(shade));
        let themes = world.resource::<Themes>();
        let palette = themes
            .get(&ThemeId::default())
            .variants
            .get(shade)
            .palette
            .clone();
        theme::show(
            world,
            HamletTheme::new(palette, &Measures::designed()),
        );
        world.insert_resource(settings::Pane::Appearance);
    }
}
