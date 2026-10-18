//! `manifest.toml`: what an installed plugin says about itself.

use hamlet_sdk::plugin::Need;

/// A capability a plugin asks for, and why.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    pub capability: String,
    pub reason: String,
}

/// The contents of a plugin's `manifest.toml`.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    /// The version of the WIT package the plugin was built for.
    pub interface: String,
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
