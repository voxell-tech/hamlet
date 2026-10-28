//! The versions of the chief's listings as the bazaar's server says
//! them (`GET /v1/me/listings`), with the details each carries, the
//! fees to verify one (`GET /v1/sell/config`), the files of one
//! against the version before it, and the tags already in use.

use serde::{Deserialize, Serialize};

use crate::bazaar::market::lot::{Kind, Lot, Paying, semver};

/// Where a version stands.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub(crate) enum State {
    /// Its details can still be changed.
    Draft,
    /// Live, with no badge.
    Unverified,
    /// Paid for, and being checked.
    Checking,
    /// Live, with a badge.
    Verified,
    Rejected,
}

impl State {
    pub fn words(self) -> &'static str {
        match self {
            State::Draft => "Draft",
            State::Unverified => "Unverified",
            State::Checking => "Being checked",
            State::Verified => "\u{2713} Verified",
            State::Rejected => "Rejected",
        }
    }
}

/// What a version is listed with.
#[derive(
    Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Details {
    pub title: String,
    pub summary: String,
    pub description: String,
    pub price_cents: u32,
    pub tags: Vec<String>,
    /// Its picture, relative to the server, if it has one.
    #[serde(skip_serializing)]
    pub cover_url: Option<String>,
}

/// One version of a listing of the chief's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Version {
    /// `MAJOR.MINOR.PATCH`.
    pub version: String,
    pub state: State,
    /// When it was uploaded, in ISO 8601.
    #[serde(default)]
    pub created_at: String,
    /// The bytes of its package.
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub file_count: u32,
    #[serde(default)]
    pub details: Details,
    /// Why the checks rejected it, as `Agent 5 (Safety): ...`.
    #[serde(default)]
    pub reasons: Vec<String>,
    /// When a person at the bazaar was asked to look at it again.
    #[serde(default)]
    pub human_asked_at: Option<String>,
    /// When it went live, published or verified.
    #[serde(default)]
    pub published_at: Option<String>,
}

impl Version {
    /// Buyers get it: published or verified, being checked once
    /// published among them.
    pub fn is_live(&self) -> bool {
        match self.state {
            State::Unverified | State::Verified => true,
            State::Checking => self.published_at.is_some(),
            State::Draft | State::Rejected => false,
        }
    }

    /// When it was uploaded, as `YYYY-MM-DD HH:MM:SS UTC`.
    pub fn uploaded(&self) -> String {
        let at = self.created_at.trim_end_matches('Z');
        let at = at.split('.').next().unwrap_or(at).replace('T', " ");
        match at.is_empty() {
            true => String::new(),
            false => format!("{at} UTC"),
        }
    }
}

/// The versions of `lot`, newest first.
pub(crate) fn newest_first(lot: &Lot) -> Vec<Version> {
    let mut all = lot.versions.clone();
    all.sort_by_key(|one| std::cmp::Reverse(semver(&one.version)));
    all
}

/// The version before `version` of `lot`, if there is one.
pub(crate) fn before<'a>(
    lot: &'a Lot,
    version: &str,
) -> Option<&'a Version> {
    let at = semver(version);
    lot.versions
        .iter()
        .filter(|one| semver(&one.version) < at)
        .max_by_key(|one| semver(&one.version))
}

/// The details `lot` is shown with: those of its newest live version,
/// else its newest.
pub(crate) fn shown(lot: &Lot) -> Details {
    let all = newest_first(lot);
    all.iter()
        .find(|one| one.is_live())
        .or(all.first())
        .map(|one| one.details.clone())
        .or_else(|| lot.shown.clone())
        .unwrap_or_else(|| Details {
            title: lot.title.clone(),
            summary: lot.summary.clone(),
            description: lot.description.clone(),
            price_cents: lot.price_cents,
            tags: lot.tags.clone(),
            cover_url: lot.cover_url.clone(),
        })
}

