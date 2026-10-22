//! A plugin's scenes as the harness reads them, with no GPU: what
//! they hold, and the person clicking a watched entity.

use bevy::math::Vec3;
use hamlet_engine_plugins::PluginInfo;
use hamlet_engine_render::{
    Hit, SceneEvent, SceneInput, SceneWorldExt,
};

use crate::Harness;

/// What the scene of a plugin holds.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct SceneCounts {
    /// Entities, each copy of instances counted.
    pub entities: usize,
    pub meshes: usize,
    pub materials: usize,
    pub cameras: usize,
    /// Entities that lay a decal.
    pub decals: usize,
}

impl Harness {
    /// What the scene of the plugin `plugin` holds, once it has one.
    pub fn scene(&mut self, plugin: &str) -> Option<SceneCounts> {
        let world = self.world();
        let owner = world
            .query::<(bevy::ecs::entity::Entity, &PluginInfo)>()
            .iter(world)
            .find(|(_, info)| info.id.as_str() == plugin)
            .map(|(owner, _)| owner)?;
        let scene = world.scene_of(owner)?;
        Some(SceneCounts {
            entities: scene.entities(),
            meshes: scene.meshes(),
            materials: scene.materials(),
            cameras: scene.cameras(),
            decals: scene.decals(),
        })
    }

    /// Clicks the entity of the plugin `plugin` it named `key`, the
    /// copy `index` of instances, as the pointer would.
    pub fn pick(
        &mut self,
        plugin: &str,
        key: u32,
        index: u32,
    ) -> Result<(), String> {
        let world = self.world();
        let owner = world
            .query::<(bevy::ecs::entity::Entity, &PluginInfo)>()
            .iter(world)
            .find(|(_, info)| info.id.as_str() == plugin)
            .map(|(owner, _)| owner)
            .ok_or_else(|| format!("there is no plugin {plugin}"))?;
        world.write_message(SceneInput {
            owner,
            event: SceneEvent::Click(Hit {
                key,
                index,
                point: Vec3::ZERO,
                normal: Vec3::Y,
            }),
        });
        Ok(())
    }
}
