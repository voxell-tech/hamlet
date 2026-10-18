//! What the chief answered for one plugin, as it is written to
//! `grants.toml`.

use std::collections::BTreeMap;

use super::ask::{Asked, Asks};
use super::choice::Choice;
use super::granted::Choices;
use super::name::Capability;

/// Whether the chief let a plugin in.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Installation {
    Installed,
    Declined,
}

/// The chief's answers for a plugin: the version they were given for,
/// the capabilities that were asked about, and the choice for each.
#[derive(
    Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub state: Installation,
    pub version: String,
    /// The capabilities of the manifest the chief was asked about.
    #[serde(default)]
    pub asks: Vec<String>,
    /// The choice for each of `asks` the chief made. A declined
    /// plugin has none.
    #[serde(default)]
    pub choices: BTreeMap<String, Choice>,
    /// The scope each of `asks` had when the chief was asked. A file
    /// written before scopes were kept has none, which is not the
    /// same as an empty scope.
    #[serde(default)]
    pub scopes: BTreeMap<String, Vec<String>>,
}

/// What a record says about a plugin whose manifest is now `asked`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Standing {
    /// The chief was never asked, or was asked about another plugin
    /// than this one.
    Unknown,
    /// The chief said no to this very plugin.
    Declined,
    Installed {
        /// The choices that still apply.
        choices: Choices,
        /// What the manifest asks that the chief was not asked
        /// about.
        fresh: Vec<Capability>,
    },
}

impl Record {
    /// A plugin the chief let in, with the choices it made.
    pub fn installed(version: &str, choices: &Choices) -> Self {
        Self {
            state: Installation::Installed,
            version: version.into(),
            asks: choices
                .answered()
                .iter()
                .map(|(capability, _)| capability.name().into())
                .collect(),
            choices: choices
                .answered()
                .iter()
                .map(|(capability, choice)| {
                    (capability.name().into(), *choice)
                })
                .chain(choices.answered_targets().iter().map(
                    |(capability, target, choice)| {
                        (
                            format!("{}:{target}", capability.name()),
                            *choice,
                        )
                    },
                ))
                .collect(),
            scopes: BTreeMap::new(),
        }
    }

    /// This record, with the scope in `asked` for each capability it
    /// holds an answer for.
    pub fn scoped(mut self, asked: &Asks) -> Self {
        self.scopes = asked
            .0
            .iter()
            .filter(|ask| {
                self.asks
                    .iter()
                    .any(|name| name == ask.capability.name())
            })
            .map(|ask| {
                (ask.capability.name().into(), ask.scope.0.clone())
            })
            .collect();
        self
    }

    /// A plugin the chief said no to, with what it asked.
    pub fn declined(version: &str, asked: &Asks) -> Self {
        Self {
            state: Installation::Declined,
            version: version.into(),
            asks: asked
                .capabilities()
                .iter()
                .map(|capability| capability.name().into())
                .collect(),
            choices: BTreeMap::new(),
            scopes: BTreeMap::new(),
        }
    }

    /// Whether `ask` has a scope other than the one the chief was
    /// asked about. A scope that was never kept counts as changed
    /// only when the version moved on and the ask names one.
    fn changed(&self, ask: &Asked, version: &str) -> bool {
        match self.scopes.get(ask.capability.name()) {
            Some(known) => {
                let sorted = |scope: &[String]| {
                    let mut scope = scope.to_vec();
                    scope.sort();
                    scope.dedup();
                    scope
                };
                sorted(known) != sorted(&ask.scope.0)
            }
            None => {
                self.version != version && !ask.scope.0.is_empty()
            }
        }
    }

    fn answered(&self) -> Vec<Capability> {
        self.asks
            .iter()
            .filter_map(|name| Capability::parse(name).ok())
            .collect()
    }
}

/// What `record` says about the plugin of `version` whose manifest
/// asks `asked`.
pub fn standing(
    record: Option<&Record>,
    version: &str,
    asked: &Asks,
) -> Standing {
    let Some(record) = record else {
        return Standing::Unknown;
    };
    let wanted = asked.capabilities();
    let answered = record.answered();
    if record.state == Installation::Declined {
        let same = record.version == version
            && wanted.len() == answered.len()
            && wanted.iter().all(|c| answered.contains(c));
        return if same {
            Standing::Declined
        } else {
            Standing::Unknown
        };
    }
    let choices = record
        .choices
        .iter()
        .filter_map(|(name, choice)| {
            let (name, target) = match name.split_once(':') {
                Some((name, target)) => (name, Some(target)),
                None => (name.as_str(), None),
            };
            Some((Capability::parse(name).ok()?, target, *choice))
        })
        .fold(
            Choices::default(),
            |choices, (capability, target, choice)| match target {
                Some(target) => choices
                    .decided_for(capability, target, choice, false),
                None => choices.decided(capability, choice, false),
            },
        )
        .within(&wanted);
    let fresh = asked
        .0
        .iter()
        .filter(|ask| {
            !answered.contains(&ask.capability)
                || record.changed(ask, version)
        })
        .map(|ask| ask.capability)
        .collect::<Vec<_>>();
    let kept = wanted
        .into_iter()
        .filter(|capability| !fresh.contains(capability))
        .collect::<Vec<_>>();
    Standing::Installed {
        choices: choices.within(&kept),
        fresh,
    }
}
