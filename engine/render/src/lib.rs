//! What plugins draw. A scene is kept entity by entity as its owner
//! changes it, its cameras shown in a panel's viewport or in the
//! window behind the dock, and what the person does over them told
//! back to the owner. An owner may draw with WGSL of its own, checked
//! before use, and run compute over its buffers, held to its GPU
//! time. The owner is an entity; this crate knows nothing of how it
//! runs.

mod change;
mod compute;
mod guard;
mod label;
mod material;
mod pass;
mod pick;
mod scene;
mod shader;
mod viewport;

use bevy::app::App;
use bevy::asset::{AssetPlugin, Assets};
use bevy::image::Image;
use bevy::mesh::Mesh;
use bevy::pbr::StandardMaterial;
use bevy::render::storage::ShaderBuffer;
use bevy::shader::Shader;

pub use self::change::{CameraSetup, SceneChange, SceneLight};
pub use self::guard::{GpuInfo, GpuLost, Throttled};
pub use self::material::ShadedMaterial;
pub use self::pass::ScenePass;
pub use self::pick::{Hit, PointerButton, SceneEvent, SceneInput};
pub use self::scene::{
    Scene, SceneCamera, SceneOf, ScenePart, SceneWorldExt, Scenes,
};
pub use self::shader::{
    CheckedShader, PARAMS, ShaderKind, ShaderRules, check,
};
pub use self::viewport::{RenderTuning, Viewport};

pub fn plugin(app: &mut App) {
    // Without Bevy's assets, as in the harness, a scene is kept all
    // the same and drawn nowhere.
    if !app.is_plugin_added::<AssetPlugin>() {
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<Shader>>()
            .init_resource::<Assets<ShaderBuffer>>();
    }
    app.add_message::<SceneInput>().add_plugins((
        scene::plugin,
        viewport::plugin,
        label::plugin,
        material::plugin,
        compute::plugin,
        pass::plugin,
        guard::plugin,
    ));
}
