//! Plugins: tools that add to what every agent of Hamlet can do, as
//! the Tinker's Stall shows them.

use serde::{Deserialize, Serialize};

use super::lot::{Detail, Lot};
use super::{Listing, Price, Seller, Standing};

/// A plugin: tools that add to Hamlet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contraption {
    /// Its listing's id at the bazaar.
    pub id: String,
    pub name: String,
    pub publisher: Seller,
    pub version: String,
    /// The day it last changed, as `YYYY-MM-DD`.
    pub updated: String,
    pub price: Price,
    /// Its owners are those who installed it.
    pub standing: Standing,
    /// What it is, in a line.
    pub summary: String,
    pub description: String,
    /// The path of its cover on the bazaar's server.
    pub cover: Option<String>,
    pub kind: Option<Kind>,
    pub tags: Vec<String>,
    pub adds: Vec<Addition>,
    pub asks: Vec<Permission>,
    /// The latest change first.
    pub changes: Vec<Change>,
}

impl Contraption {
    /// The plugin `lot` sells.
    pub fn of(lot: &Lot) -> Contraption {
        let meta = lot
            .latest
            .as_ref()
            .and_then(|latest| latest.plugin())
            .unwrap_or_default();
        let mut asks = Vec::<Permission>::new();
        for tool in &meta.tools {
            for asked in &tool.permissions {
                if let Some(permission) = Permission::named(asked)
                    && !asks.contains(&permission)
                {
                    asks.push(permission);
                }
            }
        }
        asks.sort();
        Contraption {
            id: lot.id.clone(),
            name: lot.title.clone(),
            publisher: lot.seller.seller(),
            version: lot.version().to_string(),
            updated: lot.updated().to_string(),
            price: lot.price(),
            standing: lot.standing(),
            summary: lot.summary.clone(),
            description: lot.description.clone(),
            cover: lot.cover_url.clone(),
            kind: Kind::named(&lot.category),
            tags: lot.tags.clone(),
            adds: meta
                .tools
                .iter()
                .map(|tool| Addition {
                    name: tool.name.clone(),
                    words: tool.description.clone(),
                })
                .collect(),
            asks,
            changes: lot
                .latest
                .iter()
                .map(|latest| Change {
                    version: latest.version.clone(),
                    words: latest.changes.clone(),
                })
                .collect(),
        }
    }

    /// The plugin of `detail`, with every version's changes.
    pub fn of_detail(detail: &Detail) -> Contraption {
        Contraption {
            changes: detail
                .versions
                .iter()
                .map(|release| Change {
                    version: release.version.clone(),
                    words: release.changes.clone(),
                })
                .collect(),
            ..Contraption::of(&detail.listing)
        }
    }
}

/// The shelf of the stall a plugin sits on.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum Kind {
    Drawing,
    Planning,
    Data,
    Language,
    Maps,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::Drawing,
        Kind::Planning,
        Kind::Data,
        Kind::Language,
        Kind::Maps,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Drawing => "Diagrams",
            Kind::Planning => "Productivity",
            Kind::Data => "Data",
            Kind::Language => "Language",
            Kind::Maps => "Integrations",
        }
    }

    /// The shelf a listing's category names.
    pub fn named(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| {
            kind.name().eq_ignore_ascii_case(name.trim())
        })
    }
}

/// A tool a plugin adds, for every agent to call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Addition {
    pub name: String,
    pub words: String,
}

/// Something a plugin's tools ask to be let do, as their manifests
/// say.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub enum Permission {
    RunProgram,
    ReadFiles,
    WriteFiles,
    Network,
}

impl Permission {
    /// The permission the server names `name`.
    pub fn named(name: &str) -> Option<Permission> {
        match name {
            "run_program" => Some(Permission::RunProgram),
            "read_files" => Some(Permission::ReadFiles),
            "write_files" => Some(Permission::WriteFiles),
            "network" => Some(Permission::Network),
            _ => None,
        }
    }

    pub fn words(&self) -> String {
        match self {
            Permission::RunProgram => {
                "Run its own program, in the sandbox".into()
            }
            Permission::ReadFiles => {
                "Read files its manifest names".into()
            }
            Permission::WriteFiles => {
                "Write files its manifest names".into()
            }
            Permission::Network => "Reach the internet".into(),
        }
    }

    /// True when it lets the plugin change what is the chief's, or
    /// send it away.
    pub fn grave(&self) -> bool {
        matches!(self, Permission::WriteFiles | Permission::Network)
    }
}

/// What one version changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub version: String,
    pub words: String,
}

impl Listing for Contraption {
    fn name(&self) -> &str {
        &self.name
    }

    fn told(&self) -> String {
        let adds = self
            .adds
            .iter()
            .map(|addition| addition.name.as_str())
            .collect::<Vec<_>>();
        format!(
            "{} {} {} {} {} {} {}",
            self.name,
            self.publisher.name,
            self.publisher.login,
            self.summary,
            self.kind.map_or("", Kind::name),
            self.tags.join(" "),
            adds.join(" ")
        )
    }

    fn price(&self) -> Price {
        self.price
    }

    fn standing(&self) -> Standing {
        self.standing
    }

    fn updated(&self) -> &str {
        &self.updated
    }
}
