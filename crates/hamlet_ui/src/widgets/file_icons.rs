//! The icons of files and folders by their names, from Catppuccin's
//! VS Code icons (`file_icons/LICENSE`), in their own colours. They
//! are a family apart from the panels' icons, for the Files panel's
//! tree alone. `file_icons/icons.json` holds them and which name
//! takes which; `file_icons/import.mjs` makes it from a checkout of
//! Catppuccin's repository.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use bevy::asset::uuid::Uuid;
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
use resvg::{tiny_skia, usvg};
use serde::Deserialize;

use super::icons::{DENSITY, SIZES};

/// The handles of the icons' images start here.
const FIRST: u128 = 0x6861_6d6c_6574_5f66_696c_6573_0000_0000;

/// What has an icon: a file, or a folder open or closed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    File,
    Folder { open: bool },
}

/// `file_icons/icons.json`.
#[derive(Deserialize)]
struct Bundle {
    /// Whole names, in small letters, and their icons.
    names: HashMap<String, String>,
    /// Endings after a dot, in small letters, and their icons.
    extensions: HashMap<String, String>,
    folders: HashMap<String, String>,
    /// Each icon's SVG, by its name.
    icons: BTreeMap<String, String>,
}

fn bundle() -> &'static Bundle {
    static BUNDLE: OnceLock<Bundle> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        serde_json::from_str(include_str!("file_icons/icons.json"))
            .expect("file_icons/icons.json is a bundle")
    })
}

/// The name of the icon of `name`, as VS Code finds it: by the whole
/// name, then by its longest ending, then the plain file or folder.
pub fn icon_of(name: &str, kind: Kind) -> &'static str {
    let bundle = bundle();
    let name = name.to_lowercase();
    let found = match kind {
        Kind::Folder { open } => {
            bundle.folders.get(&name).and_then(|icon| {
                let icon = if open {
                    format!("{icon}_open")
                } else {
                    icon.clone()
                };
                bundle.icons.get_key_value(&icon).map(|(k, _)| k)
            })
        }
        Kind::File => bundle.names.get(&name).or_else(|| {
            name.match_indices('.').find_map(|(at, _)| {
                bundle.extensions.get(&name[at + 1..])
            })
        }),
    };
    let plain = match kind {
        Kind::File => "_file",
        Kind::Folder { open: false } => "_folder",
        Kind::Folder { open: true } => "_folder_open",
    };
    found.map_or(plain, String::as_str)
}

/// The image of the icon `which` for a side of `size`, drawn the
/// first time it is asked for.
pub fn file_icon(
    world: &mut World,
    which: &str,
    size: f32,
) -> Handle<Image> {
    let icons = &bundle().icons;
    let at = icons.keys().position(|name| name == which).unwrap_or(0);
    // The smallest drawing that is not stretched, or the largest.
    let drawn = SIZES
        .iter()
        .position(|drawn| *drawn >= size)
        .unwrap_or(SIZES.len() - 1);
    let index = (at * SIZES.len() + drawn) as u128;
    let handle = Handle::from(Uuid::from_u128(FIRST + index));
    let mut images = world.resource_mut::<Assets<Image>>();
    if images.get(&handle).is_none()
        && let Some(svg) = icons.get(which)
        && let Some(image) = drawing(svg, SIZES[drawn])
    {
        let _ = images.insert(&handle, image);
    }
    handle
}

/// `svg` in its own colours on nothing, filling a square of `size`;
/// none if it does not read.
fn drawing(svg: &str, size: f32) -> Option<Image> {
    let side = (size * DENSITY).round() as u32;
    let tree =
        usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let drawn = tree.size();
    let scale = side as f32 / drawn.width().max(drawn.height());
    let mut pixmap = tiny_skia::Pixmap::new(side, side)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let data = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let pixel = pixel.demultiply();
            [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]
        })
        .collect();
    Some(Image::new(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_finds_its_icon_as_vs_code_finds_it() {
        let file = |name| icon_of(name, Kind::File);
        assert_eq!(file("Cargo.toml"), "cargo");
        assert_eq!(file("main.rs"), "rust");
        assert_eq!(file("types.d.ts"), "typescript-def");
        assert_eq!(file("app.ts"), "typescript");
        assert_eq!(file("README.MD"), "readme");
        assert_eq!(file("no-ending"), "_file");
        assert_eq!(file("odd.unknown"), "_file");
        assert_eq!(
            icon_of("src", Kind::Folder { open: false }),
            "folder_src"
        );
        assert_eq!(
            icon_of("src", Kind::Folder { open: true }),
            "folder_src_open"
        );
        assert_eq!(
            icon_of("misc", Kind::Folder { open: true }),
            "_folder_open"
        );
    }

    #[test]
    fn every_icon_named_is_there_and_draws() {
        let bundle = bundle();
        for icon in bundle
            .names
            .values()
            .chain(bundle.extensions.values())
            .chain(bundle.folders.values())
        {
            assert!(bundle.icons.contains_key(icon), "{icon}");
        }
        for (name, svg) in &bundle.icons {
            assert!(drawing(svg, 14.0).is_some(), "{name}");
        }
    }
}
