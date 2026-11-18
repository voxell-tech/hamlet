use bevy::app::App;
use bevy::asset::{AssetApp, AssetPlugin};
use bevy::camera::{
    Camera, Camera2d, ComputedCameraValues, RenderTargetInfo,
};
use bevy::image::{Image, TextureAtlasLayout};
use bevy::math::UVec2;
use bevy::text::TextPlugin;
use bevy::ui::UiPlugin;

use crate::widgets::platform::install_fonts;

/// An app that lays out UI text in Hamlet's fonts, on a square window
/// `size` physical pixels wide at `scale`.
pub(crate) fn ui_app(size: u32, scale: f32) -> App {
    let mut app = App::new();
    app.add_plugins((
        AssetPlugin::default(),
        bevy::time::TimePlugin,
        bevy::transform::TransformPlugin,
        bevy::input::InputPlugin,
        bevy::picking::PickingPlugin,
        bevy::picking::InteractionPlugin,
        TextPlugin,
        UiPlugin,
    ))
    .init_asset::<Image>()
    .init_asset::<TextureAtlasLayout>();
    install_fonts(app.world_mut());
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::splat(size),
                    scale_factor: scale,
                }),
                ..Default::default()
            },
            ..Default::default()
        },
    ));
    app
}
