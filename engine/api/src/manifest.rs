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

/// The most environment variables a manifest may name.
const VARIABLES: usize = 16;

/// The longest an environment variable's name may be, in bytes.
const VARIABLE: usize = 64;

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

/// Whether `name` can be an environment variable a manifest names:
/// a capital letter, then capitals, digits and `_`, at most 64 bytes.
pub fn is_variable(name: &str) -> bool {
    name.len() <= VARIABLE
        && name.starts_with(|c: char| c.is_ascii_uppercase())
        && name.chars().all(|c| {
            c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'
        })
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

    /// Each environment variable it names, with its value as
    /// `lookup` gives it when that is not empty, else the default.
    pub fn environment(
        &self,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Vec<(String, String)> {
        self.env
            .iter()
            .map(|(name, default)| {
                let value = lookup(name)
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| default.clone());
                (name.clone(), value)
            })
            .collect()
    }
}

/// The variable an ask's target `$NAME` stands for: `NAME`.
pub fn variable_of(target: &str) -> Option<&str> {
    target.strip_prefix('$')
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
    /// An environment variable that is not well named, or whose
    /// default is not readable: which.
    Variable(String),
    /// More environment variables named than the most: how many.
    Variables(usize),
    /// An ask on a target `$NAME` that is not of `net.fetch`, or
    /// whose variable it does not name: the capability and target.
    Unnamed(Capability, String),
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
            Self::Variable(name) => write!(
                f,
                "the environment variable {name:?} is not a capital \
                 letter then capitals, digits and _ within \
                 {VARIABLE} bytes, or its default is empty or not \
                 readable"
            ),
            Self::Variables(count) => write!(
                f,
                "it names {count} environment variables, and \
                 {VARIABLES} is the most"
            ),
            Self::Unnamed(capability, target) => write!(
                f,
                "{} for {target:?}: only net.fetch takes a \
                 variable, and only one its [env] names",
                capability.name()
            ),
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
        if self.env.len() > VARIABLES {
            return Err(ManifestError::Variables(self.env.len()));
        }
        for (name, default) in &self.env {
            if !is_variable(name) || !readable(default) {
                return Err(ManifestError::Variable(name.clone()));
            }
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
            if let Some(name) = variable_of(&ask.target)
                && (ask.capability != Capability::NetFetch
                    || !self.env.contains_key(name))
            {
                return Err(ManifestError::Unnamed(
                    ask.capability,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest::new(
            PluginId::try_from("test.bazaar").unwrap(),
            "Bazaar",
            "0.1.0",
        )
    }

    fn fetch(target: &str) -> Ask {
        Ask {
            capability: Capability::NetFetch,
            target: target.into(),
            reason: "Reads the bazaar".into(),
        }
    }

    #[test]
    fn its_environment_is_named_well_and_not_too_long() {
        let mut named = manifest();
        named.env.insert(
            "HAMLET_BAZAAR_URL".into(),
            "http://localhost:8787".into(),
        );
        named.env.insert("X2_".into(), "on".into());
        named
            .env
            .insert(format!("A{}", "B".repeat(63)), "on".into());
        assert_eq!(named.check(), Ok(()));

        let long = format!("A{}", "B".repeat(64));
        for name in
            ["", "lower", "2X", "_X", "A-B", "A B", "É", &long]
        {
            let mut badly = manifest();
            badly.env.insert(name.into(), "on".into());
            assert_eq!(
                badly.check(),
                Err(ManifestError::Variable(name.into())),
                "{name:?} was taken"
            );
        }
        for default in ["", " ", "a\nb"] {
            let mut badly = manifest();
            badly.env.insert("X".into(), default.into());
            assert_eq!(
                badly.check(),
                Err(ManifestError::Variable("X".into()))
            );
        }

        let mut many = manifest();
        for at in 0..=VARIABLES {
            many.env.insert(format!("X{at}"), "on".into());
        }
        assert_eq!(
            many.check(),
            Err(ManifestError::Variables(VARIABLES + 1))
        );
    }

    #[test]
    fn only_net_fetch_asks_for_a_variable_it_names() {
        let mut asking = manifest();
        asking.env.insert(
            "HAMLET_BAZAAR_URL".into(),
            "http://localhost:8787".into(),
        );
        asking.asks.push(fetch("$HAMLET_BAZAAR_URL"));
        assert_eq!(asking.check(), Ok(()));

        let mut unnamed = asking.clone();
        unnamed.asks.push(fetch("$ELSEWHERE"));
        assert_eq!(
            unnamed.check(),
            Err(ManifestError::Unnamed(
                Capability::NetFetch,
                "$ELSEWHERE".into()
            ))
        );

        let mut other = asking.clone();
        other.asks.push(Ask {
            capability: Capability::ServiceCall,
            ..fetch("$HAMLET_BAZAAR_URL")
        });
        assert_eq!(
            other.check(),
            Err(ManifestError::Unnamed(
                Capability::ServiceCall,
                "$HAMLET_BAZAAR_URL".into()
            ))
        );
    }

    #[test]
    fn a_variable_is_the_lookups_unless_empty_else_its_default() {
        let mut named = manifest();
        named.env.insert("SET".into(), "default".into());
        named.env.insert("EMPTY".into(), "default".into());
        named.env.insert("UNSET".into(), "default".into());
        let lookup = |name: &str| match name {
            "SET" => Some("set".to_owned()),
            "EMPTY" => Some(String::new()),
            _ => None,
        };
        assert_eq!(
            named.environment(lookup),
            [
                ("EMPTY".to_owned(), "default".to_owned()),
                ("SET".to_owned(), "set".to_owned()),
                ("UNSET".to_owned(), "default".to_owned()),
            ]
        );
    }
}
