//! The addresses chiefs and agents name the Hand, mayors, hires,
//! workers and scholars by: `hand`, `mayor`, `mayor@<hamlet>`,
//! `hire#<id>`, `worker#<id>` and `scholar#<id>`.

use std::fmt;
use std::str::FromStr;

use crate::client::{AgentId, ScholarId};
use crate::{HAND, MAYOR};

/// The most bytes of a hamlet's id in a mayor's address.
const LONGEST_HAMLET: usize = 64;

/// An agent's or a scholar's address.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Address {
    Hand,
    /// A hamlet's mayor, by the hamlet's id, or the speaker's own.
    Mayor {
        hamlet: Option<String>,
    },
    Hire(AgentId),
    Worker(AgentId),
    Scholar(ScholarId),
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
        if text == MAYOR {
            return Ok(Self::Mayor { hamlet: None });
        }
        if let Some(hamlet) = text.strip_prefix("mayor@") {
            let plain = (1..=LONGEST_HAMLET).contains(&hamlet.len())
                && hamlet
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-');
            return match plain {
                true => Ok(Self::Mayor {
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
            "hire" => Ok(Self::Hire(AgentId(id))),
            "worker" => Ok(Self::Worker(AgentId(id))),
            "scholar" => Ok(Self::Scholar(ScholarId(id))),
            _ => Err(bad()),
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hand => f.write_str(HAND),
            Self::Mayor { hamlet: None } => f.write_str(MAYOR),
            Self::Mayor {
                hamlet: Some(hamlet),
            } => write!(f, "{MAYOR}@{hamlet}"),
            Self::Hire(id) => write!(f, "hire#{}", id.0),
            Self::Worker(id) => write!(f, "worker#{}", id.0),
            Self::Scholar(id) => write!(f, "scholar#{}", id.0),
        }
    }
}

/// `text` without the ` (<name>)` an address is shown and copied
/// with, as in `hire#2 (Mira)`.
pub fn bare(text: &str) -> &str {
    let text = text.trim();
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
            "mayor",
            "mayor@0f8fad5b-d9cb-469f-a165-70867728950e",
            "hire#2",
            "worker#7",
            "scholar#4",
        ] {
            assert_eq!(
                text.parse::<Address>().unwrap().to_string(),
                text
            );
        }
        for text in ["hire#", "hire#+2", "hire#2a", "mayor@", "pip#2"]
        {
            assert!(text.parse::<Address>().is_err(), "{text}");
        }
    }
}
