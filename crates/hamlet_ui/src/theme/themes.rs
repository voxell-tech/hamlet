//! The themes a chief can pick, one family a file under
//! `assets/themes/`.

use std::fmt;

use bevy::app::{App, PreUpdate, Startup};
use bevy::asset::{Asset, AssetEvent, AssetServer, Assets, Handle};
use bevy::ecs::message::MessageReader;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Res, ResMut};
use bevy::log::{info, warn};
use bevy::reflect::TypePath;
use bevy::window::WindowTheme;
use hamlet_assets::{File, THEMES};
use serde::{Deserialize, Serialize};

use super::contrast::Report;
use super::{HamletTheme, Measures, Palette};

/// Reads a saved theme file again; adds nothing without an asset
/// server.
pub(super) fn plugin(app: &mut App) {
    if hamlet_assets::embed::<ThemeFile>(app, THEMES) {
        app.add_systems(Startup, open)
            .add_systems(PreUpdate, reread);
    }
}

/// A theme's id: its file's stem under `assets/themes/`.
#[derive(
    Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct ThemeId(String);

impl fmt::Display for ThemeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Hamlet's own family.
impl Default for ThemeId {
    fn default() -> Self {
        Self("hamlet".into())
    }
}

/// A family a chief can pick.
#[derive(Clone, PartialEq, Debug)]
pub struct Theme {
    pub id: ThemeId,
    pub name: String,
    pub variants: Variants,
}

/// A family's light variant, its dark one, or both.
#[derive(Clone, PartialEq, Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "a handful are made at launch and never moved"
)]
pub enum Variants {
    Light(Variant),
    Dark(Variant),
    Both { light: Variant, dark: Variant },
}

impl Variants {
    /// The variant for `appearance`, or the only one.
    pub fn get(&self, appearance: WindowTheme) -> &Variant {
        match (self, appearance) {
            (Self::Both { light, .. }, WindowTheme::Light) => light,
            (Self::Both { dark, .. }, WindowTheme::Dark) => dark,
            (Self::Light(only) | Self::Dark(only), _) => only,
        }
    }

    /// Each variant with its appearance, light first.
    pub fn iter(
        &self,
    ) -> impl Iterator<Item = (WindowTheme, &Variant)> {
        let (light, dark) = match self {
            Self::Light(light) => (Some(light), None),
            Self::Dark(dark) => (None, Some(dark)),
            Self::Both { light, dark } => (Some(light), Some(dark)),
        };
        light
            .map(|light| (WindowTheme::Light, light))
            .into_iter()
            .chain(dark.map(|dark| (WindowTheme::Dark, dark)))
    }
}

impl Theme {
    /// The contrast report of each variant, light first.
    pub fn reports(
        &self,
        measures: &Measures,
    ) -> impl Iterator<Item = (WindowTheme, Report)> {
        self.variants.iter().map(|(appearance, variant)| {
            let theme =
                HamletTheme::new(variant.palette.clone(), measures);
            let title = format!("{}: {}", self.name, variant.name);
            (appearance, Report::of(title, &theme))
        })
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Variant {
    pub name: String,
    pub palette: Palette,
}

/// Every shipped family, Hamlet first, then by name.
#[derive(Resource, Clone, Debug)]
pub struct Themes(Vec<Theme>);

impl Themes {
    /// The built-in files. Panics naming a file that does not read.
    pub fn shipped() -> Self {
        let mut themes =
            THEMES.iter().map(shipped).collect::<Vec<_>>();
        themes.sort_by_key(|theme| {
            (theme.id != ThemeId::default(), theme.name.clone())
        });
        Self(themes)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Theme> {
        self.0.iter()
    }

    /// Theme `id`, or Hamlet's when `id` no longer ships.
    pub fn get(&self, id: &ThemeId) -> &Theme {
        let hamlet = ThemeId::default();
        self.0
            .iter()
            .find(|theme| theme.id == *id)
            .or_else(|| {
                self.0.iter().find(|theme| theme.id == hamlet)
            })
            .expect("Hamlet's theme ships")
    }

    fn replace(&mut self, theme: Theme) {
        if let Some(kept) =
            self.0.iter_mut().find(|kept| kept.id == theme.id)
        {
            *kept = theme;
        }
    }
}

/// Hamlet's own family as it was built in.
pub(super) fn hamlet() -> Theme {
    let id = ThemeId::default();
    let file = THEMES
        .iter()
        .find(|file| file.stem() == id.0)
        .expect("assets/themes/hamlet.toml is listed");
    shipped(file)
}

/// A theme file as written.
#[derive(Asset, TypePath, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    name: String,
    light: Option<VariantFile>,
    dark: Option<VariantFile>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantFile {
    name: String,
    palette: Palette,
}

impl From<VariantFile> for Variant {
    fn from(file: VariantFile) -> Self {
        Self {
            name: file.name,
            palette: file.palette,
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum Unread {
    #[error("names neither a [light] nor a [dark] variant")]
    NoVariant,
}

fn read(id: ThemeId, file: ThemeFile) -> Result<Theme, Unread> {
    let variants = match (file.light, file.dark) {
        (Some(light), Some(dark)) => Variants::Both {
            light: light.into(),
            dark: dark.into(),
        },
        (Some(light), None) => Variants::Light(light.into()),
        (None, Some(dark)) => Variants::Dark(dark.into()),
        (None, None) => return Err(Unread::NoVariant),
    };
    Ok(Theme {
        id,
        name: file.name,
        variants,
    })
}

fn shipped(file: &File) -> Theme {
    toml::from_str(file.text())
        .map_err(|e| e.to_string())
        .and_then(|read_file| {
            read(ThemeId(file.stem().into()), read_file)
                .map_err(|e| e.to_string())
        })
        .unwrap_or_else(|e| panic!("assets/{}: {e}", file.name))
}

/// Each theme file as an asset, by its theme's id.
#[derive(Resource)]
struct Open(Vec<(ThemeId, Handle<ThemeFile>)>);

fn open(server: Res<AssetServer>, mut commands: Commands) {
    commands.insert_resource(Open(
        THEMES
            .iter()
            .map(|file| {
                (
                    ThemeId(file.stem().into()),
                    server.load(file.path()),
                )
            })
            .collect(),
    ));
}

/// Replaces a saved theme in [`Themes`] and warns of each pair that
/// no longer reads. A file that no longer reads keeps its last
/// values.
fn reread(
    mut changes: MessageReader<AssetEvent<ThemeFile>>,
    open: Res<Open>,
    files: Res<Assets<ThemeFile>>,
    measures: Res<Measures>,
    mut themes: ResMut<Themes>,
) {
    for change in changes.read() {
        let AssetEvent::Modified { id } = change else {
            continue;
        };
        let Some((theme, file)) = open
            .0
            .iter()
            .find(|(_, handle)| handle.id() == *id)
            .and_then(|(theme, _)| Some((theme, files.get(*id)?)))
        else {
            continue;
        };
        match read(theme.clone(), file.clone()) {
            Ok(read) => {
                info!("assets/themes/{}.toml read again", theme.0);
                for (_, report) in read.reports(&measures) {
                    if let Some(failing) = report.failing() {
                        warn!("{failing}");
                    }
                }
                themes.replace(read);
            }
            Err(why) => {
                warn!("assets/themes/{}.toml: {why}", theme.0);
            }
        }
    }
}
