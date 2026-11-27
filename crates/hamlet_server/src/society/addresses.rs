//! Addresses: how chiefs and agents name the Hand, mayors, hires,
//! workers and profiles, by ids and never by a profile's name (C30,
//! H14).

use std::fmt;

use anyhow::Result;
use hamlet_messages::realm::HamletId;
use uuid::Uuid;

use super::refusal::Refusal;

/// A name for an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Address {
    Hand,
    /// A hamlet's mayor, or the speaker's own.
    Mayor {
        hamlet: Option<HamletId>,
    },
    Hire(i64),
    Worker(i64),
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hand => f.write_str("hand"),
            Self::Mayor { hamlet: None } => f.write_str("mayor"),
            Self::Mayor {
                hamlet: Some(hamlet),
            } => write!(f, "mayor@{}", hamlet.0),
            Self::Hire(id) => write!(f, "hire#{id}"),
            Self::Worker(id) => write!(f, "worker#{id}"),
        }
    }
}

/// Which agents an address may name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reach {
    Living,
    /// The ended too, but not the erased.
    Kept,
}

/// Reads an address: `hand`, `mayor`, `mayor@<hamlet>`, `hire#<id>`
/// or `worker#<id>`.
pub(crate) fn parse(text: &str) -> Result<Address, Refusal> {
    let text = text.trim();
    let bad = || Refusal::BadAddress {
        address: text.into(),
    };
    if text == "hand" {
        return Ok(Address::Hand);
    }
    if text == "mayor" {
        return Ok(Address::Mayor { hamlet: None });
    }
    if let Some(hamlet) = text.strip_prefix("mayor@") {
        return Ok(Address::Mayor {
            hamlet: Some(hamlet_id(hamlet).ok_or_else(bad)?),
        });
    }
    let (kind, id) = text.split_once('#').ok_or_else(bad)?;
    let id = id.parse::<i64>().map_err(|_| bad())?;
    match kind {
        "hire" => Ok(Address::Hire(id)),
        "worker" => Ok(Address::Worker(id)),
        _ => Err(bad()),
    }
}

/// Reads a profile's address, `profile#<id>`, to its number.
pub(crate) fn parse_profile(text: &str) -> Result<i64, Refusal> {
    let text = text.trim();
    text.strip_prefix("profile#")
        .and_then(|id| id.parse::<i64>().ok())
        .ok_or_else(|| Refusal::BadAddress {
            address: text.into(),
        })
}

/// The hamlet's id `text` is, in its one written form, if it is one.
fn hamlet_id(text: &str) -> Option<HamletId> {
    Uuid::try_parse(text.trim())
        .ok()
        .map(|id| HamletId(id.hyphenated().to_string()))
}
