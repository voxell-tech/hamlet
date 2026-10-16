//! What the bazaar's server says of the things it sells, as it says
//! it: listings, their versions, their reviews and who signed in.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Price, Seller, Standing};

/// What a listing sells.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Agent,
    Hamlet,
    Plugin,
}

impl Kind {
    pub const ALL: [Kind; 3] =
        [Kind::Agent, Kind::Hamlet, Kind::Plugin];

    /// Its name in the server's routes.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Agent => "agent",
            Kind::Hamlet => "hamlet",
            Kind::Plugin => "plugin",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Agent => "Agent",
            Kind::Hamlet => "Hamlet",
            Kind::Plugin => "Plugin",
        }
    }
}

/// Where a listing stands with its buyers.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Never published.
    Draft,
    Listed,
    /// Taken off the shelves by its seller. Its owners keep it.
    Unlisted,
    /// Taken down by the bazaar.
    Removed,
}

/// A listing at the bazaar's server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lot {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    /// What it is, in a line.
    pub summary: String,
    #[serde(default)]
    pub description: String,
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub price_cents: u32,
    pub status: Status,
    pub seller: Maker,
    /// The path of its cover on the server.
    #[serde(default)]
    pub cover_url: Option<String>,
    /// Its newest version, `None` until one is uploaded.
    #[serde(default)]
    pub latest: Option<Release>,
    #[serde(default)]
    pub owners: u32,
    /// Out of five, `0` with no reviews.
    #[serde(default)]
    pub stars: f32,
    #[serde(default)]
    pub reviews: u32,
    pub created_at: String,
    pub updated_at: String,
}

impl Lot {
    pub fn price(&self) -> Price {
        Price::of_cents(self.price_cents)
    }

    pub fn standing(&self) -> Standing {
        Standing {
            stars: self.stars,
            reviews: self.reviews,
            owners: self.owners,
        }
    }

    /// The day it last changed, as `YYYY-MM-DD`.
    pub fn updated(&self) -> &str {
        day(&self.updated_at)
    }

    /// Its newest version's number, empty with none.
    pub fn version(&self) -> &str {
        self.latest.as_ref().map_or("", |latest| &latest.version)
    }
}

/// The day of an ISO 8601 time.
pub fn day(time: &str) -> &str {
    time.get(..10).unwrap_or(time)
}

/// The one who sells a listing, as the server names them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Maker {
    pub id: String,
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub verified: bool,
}

impl Maker {
    pub fn seller(&self) -> Seller {
        Seller {
            name: self
                .name
                .clone()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| self.login.clone()),
            login: self.login.clone(),
            verified: self.verified,
        }
    }
}

/// One version of a listing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    /// `MAJOR.MINOR.PATCH`.
    pub version: String,
    #[serde(default)]
    pub changes: String,
    /// Bytes of its package.
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub sha256: String,
    pub created_at: String,
    /// What is inside, as the server read it from the package.
    #[serde(default)]
    pub meta: Value,
}

impl Release {
    pub fn agent(&self) -> Option<AgentMeta> {
        serde_json::from_value(self.meta.clone()).ok()
    }

    pub fn hamlet(&self) -> Option<HamletMeta> {
        serde_json::from_value(self.meta.clone()).ok()
    }

    pub fn plugin(&self) -> Option<PluginMeta> {
        serde_json::from_value(self.meta.clone()).ok()
    }
}

/// What an agent's package holds.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(default)]
pub struct AgentMeta {
    pub name: String,
    pub purpose: String,
    pub model: Option<String>,
    /// Its `CHARACTER.md`, cut short.
    pub character: String,
    pub skills: Vec<String>,
    /// It comes with its `NOTES.md`.
    pub notes: bool,
}

/// What a hamlet's package holds.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(default)]
pub struct HamletMeta {
    pub name: String,
    pub title: String,
    pub kind: String,
    pub purpose: String,
    /// The seed of its town's looks, in decimal.
    pub seed: String,
    pub mayor: AgentMeta,
    pub agents: Vec<AgentMeta>,
}

impl HamletMeta {
    pub fn seed(&self) -> u64 {
        self.seed.parse().unwrap_or_default()
    }
}

/// What a plugin's package holds.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(default)]
pub struct PluginMeta {
    pub tools: Vec<ToolMeta>,
}

/// A tool a plugin adds, and what it asks to be let do.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(default)]
pub struct ToolMeta {
    pub name: String,
    pub description: String,
    /// `run_program`, `network`, `read_files`, `write_files`.
    pub permissions: Vec<String>,
}