/// Where `lot` stands as a whole, for its row: being checked while
/// its newest version is, else as its newest live version, else as
/// its newest.
pub(crate) fn standing(lot: &Lot) -> State {
    let all = newest_first(lot);
    let newest = all.first().map_or(State::Draft, |one| one.state);
    if newest == State::Checking {
        return newest;
    }
    all.iter()
        .find(|one| one.is_live())
        .map_or(newest, |one| one.state)
}

/// The bazaar's terms for selling (`GET /v1/sell/config`).
#[derive(
    Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct Fees {
    pub kinds: Vec<KindFee>,
    /// The fee to verify a version of a listing that has had a
    /// verified version.
    pub update_verify_cents: u32,
    /// The least a paid listing may cost.
    pub min_price_cents: u32,
}

/// What selling a kind of listing costs and allows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KindFee {
    pub kind: Kind,
    /// It can be sold now.
    pub available: bool,
    pub verify_cents: u32,
    /// The largest package of it, in bytes.
    #[serde(default)]
    pub max_bytes: u64,
}

impl Fees {
    pub fn of(&self, kind: Kind) -> Option<&KindFee> {
        self.kinds.iter().find(|one| one.kind == kind)
    }

    /// The fee to verify a version of a listing of `kind` with
    /// `versions`: the update fee once any of them was verified.
    pub fn to_verify(&self, kind: Kind, versions: &[Version]) -> u32 {
        let verified =
            versions.iter().any(|one| one.state == State::Verified);
        match verified {
            true => self.update_verify_cents,
            false => self.of(kind).map_or(0, |one| one.verify_cents),
        }
    }
}

/// How a file of a version differs from the version before.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Change {
    Added,
    Changed,
    Removed,
    Same,
}

impl Change {
    pub fn words(self) -> &'static str {
        match self {
            Change::Added => "added",
            Change::Changed => "changed",
            Change::Removed => "removed",
            Change::Same => "same",
        }
    }
}

/// A file of a version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct File {
    pub path: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub executable: bool,
    pub change: Change,
    /// Its words, for a text file.
    #[serde(default)]
    pub text: Option<String>,
    /// Its words in the version before.
    #[serde(default)]
    pub previous_text: Option<String>,
}

/// The files of a version against another
/// (`GET /v1/listings/:id/versions/:v/files?against=`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct FilesOf {
    pub files: Vec<File>,
}

/// A payment, as `GET /v1/payments/:id` says it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Payment {
    pub status: Paying,
}

/// A tag in use, and how many listings have it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TagCount {
    pub tag: String,
    #[serde(default)]
    pub count: u32,
}

/// Stripe's page to pay a version's verification on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Verifying {
    pub checkout_url: String,
    #[serde(default)]
    pub payment_id: String,
    #[serde(default)]
    pub amount_cents: u32,
}

/// `bytes` in a few words: `53 KB`.
pub(crate) fn size_words(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{} KB", bytes.div_ceil(1024)),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(state: State) -> Version {
        Version {
            version: "0.0.1".into(),
            state,
            created_at: String::new(),
            size: 0,
            file_count: 0,
            details: Details::default(),
            reasons: Vec::new(),
            human_asked_at: None,
            published_at: None,
        }
    }

    #[test]
    fn the_update_fee_is_charged_once_a_version_was_verified() {
        let fees = Fees {
            kinds: vec![KindFee {
                kind: Kind::Hamlet,
                available: true,
                verify_cents: 400,
                max_bytes: 0,
            }],
            update_verify_cents: 99,
            min_price_cents: 99,
        };
        let first = [version(State::Draft), version(State::Rejected)];
        assert_eq!(fees.to_verify(Kind::Hamlet, &first), 400);
        let later = [version(State::Verified), version(State::Draft)];
        assert_eq!(fees.to_verify(Kind::Hamlet, &later), 99);
        assert_eq!(fees.to_verify(Kind::Agent, &first), 0);
    }
}
