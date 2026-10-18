//! `manifest.toml`: what an installed plugin says about itself.

use hamlet_sdk::capability::{Asked, Asks, Refusal};
use hamlet_sdk::plugin::{Exports, Imports, Need, Range};

/// The name the host publishes its events under, which no plugin
/// may take.
pub const HOST: &str = "host";

/// A capability a plugin asks for, and why.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    pub capability: String,
    pub reason: String,
    /// What the capability is limited to, where it can be.
    #[serde(default)]
    pub scope: Vec<String>,
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
    /// Plugins that must be installed and running, or this one does
    /// not start.
    #[serde(default)]
    pub needs: Vec<Need>,
    /// Plugins this one uses when they are there.
    #[serde(default)]
    pub wants: Vec<Need>,
    #[serde(default)]
    pub exports: Exports,
    #[serde(default)]
    pub imports: Imports,
    #[serde(default)]
    pub asks: Vec<Ask>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Why the manifest cannot be, where it says what it cannot: a
    /// range that is not one, or a name that is not a name.
    pub fn problem(&self) -> Option<String> {
        let ranges =
            self.needs.iter().chain(&self.wants).find_map(|need| {
                Range::parse(&need.range).err().map(|why| {
                    format!("the range for `{}`: {why}", need.id)
                })
            });
        ranges
            .or_else(|| self.exports.problem())
            .or_else(|| self.imports.problem())
            .or_else(|| {
                (self.id == HOST).then(|| {
                    format!("`{HOST}` is the name of the host")
                })
            })
    }

    /// What the manifest asks for, or the first ask that cannot be
    /// granted.
    pub fn asked(&self) -> Result<Asks, Refusal> {
        self.asks
            .iter()
            .map(|ask| {
                Asked::parse(&ask.capability, &ask.reason, &ask.scope)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|asks| {
                let mut seen = Vec::new();
                // The first ask for a capability is the one kept.
                Asks(
                    asks.into_iter()
                        .filter(|ask| {
                            let new = !seen.contains(&ask.capability);
                            seen.push(ask.capability);
                            new
                        })
                        .collect(),
                )
            })
    }
}

impl From<&Manifest> for hamlet_sdk::plugin::Manifest {
    fn from(manifest: &Manifest) -> Self {
        Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            needs: manifest.needs.clone(),
            wants: manifest.wants.clone(),
            exports: manifest.exports.clone(),
            imports: manifest.imports.clone(),
            builtin: false,
        }
    }
}
