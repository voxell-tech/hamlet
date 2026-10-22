//! What the bazaar's server says of the things it sells, as it says
//! it (`hamlet-server/docs/api.md`): listings, their versions, their
//! reviews, and who signed in.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What a listing sells, as the server names it.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Agent,
    Hamlet,
    /// An agent's tools.
    Plugin,
}

impl Kind {
    pub const ALL: [Kind; 3] =
        [Kind::Agent, Kind::Hamlet, Kind::Plugin];

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
pub(crate) enum Status {
    /// Never published.
    Draft,
    Listed,
    /// Taken off the shelves by its seller. Its owners keep it.
    Unlisted,
    /// Taken down by the bazaar.
    Removed,
}

impl Status {
    pub fn words(self) -> &'static str {
        match self {
            Status::Draft => "Draft",
            Status::Listed => "On the shelves",
            Status::Unlisted => "Off the shelves",
            Status::Removed => "Taken down by the bazaar",
        }
    }
}

/// A listing at the bazaar's server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Lot {
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
    /// Its picture, relative to the server, if it has one; its query
    /// changes with each new cover.
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
    #[serde(default)]
    pub updated_at: String,
}

impl Lot {
    /// The day it last changed, as `YYYY-MM-DD`.
    pub fn updated(&self) -> &str {
        self.updated_at.get(..10).unwrap_or(&self.updated_at)
    }

    /// Its newest version's number, empty with none.
    pub fn version(&self) -> &str {
        self.latest.as_ref().map_or("", |latest| &latest.version)
    }

    /// What its newest version holds, as the server read it.
    pub fn meta<T: Default + for<'de> Deserialize<'de>>(&self) -> T {
        self.latest
            .as_ref()
            .and_then(|latest| {
                serde_json::from_value(latest.meta.clone()).ok()
            })
            .unwrap_or_default()
    }
}

/// The one who sells a listing, as the server names them.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Maker {
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Their GitHub picture. Kept to be drawn once plugins' views
    /// can show a picture.
    #[serde(default)]
    pub avatar_url: Option<String>,
    /// Stripe takes payments for them.
    #[serde(default)]
    pub verified: bool,
}

impl Maker {
    /// The name they are shown by.
    pub fn shown(&self) -> &str {
        self.name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&self.login)
    }
}

/// One version of a listing.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Release {
    /// `MAJOR.MINOR.PATCH`.
    pub version: String,
    #[serde(default)]
    pub changes: String,
    /// The bytes of its package.
    #[serde(default)]
    pub size: u64,
    /// What is inside, as the server read it from the package.
    #[serde(default)]
    pub meta: Value,
}

/// What an agent's package holds.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(default)]
pub(crate) struct AgentMeta {
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
pub(crate) struct HamletMeta {
    pub name: String,
    pub title: String,
    pub kind: String,
    pub purpose: String,
    pub mayor: AgentMeta,
    pub agents: Vec<AgentMeta>,
}

/// What a package of tools holds.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(default)]
pub(crate) struct PluginMeta {
    pub tools: Vec<ToolMeta>,
}

/// A tool, and what it asks to be let do.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(default)]
pub(crate) struct ToolMeta {
    pub name: String,
    pub description: String,
    /// `run_program`, `network`, `read_files`, `write_files`.
    pub permissions: Vec<String>,
}

/// A listing on its own page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Detail {
    pub listing: Lot,
    #[serde(default)]
    pub reviews: Vec<Said>,
    /// The one signed in owns it.
    #[serde(default)]
    pub owned: bool,
}

/// One owner's review.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Said {
    pub by: Reviewer,
    pub stars: u8,
    #[serde(default)]
    pub words: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Reviewer {
    pub login: String,
}

/// The one signed in to the bazaar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct User {
    pub login: String,
    #[serde(default)]
    pub payouts: Option<Payouts>,
}

impl User {
    /// They may sell for a price.
    pub fn paid(&self) -> bool {
        self.payouts.is_some_and(|payouts| payouts.enabled)
    }
}

/// How a seller is paid, through Stripe.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Payouts {
    /// They have a Stripe account.
    pub connected: bool,
    /// Stripe lets it take payments.
    pub enabled: bool,
}

/// A listing the one signed in owns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Owned {
    pub listing: Lot,
}

/// What a listing is put up with, or changed to.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Draft {
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
pub(crate) struct Device {
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
pub(crate) struct Approved {
    pub token: String,
    pub user: User,
}

/// What came of asking to buy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Checkout {
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
pub(crate) enum Paying {
    Pending,
    Paid,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Purchase {
    pub status: Paying,
}

/// A page of listings, or of what is owned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Page<T> {
    pub items: Vec<T>,
    #[serde(default)]
    pub total: Option<u32>,
}

/// An address to open in the browser.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Link {
    pub url: String,
}

/// An upload in parts begun.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Upload {
    pub id: String,
}

/// `version` as numbers, to be ordered.
pub(crate) fn semver(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|part| {
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<u64>().ok())
            .flatten()
    });
    let found = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(found)
}

/// The version after `version`, by its smallest step.
pub(crate) fn next_version(version: &str) -> String {
    match semver(version) {
        Some((major, minor, patch)) => {
            format!("{major}.{minor}.{}", patch + 1)
        }
        None => "1.0.0".into(),
    }
}

/// `cents` as dollars: `$4.99`, or Free.
pub(crate) fn price_words(cents: u32) -> String {
    match cents {
        0 => "Free".into(),
        cents => format!("${}.{:02}", cents / 100, cents % 100),
    }
}
