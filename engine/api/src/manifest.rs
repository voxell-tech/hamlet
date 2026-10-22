//! A plugin's manifest: who it is and what it asks for.

use core::fmt;
use std::collections::BTreeMap;

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

use crate::API_VERSION;
use crate::asset::{AssetTypes, TypeName};
use crate::capability::{Ask, Capability};
use crate::event::Events;
use crate::id::{PluginId, is_name};
use crate::limits::Limits;
use crate::service::Services;

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
    #[serde(default)]
    pub services: Services,
    #[serde(default)]
    pub events: Events,
    /// The limits of its instance it asks for in place of the
    /// engine's.
    #[serde(default)]
    pub limits: Limits,
    /// The plugins it needs: each id with a semver requirement of
    /// its version, `"^0.1"`.
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    /// The environment variables it reads, each with the value it
    /// has where the engine's environment does not set it.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// A plugin another needs, and the versions of it that fit.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Dependency {
    pub id: PluginId,
    pub requirement: VersionReq,
}

impl Dependency {
    /// Whether `version` fits. One that is not semver never does.
    pub fn fits(&self, version: &str) -> bool {
        Version::parse(version)
            .is_ok_and(|version| self.requirement.matches(&version))
    }
}

impl Manifest {
    /// A manifest of `id`, `name` and `version` against this API that
    /// asks for, shares and needs nothing.
    pub fn new(id: PluginId, name: &str, version: &str) -> Self {
        Self {
            id,
            name: name.into(),
            version: version.into(),
            api: API_VERSION.into(),
            asks: Vec::new(),
            assets: AssetTypes::default(),
            services: Services::default(),
            events: Events::default(),
            limits: Limits::default(),
            dependencies: BTreeMap::new(),
            env: BTreeMap::new(),
        }
    }

    /// Its dependencies, by id, leaving out each that
    /// [`check`](Self::check) refuses.
    pub fn needs(&self) -> Vec<Dependency> {
        self.dependencies
            .iter()
            .filter_map(|(id, requirement)| {
                Some(Dependency {
                    id: PluginId::try_from(id.as_str()).ok()?,
                    requirement: VersionReq::parse(requirement)
                        .ok()?,
                })
            })
            .collect()
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
    /// A service or event that is not well named, or named twice:
    /// which.
    Shared(String),
    /// A limit asked for that is zero: which.
    Limit(&'static str),
    /// A dependency whose id is not a plugin's or whose requirement
    /// is not semver: the two.
    Dependency(String, String),
    /// It names itself among its dependencies.
    OnItself,
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
            Self::Shared(name) => {
                write!(f, "{name:?} is not a name, or named twice")
            }
            Self::Limit(limit) => {
                write!(f, "the limit {limit} is not above zero")
            }
            Self::Dependency(id, requirement) => write!(
                f,
                "the dependency {id:?} = {requirement:?} is not a \
                 plugin id with a version requirement"
            ),
            Self::OnItself => write!(f, "it depends on itself"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Whether each of `names` is `valid` and none comes twice.
fn once_each<'a>(
    names: impl Iterator<Item = &'a str>,
    valid: fn(&str) -> bool,
) -> Result<(), ManifestError> {
    let mut seen = Vec::new();
    for name in names {
        if !valid(name) || seen.contains(&name) {
            return Err(ManifestError::Shared(name.into()));
        }
        seen.push(name);
    }
    Ok(())
}

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
        let exports = self
            .services
            .exports
            .iter()
            .map(|export| &export.name[..]);
        let imports = self
            .services
            .imports
            .iter()
            .map(|import| import.service.as_str());
        once_each(exports, is_name)?;
        once_each(imports, |_| true)?;
        for import in &self.services.imports {
            self.asked(
                Capability::ServiceCall,
                import.service.as_str(),
            )?;
        }
        let sends =
            self.events.sends.iter().map(|sent| &sent.name[..]);
        let hears = self
            .events
            .hears
            .iter()
            .map(|heard| heard.event.as_str());
        once_each(sends, is_name)?;
        once_each(hears, |_| true)?;
        for (id, requirement) in &self.dependencies {
            if PluginId::try_from(id.as_str()).is_err()
                || VersionReq::parse(requirement).is_err()
            {
                return Err(ManifestError::Dependency(
                    id.clone(),
                    requirement.clone(),
                ));
            }
            if id == self.id.as_str() {
                return Err(ManifestError::OnItself);
            }
        }
        self.limits.check().map_err(ManifestError::Limit)
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
