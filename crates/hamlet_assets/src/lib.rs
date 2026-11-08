//! The files under `assets/`, built into the app: the tuning values,
//! the colours and the typefaces. A tuning file is also a Bevy asset,
//! so a saved change reaches an app run with `hot-reload`.

pub mod tint;

use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use bevy_app::{App, Plugin, PreUpdate, Startup};
use bevy_asset::io::Reader;
use bevy_asset::io::embedded::EmbeddedAssetRegistry;
use bevy_asset::{
    Asset, AssetApp, AssetEvent, AssetLoader, AssetPlugin,
    AssetServer, Assets, Handle, LoadContext,
};
use bevy_ecs::error::BevyError;
use bevy_ecs::message::MessageReader;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Res};
use bevy_ecs::world::{Mut, World};
use bevy_log::info;
use bevy_reflect::TypePath;
use serde::de::DeserializeOwned;

/// A file under `assets/`, as it was when the app was built.
#[derive(Clone, Copy)]
pub struct File {
    /// Its name under `assets/`.
    pub name: &'static str,
    pub bytes: &'static [u8],
}

macro_rules! file {
    ($name:literal) => {
        File {
            name: $name,
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/",
                $name
            )),
        }
    };
}

/// Every tuning value of the world.
pub const WORLD: File = file!("world.toml");
/// The colours the world is made of.
pub const PALETTE: File = file!("palette.toml");
/// The tuning of the effects.
pub const VFX: File = file!("vfx.toml");
/// The panels' sizes and limits.
pub const UI: File = file!("ui.toml");
/// The widgets' effects.
pub const EFFECTS: File = file!("effects.toml");
/// The panels' lettering, spacing, shadow fall and motion, shared by
/// every theme.
pub const MEASURES: File = file!("measures.toml");

/// The themes a chief can pick, one family a file. The file's name,
/// less `.toml`, is the theme's id.
pub const THEMES: &[File] = &[
    file!("themes/catppuccin.toml"),
    file!("themes/hamlet.toml"),
    file!("themes/monokai_vox.toml"),
    file!("themes/tokyonight.toml"),
];

/// The typefaces. Their licences are beside them in `assets/fonts/`.
pub mod fonts {
    use super::File;

    /// Inter (SIL Open Font License), the interface typeface.
    pub const INTER: File = file!("fonts/InterVariable.ttf");
    /// Cinzel (SIL Open Font License), for headers and buttons.
    pub const CINZEL: File = file!("fonts/CinzelVariable.ttf");
    /// Twemoji (CC-BY 4.0), for emoji.
    pub const TWEMOJI: File = file!("fonts/TwemojiMozilla.ttf");
}

impl File {
    /// The file's whole path in the workspace's `assets/`, where it
    /// is built in from, which the watcher knows a saved file by.
    fn kept(&self) -> PathBuf {
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        crate_dir
            .ancestors()
            .nth(2)
            .unwrap_or(crate_dir)
            .join("assets")
            .join(self.name)
    }

    /// The path the asset server knows it by.
    pub fn path(&self) -> String {
        format!("embedded://hamlet_assets/{}", self.name)
    }

    pub fn text(&self) -> &'static str {
        std::str::from_utf8(self.bytes)
            .unwrap_or_else(|e| panic!("assets/{}: {e}", self.name))
    }

    /// Its name without folder or extension: `themes/hamlet.toml` is
    /// `hamlet`.
    pub fn stem(&self) -> &'static str {
        let name = self.name.rsplit('/').next().unwrap_or(self.name);
        name.split_once('.').map_or(name, |(stem, _)| stem)
    }
}