/// A listing on its own page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Detail {
    pub listing: Lot,
    /// The newest first.
    #[serde(default)]
    pub versions: Vec<Release>,
    #[serde(default)]
    pub reviews: Vec<Said>,
    /// The one signed in owns it.
    #[serde(default)]
    pub owned: bool,
    /// The one signed in sells it.
    #[serde(default)]
    pub mine: bool,
}

/// One owner's review.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Said {
    pub by: Reviewer,
    pub stars: u8,
    #[serde(default)]
    pub words: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reviewer {
    pub login: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

/// The one signed in to the bazaar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub payouts: Option<Payouts>,
}

impl User {
    /// They may sell for a price.
    pub fn paid(&self) -> bool {
        self.payouts.as_ref().is_some_and(|payouts| payouts.enabled)
    }
}

/// How a seller is paid, through Stripe.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Payouts {
    /// They have a Stripe account.
    pub connected: bool,
    /// Stripe lets it take payments.
    pub enabled: bool,
}

/// A listing the one signed in owns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Owned {
    pub listing: Lot,
    /// The newest first.
    #[serde(default)]
    pub versions: Vec<Release>,
    pub owned_since: String,
}

/// What a listing is put up with, or changed to.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    pub title: String,
    pub summary: String,
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
    pub price_cents: u32,
}

/// The sign-in begun with GitHub.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub device_code: String,
    /// The code the chief types at `verification_uri`.
    pub user_code: String,
    pub verification_uri: String,
    /// Seconds between asks.
    pub interval: u64,
    /// Seconds the code lasts.
    pub expires_in: u64,
}

/// A sign-in that went through.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Approved {
    pub token: String,
    pub user: User,
}

/// What came of asking to buy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bought {
    /// It is the buyer's already: it was free.
    pub owned: bool,
    #[serde(default)]
    pub purchase_id: Option<String>,
    /// Stripe's page to pay on.
    #[serde(default)]
    pub checkout_url: Option<String>,
}

/// Where a payment stands.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Paying {
    Pending,
    Paid,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PurchaseState {
    pub(crate) status: Paying,
}

/// A page of listings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Page<T> {
    pub(crate) items: Vec<T>,
    #[serde(default)]
    pub(crate) total: Option<u32>,
}

/// `version` as numbers, to be ordered.
pub fn semver(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|part| {
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<u64>().ok())
            .flatten()
    });
    let found = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(found)
}

/// The version after `version`, by its smallest step.
pub fn next_version(version: &str) -> String {
    match semver(version) {
        Some((major, minor, patch)) => {
            format!("{major}.{minor}.{}", patch + 1)
        }
        None => "1.0.0".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_read_and_stepped() {
        assert_eq!(semver("1.10.2"), Some((1, 10, 2)));
        assert!(semver("1.2.3") < semver("1.10.0"));
        assert_eq!(semver("1.2"), None);
        assert_eq!(semver("1.2.3.4"), None);
        assert_eq!(semver("1.-2.3"), None);
        assert_eq!(next_version("1.2.9"), "1.2.10");
        assert_eq!(next_version(""), "1.0.0");
    }

    #[test]
    fn a_listing_is_read_as_the_server_sends_it() {
        let lot = serde_json::from_str::<Lot>(
            r#"{"id":"l_1","kind":"plugin","title":"Cal","summary":"s",
            "description":"","category":"Data","tags":["a"],
            "priceCents":250,"currency":"usd","status":"listed",
            "seller":{"id":"u","login":"octo","name":null,
            "avatarUrl":null,"verified":true},"coverUrl":null,
            "latest":{"version":"1.0.0","changes":"","size":9,
            "sha256":"x","createdAt":"2026-10-01T00:00:00Z",
            "meta":{"tools":[{"name":"cal","description":"d",
            "permissions":["run_program","network"]}]}},
            "owners":3,"stars":4.5,"reviews":2,
            "createdAt":"2026-09-01T00:00:00Z",
            "updatedAt":"2026-10-01T00:00:00Z"}"#,
        )
        .unwrap();
        assert_eq!(lot.price(), Price::Once(2.5));
        assert_eq!(lot.seller.seller().name, "octo");
        assert_eq!(lot.updated(), "2026-10-01");
        let tools = lot.latest.unwrap().plugin().unwrap().tools;
        assert_eq!(tools[0].permissions, ["run_program", "network"]);
    }
}
