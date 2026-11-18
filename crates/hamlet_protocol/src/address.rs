//! The addresses chiefs and agents name the Hand, leads, agents,
//! workers and profiles by: `hand`, `lead`, `lead@<hamlet>`,
//! `agent#<id>`, `worker#<id>` and `profile#<id>`.

use std::fmt;
use std::str::FromStr;

use crate::client::{AgentId, ProfileId};
use crate::{HAND, LEAD};

/// The most bytes of a hamlet's id in a lead's address.
const LONGEST_HAMLET: usize = 64;

/// An agent's or a profile's address.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Address {
    Hand,
    /// A hamlet's lead, by the hamlet's id, or the speaker's own.
    Lead {
        hamlet: Option<String>,
    },
    Agent(AgentId),
    Worker(AgentId),
    Profile(ProfileId),
}

/// Text that is no address.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not an address")]
pub struct NotAnAddress(pub String);

impl FromStr for Address {
    type Err = NotAnAddress;

    fn from_str(text: &str) -> Result<Self, NotAnAddress> {
        let bad = || NotAnAddress(text.into());
        if text == HAND {
            return Ok(Self::Hand);
        }
        if text == LEAD {
            return Ok(Self::Lead { hamlet: None });
        }
        if let Some(hamlet) = text.strip_prefix("lead@") {
            let plain = (1..=LONGEST_HAMLET).contains(&hamlet.len())
                && hamlet
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-');
            return match plain {
                true => Ok(Self::Lead {
                    hamlet: Some(hamlet.into()),
                }),
                false => Err(bad()),
            };
        }
        let (kind, id) = text.split_once('#').ok_or_else(bad)?;
        // `u64` alone would take a leading `+`.
        if !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        let id = id.parse::<u64>().map_err(|_| bad())?;
        match kind {
            "agent" => Ok(Self::Agent(AgentId(id))),
            "worker" => Ok(Self::Worker(AgentId(id))),
            "profile" => Ok(Self::Profile(ProfileId(id))),
            _ => Err(bad()),
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hand => f.write_str(HAND),
            Self::Lead { hamlet: None } => f.write_str(LEAD),
            Self::Lead {
                hamlet: Some(hamlet),
            } => write!(f, "{LEAD}@{hamlet}"),
            Self::Agent(id) => write!(f, "agent#{}", id.0),
            Self::Worker(id) => write!(f, "worker#{}", id.0),
            Self::Profile(id) => write!(f, "profile#{}", id.0),
        }
    }
}

/// `text` without the ` (<name>)` an address is shown and copied
/// with, as in `agent#2 (Mira)`, and without the `@` a line mentions
/// it after.
pub fn bare(text: &str) -> &str {
    let text = text.trim();
    let text = text.strip_prefix('@').unwrap_or(text);
    match text.split_once(" (") {
        Some((address, _)) if text.ends_with(')') => {
            address.trim_end()
        }
        _ => text,
    }
}

/// Whether `text` is an address.
pub fn is_address(text: &str) -> bool {
    text.parse::<Address>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_reads_back_as_it_was_written() {
        for text in [
            "hand",
            "lead",
            "lead@0f8fad5b-d9cb-469f-a165-70867728950e",
            "agent#2",
            "worker#7",
            "profile#4",
        ] {
            assert_eq!(
                text.parse::<Address>().unwrap().to_string(),
                text
            );
        }
        for text in
            ["agent#", "agent#+2", "agent#2a", "lead@", "pip#2"]
        {
            assert!(text.parse::<Address>().is_err(), "{text}");
        }
    }
}
