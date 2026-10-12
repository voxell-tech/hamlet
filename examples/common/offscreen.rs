//! An app that draws into an image with no window, and the stills
//! saved from it.

use std::path::PathBuf;
use std::time::Duration;

use bevy::app::{PluginGroupBuilder, ScheduleRunnerPlugin};
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::{ExitCondition, WindowResolution};
use bevy::winit::WinitPlugin;

/// Bevy's plugins for a view of `size` that is never shown, with
/// every pipeline ready before its first frame.
pub fn plugins(size: UVec2) -> PluginGroupBuilder {
    DefaultPlugins
        // A window entity with no winit behind it.
        .set(WindowPlugin {
            primary_window: Some(Window {
                resolution: WindowResolution::new(size.x, size.y)
                    .with_scale_factor_override(1.0),
                ..default()
            }),
            exit_condition: ExitCondition::DontExit,
            ..default()
        })
        .set(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        })
        .disable::<WinitPlugin>()
        .add(ScheduleRunnerPlugin::run_loop(Duration::ZERO))
}

/// The image the app draws into, and its pixels to one of the
/// interface's.
#[derive(Resource)]
pub struct Target(Handle<Image>, f32);

impl Target {
    /// Gives `app` an image of `pixels` to draw into.
    pub fn insert(app: &mut App, pixels: UVec2, scale: f32) {
        let image = Image::new_target_texture(
            pixels.x,
            pixels.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        );
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(image);
        app.insert_resource(Target(image, scale));
    }

    pub fn drawn(&self) -> RenderTarget {
        RenderTarget::Image(ImageRenderTarget {
            handle: self.0.clone(),
            scale_factor: self.1,
        })
    }

    /// Saves what is drawn this frame to `path`.
    pub fn shoot(&self, commands: &mut Commands, path: PathBuf) {
        commands
            .spawn(Screenshot(self.drawn()))
            .observe(save_to_disk(path));
    }
}

/// The argument after `name` on the command line.
pub fn word(name: &str) -> Option<String> {
    let args = std::env::args().collect::<Vec<_>>();
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).cloned()
}