/// Registers `T`'s TOML loader and puts each of `files` in the
/// embedded registry, so each loads by [`File::path`] and a saved one
/// is read again under `hot-reload`. False without an asset server.
pub fn embed<T: Asset + DeserializeOwned>(
    app: &mut App,
    files: &[File],
) -> bool {
    if !app.is_plugin_added::<AssetPlugin>() {
        return false;
    }
    let registry = app.world().resource::<EmbeddedAssetRegistry>();
    for file in files {
        registry.insert_asset(
            file.kept(),
            Path::new("hamlet_assets").join(file.name).as_path(),
            file.bytes,
        );
    }
    app.init_asset::<T>()
        .register_asset_loader(TomlLoader::<T>(PhantomData));
    true
}

/// A file of tuning values, and the resources its sections become.
pub trait Tuning: Asset + DeserializeOwned + Clone {
    const FILE: File;

    /// The values as designed: the file as it was built in.
    fn designed() -> Self {
        toml::from_str(Self::FILE.text()).unwrap_or_else(|e| {
            panic!("assets/{}: {e}", Self::FILE.name)
        })
    }

    /// Puts every section that differs from `was` into the world.
    /// A section nobody saved a change to is left as the app has it.
    fn apply(&self, was: &Self, world: &mut World);
}

/// Gives each settings type its `Default`: its place in the designed
/// values of the [`Tuning`] `$file`.
#[macro_export]
macro_rules! designed {
    ($file:ty { $($kind:ty => $($at:ident).+),+ $(,)? }) => {
        $(impl Default for $kind {
            fn default() -> Self {
                <$file as $crate::Tuning>::designed().$($at).+
            }
        })+
    };
}

/// Replaces the resource `R` with `now` where the file's section
/// changed.
pub fn follow<R: Resource + Clone + PartialEq>(
    world: &mut World,
    was: &R,
    now: &R,
) {
    if was != now {
        world.insert_resource(now.clone());
    }
}

/// Loads `T`'s file as an asset and keeps the world in step with it.
/// Without an asset server the designed values stand.
pub struct TuningPlugin<T>(PhantomData<fn() -> T>);

impl<T> Default for TuningPlugin<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: Tuning> Plugin for TuningPlugin<T> {
    fn build(&self, app: &mut App) {
        if embed::<T>(app, &[T::FILE]) {
            app.add_systems(Startup, open::<T>)
                .add_systems(PreUpdate, reread::<T>);
        }
    }
}

/// The file as an asset, and its values when last applied.
#[derive(Resource)]
struct Open<T: Tuning> {
    file: Handle<T>,
    was: T,
}

fn open<T: Tuning>(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(Open {
        file: server.load::<T>(T::FILE.path()),
        was: T::designed(),
    });
}

fn reread<T: Tuning>(
    mut changes: MessageReader<AssetEvent<T>>,
    mut commands: Commands,
) {
    let saved = changes
        .read()
        .filter(|change| {
            matches!(change, AssetEvent::Modified { .. })
        })
        .count();
    if saved == 0 {
        return;
    }
    commands.queue(|world: &mut World| {
        world.resource_scope(|world, mut open: Mut<Open<T>>| {
            let Some(now) = world
                .resource::<Assets<T>>()
                .get(&open.file)
                .cloned()
            else {
                return;
            };
            now.apply(&open.was, world);
            info!("assets/{} read again", T::FILE.name);
            open.was = now;
        });
    });
}

#[derive(TypePath)]
struct TomlLoader<T>(PhantomData<fn() -> T>);

impl<T: Asset + DeserializeOwned> AssetLoader for TomlLoader<T> {
    type Asset = T;
    type Error = BevyError;
    type Settings = ();

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<T, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(toml::from_str(std::str::from_utf8(&bytes)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A theme file without its line in `THEMES` never ships.
    #[test]
    fn every_file_under_themes_is_listed() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/themes");
        let mut on_disk = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| {
                format!(
                    "themes/{}",
                    entry.unwrap().file_name().display()
                )
            })
            .collect::<Vec<_>>();
        on_disk.sort();
        let mut listed =
            THEMES.iter().map(|file| file.name).collect::<Vec<_>>();
        listed.sort_unstable();
        assert_eq!(on_disk, listed);
    }
}
