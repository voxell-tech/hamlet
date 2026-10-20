//! A plugin's manifest: who it is and what it asks for.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::API_VERSION;
use crate::asset::{AssetTypes, TypeName};
use crate::capability::{Ask, Capability};
use crate::id::PluginId;

/// The longest a name, version, target or reason may be, in bytes.
const LONGEST: usize = 256;

/// A plugin's `plugin.toml`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: PluginId,
    /// What the person sees it called.
    pub name: String,
    pub version: String,
    /// The [`API_VERSION`] it was built against.
    pub api: String,
    #[serde(default)]
    pub asks: Vec<Ask>,
    #[serde(default)]
    pub assets: AssetTypes,
}

impl Manifest {
    /// A manifest of `id`, `name` and `version` against this API that
    /// asks for and shares nothing.
    pub fn new(id: PluginId, name: &str, version: &str) -> Self {
        Self {
            id,
            name: name.into(),
            version: version.into(),
            api: API_VERSION.into(),
            asks: Vec::new(),
            assets: AssetTypes::default(),
        }
    }
}

/// Why a manifest is refused.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ManifestError {
    /// A name, version, target or reason that is empty, too long or
    /// holds a control character: which.
    Text(&'static str),
    /// Built against another API version: that one.
    Api(String),
    /// A target that holds a space.
    Target(String),
    /// The same capability and target asked for twice.
    Twice(Capability, String),
    /// A type of asset named twice, or both made and used.
    TypeTwice(TypeName),
    /// Something it takes from another plugin without asking for it:
    /// the capability and the target it needs.
    Unasked(Capability, String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(what) => {
                write!(f, "the {what} is empty or not readable")
            }
            Self::Api(api) => write!(
                f,
                "built against API {api}, and this engine has \
                 {API_VERSION}"
            ),
            Self::Target(target) => {
                write!(f, "{target:?} is not a target")
            }
            Self::Twice(capability, target) => write!(
                f,
                "{} for {target:?} is asked for twice",
                capability.name()
            ),
            Self::TypeTwice(name) => {
                write!(f, "the type {name} is named twice")
            }
            Self::Unasked(capability, target) => write!(
                f,
                "it takes {target} without asking for {} {target}",
                capability.name()
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Whether `text` can be shown to the person as it is.
fn readable(text: &str) -> bool {
    !text.trim().is_empty()
        && text.len() <= LONGEST
        && !text.chars().any(char::is_control)
}

impl Manifest {
    /// Whether the engine can take this manifest.
    pub fn check(&self) -> Result<(), ManifestError> {
        for (what, text) in
            [("name", &self.name), ("version", &self.version)]
        {
            if !readable(text) {
                return Err(ManifestError::Text(what));
            }
        }
        if self.api != API_VERSION {
            return Err(ManifestError::Api(self.api.clone()));
        }
        for (at, ask) in self.asks.iter().enumerate() {
            if !readable(&ask.reason) {
                return Err(ManifestError::Text("reason"));
            }
            if !readable(&ask.target) {
                return Err(ManifestError::Text("target"));
            }
            if ask.target.chars().any(char::is_whitespace) {
                return Err(ManifestError::Target(
                    ask.target.clone(),
                ));
            }
            let twice = self.asks[..at].iter().any(|before| {
                before.capability == ask.capability
                    && before.target == ask.target
            });
            if twice {
                return Err(ManifestError::Twice(
                    ask.capability,
                    ask.target.clone(),
                ));
            }
        }
        let types = self.assets.makes.iter().chain(&self.assets.uses);
        for (at, kind) in types.clone().enumerate() {
            if types
                .clone()
                .take(at)
                .any(|before| before.name == kind.name)
            {
                return Err(ManifestError::TypeTwice(
                    kind.name.clone(),
                ));
            }
        }
        for used in &self.assets.uses {
            self.asked(Capability::AssetRead, used.name.as_str())?;
        }
        Ok(())
    }

    /// Whether it asks for `capability` on `target`.
    fn asked(
        &self,
        capability: Capability,
        target: &str,
    ) -> Result<(), ManifestError> {
        let asked = self.asks.iter().any(|ask| {
            ask.capability == capability && ask.target == target
        });
        if asked {
            Ok(())
        } else {
            Err(ManifestError::Unasked(capability, target.into()))
        }
    }
}
