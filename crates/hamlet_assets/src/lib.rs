//! The files under `assets/`, built into the app: the tuning values,
//! the colours and the typefaces. A tuning file is also a Bevy asset,
//! so a saved change reaches an app run with `hot-reload`.

pub mod tint;

use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use bevy::asset::io::Reader;
use bevy::asset::io::embedded::EmbeddedAssetRegistry;
use bevy::asset::{AssetLoader, LoadContext, LoadState};
use bevy::ecs::error::BevyError;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
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
/// The limits on an installed plugin's calls.
pub const WASM: File = file!("wasm.toml");
/// The limits on a panel written in Hamlet's panel language.
pub const PANEL_LANG: File = file!("panel_lang.toml");
/// The panels' answers to the hand.
pub const UI: File = file!("ui.toml");
/// How the chat is drawn.
pub const CHAT: File = file!("chat.toml");
/// The panels' theme.
pub const THEME: File = file!("theme.toml");

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
    /// The file's path from the root of the workspace. The watcher
    /// looks there when the app is run with `cargo run`.
    fn kept(&self) -> PathBuf {
        Path::new("assets").join(self.name)
    }

    /// The path the asset server knows it by.
    fn path(&self) -> String {
        format!("embedded://hamlet_assets/{}", self.name)
    }

    fn text(&self) -> &'static str {
        std::str::from_utf8(self.bytes)
            .unwrap_or_else(|e| panic!("assets/{}: {e}", self.name))
    }
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

/// What has come of loading a [`Tuning`] file.
pub enum Loaded<T> {
    Waiting,
    Read(T),
    /// Not found, or not a file of `T`: why.
    Failed(String),
}

/// `T`'s file as [`TuningPlugin<T>`] opens it at startup. Without an
/// asset server it never loads.
#[derive(SystemParam)]
pub struct Loading<'w, T: Tuning> {
    open: Option<Res<'w, Open<T>>>,
    server: Option<Res<'w, AssetServer>>,
    files: Option<Res<'w, Assets<T>>>,
}

impl<T: Tuning> Loading<'_, T> {
    /// What has come of loading the file.
    pub fn loaded(&self) -> Loaded<T> {
        let (Some(open), Some(server), Some(files)) =
            (&self.open, &self.server, &self.files)
        else {
            return Loaded::Waiting;
        };
        match server.load_state(&open.file) {
            LoadState::Loaded => files
                .get(&open.file)
                .cloned()
                .map_or(Loaded::Waiting, Loaded::Read),
            LoadState::Failed(why) => Loaded::Failed(why.to_string()),
            _ => Loaded::Waiting,
        }
    }
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
        if !app.is_plugin_added::<AssetPlugin>() {
            return;
        }
        let file = T::FILE;
        app.world()
            .resource::<EmbeddedAssetRegistry>()
            .insert_asset(
                file.kept(),
                Path::new("hamlet_assets").join(file.name).as_path(),
                file.bytes,
            );
        app.init_asset::<T>()
            .register_asset_loader(TomlLoader::<T>(PhantomData))
            .add_systems(Startup, open::<T>)
            .add_systems(PreUpdate, reread::<T>);
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
struct TomlLoader<T: Tuning>(PhantomData<fn() -> T>);

impl<T: Tuning> AssetLoader for TomlLoader<T> {
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
