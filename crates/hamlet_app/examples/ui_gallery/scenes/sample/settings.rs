//! The settings, charter, workspace and sight gallery scenes.

use bevy::ecs::world::World;
use hamlet_client::realm::Settings;
use hamlet_protocol::client as protocol;
use hamlet_ui::settings;

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    if !matches!(
        scene,
        Scene::Settings
            | Scene::Charter
            | Scene::Workspace
            | Scene::Sight
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
        world.init_resource::<hamlet_realm::sky::SkySettings>();
        world.init_resource::<hamlet_realm::sky::FogSettings>();
        world.init_resource::<hamlet_realm::Sky>();
        world.insert_resource(hamlet_realm::North(30.0));
        world.insert_resource(settings::Pane::Sight);
    }
}
