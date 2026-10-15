//! The settings and charter gallery scenes.

use bevy::ecs::world::World;
use hamlet_client::realm::Settings;
use hamlet_protocol::v0;
use hamlet_ui::settings;

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    if !matches!(scene, Scene::Settings | Scene::Charter) {
        return;
    }
    world.resource_mut::<Settings>().0.endpoints.push(
        v0::SettingsViewEndpoints {
            name: "far".into(),
            base_url: "https://models.example/api/v1".into(),
            key: true,
        },
    );
    world.init_resource::<hamlet_desktop::awake::KeepAwake>();
    world.init_resource::<hamlet_world::sky::Quality>();
    if scene == Scene::Charter {
        world.insert_resource(settings::Charter {
            text: include_str!("../../../../assets/charter.md")
                .into(),
            own: true,
        });
        world.insert_resource(settings::Pane::Charter);
    }
}
