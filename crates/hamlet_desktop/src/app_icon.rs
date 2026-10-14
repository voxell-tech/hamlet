//! The app's icon when it is run outside a bundle: on the Dock of a
//! Mac, and on the window and the taskbar elsewhere.

use bevy::app::{App, PostStartup};
use bevy::ecs::system::NonSendMarker;

/// The logo on its rounded tile, 256 pixels a side.
const ICON: &[u8] = include_bytes!("icon.png");

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(PostStartup, show);
}

/// Puts the logo on the Dock. A bundle's own icon is the same
/// picture, so this changes nothing for a packaged app.
#[cfg(target_os = "macos")]
pub fn show(_main: NonSendMarker) {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData};

    let Some(thread) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(ICON);
    let image: Option<Retained<NSImage>> =
        NSImage::initWithData(thread.alloc(), &data);
    if let Some(image) = image {
        let app = NSApplication::sharedApplication(thread);
        // SAFETY: called on the main thread with an image that
        // lives as long as the app keeps it.
        unsafe { app.setApplicationIconImage(Some(&image)) };
    }
}

/// Puts the logo on the window, which the taskbar shows too.
#[cfg(not(target_os = "macos"))]
pub fn show(
    _main: NonSendMarker,
    windows: bevy::ecs::system::Query<
        bevy::ecs::entity::Entity,
        bevy::ecs::query::With<bevy::window::PrimaryWindow>,
    >,
) {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{
        CompressedImageFormats, Image, ImageSampler, ImageType,
    };

    let Ok(image) = Image::from_buffer(
        ICON,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::MAIN_WORLD,
    ) else {
        return;
    };
    let (wide, tall) = (image.width(), image.height());
    let Some(pixels) = image.data else {
        return;
    };
    let Ok(icon) = winit::window::Icon::from_rgba(pixels, wide, tall)
    else {
        return;
    };
    bevy::winit::WINIT_WINDOWS.with_borrow(|native| {
        for window in &windows {
            if let Some(native) = native.get_window(window) {
                native.set_window_icon(Some(icon.clone()));
            }
        }
    });
}
