//! `manifest.toml`: what an installed plugin says about itself.

use hamlet_sdk::capability::{Asked, Asks, Capability, Refusal};
use hamlet_sdk::plugin::{
    Exports, Imports, Need, RESERVED, Range, joinable, plain, valid,
};

/// The most characters of a plugin's name.
const NAME_MAX: usize = 64;

/// The most characters of the reason of an ask.
const REASON_MAX: usize = 300;

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
    /// Whether the plugin is given a `tick`, at most `tick_hz` in a
    /// second. A plugin that only answers what is sent to it sets
    /// it to false. A manifest that does not say has true, as every
    /// manifest did before the field was.
    #[serde(default = "ticks_by_default")]
    pub ticks: bool,
}

fn ticks_by_default() -> bool {
    true
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
            .or_else(|| self.identity())
            .or_else(|| self.exports.problem())
            .or_else(|| self.imports.problem())
            .or_else(|| self.unasked_export())
    }

    /// Why the interfaces it exports cannot be published: it does not
    /// ask for `plugin.export`.
    fn unasked_export(&self) -> Option<String> {
        let asked = self.asks.iter().any(|ask| {
            ask.capability == Capability::PluginExport.name()
        });
        (!self.exports.interfaces.is_empty() && !asked).then(|| {
            "it exports interfaces and does not ask for \
             `plugin.export`"
                .into()
        })
    }

    /// Why the id, the name or a reason cannot be, if they cannot.
    fn identity(&self) -> Option<String> {
        if !valid(&self.id) {
            return Some(format!(
                "the id `{}` is not a name",
                plain(
                    &self
                        .id
                        .chars()
                        .take(NAME_MAX)
                        .collect::<String>()
                )
            ));
        }
        if !joinable(&self.id) {
            return Some(format!(
                "the id `{}` has a dot, `__` or an underscore at an \
                 edge",
                self.id
            ));
        }
        if RESERVED.contains(&self.id.as_str()) {
            return Some(format!("`{}` is a reserved id", self.id));
        }
        if self.name.chars().count() > NAME_MAX {
            return Some(format!(
                "the name has more than {NAME_MAX} characters"
            ));
        }
        self.asks
            .iter()
            .any(|ask| ask.reason.chars().count() > REASON_MAX)
            .then(|| {
                format!(
                    "a reason has more than {REASON_MAX} characters"
                )
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
            .and_then(|asks| match asks.conflict() {
                Some(refusal) => Err(refusal),
                None => Ok(asks),
            })
    }
}

impl From<&Manifest> for hamlet_sdk::plugin::Manifest {
    fn from(manifest: &Manifest) -> Self {
        Self {
            id: manifest.id.clone(),
            name: plain(&manifest.name),
            version: manifest.version.clone(),
            needs: manifest.needs.clone(),
            wants: manifest.wants.clone(),
            exports: manifest.exports.clone(),
            imports: manifest.imports.clone(),
            builtin: false,
            essential: false,
        }
    }
}
