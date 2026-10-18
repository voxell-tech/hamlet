//! `manifest.toml`: what an installed plugin says about itself.

use hamlet_sdk::plugin::Need;

/// A capability a plugin asks for, and why.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    pub capability: String,
    pub reason: String,
}

/// What an installed plugin is made of.
#[derive(
    Clone, Copy, PartialEq, Eq, Default, Debug, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A WASM component, `plugin.wasm`.
    #[default]
    Wasm,
    /// A panel written in the language, `panel.ui`, and no code.
    Data,
}

/// The contents of a plugin's `manifest.toml`.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub kind: Kind,
    /// The version of the WIT package the plugin was built for. A
    /// WASM plugin needs it.
    pub interface: Option<String>,
    /// The icon of a data plugin's panel, by the name of a `Glyph`.
    pub icon: Option<String>,
    #[serde(default)]
    pub needs: Vec<Need>,
    #[serde(default)]
    pub exports: Vec<String>,
    #[serde(default)]
    pub imports: Vec<String>,
    #[serde(default)]
    pub asks: Vec<Ask>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

impl From<&Manifest> for hamlet_sdk::plugin::Manifest {
    fn from(manifest: &Manifest) -> Self {
        Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            needs: manifest.needs.clone(),
            builtin: false,
        }
    }
}
