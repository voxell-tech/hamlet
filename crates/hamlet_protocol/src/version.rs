//! The versions of Hamlet's protocols, in semver.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

/// A version in semver's `major.minor.patch` form.
///
/// Every protocol stays at 0.1.0 until Hamlet's first release.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Version {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Whether a peer at `other` speaks this version's protocol, by
    /// semver's caret: the same major version and, below 1.0, the
    /// same minor one.
    pub fn is_compatible(&self, other: &Self) -> bool {
        self.major == other.major
            && (self.major > 0 || self.minor == other.minor)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A text that is not a version.
#[derive(Debug, thiserror::Error)]
#[error("`{0}` is not a version: use major.minor.patch")]
pub struct BadVersion(String);

impl FromStr for Version {
    type Err = BadVersion;

    fn from_str(text: &str) -> Result<Self, BadVersion> {
        let bad = || BadVersion(text.to_owned());
        let mut parts = text.split('.').map(|part| {
            let leading_zero =
                part.len() > 1 && part.starts_with('0');
            match part.parse::<u64>() {
                Ok(number) if !leading_zero => Ok(number),
                _ => Err(bad()),
            }
        });
        let (Some(major), Some(minor), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(bad());
        };
        Ok(Self::new(major?, minor?, patch?))
    }
}

impl TryFrom<String> for Version {
    type Error = BadVersion;

    fn try_from(text: String) -> Result<Self, BadVersion> {
        text.parse()
    }
}

impl From<Version> for String {
    fn from(version: Version) -> Self {
        version.to_string()
    }
}

impl JsonSchema for Version {
    fn schema_name() -> Cow<'static, str> {
        "Version".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A version in semver's major.minor.patch form.",
            "type": "string",
            "pattern": r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_compatible_by_semver_s_caret() {
        let version = |text: &str| text.parse::<Version>().unwrap();
        assert!(version("0.1.0").is_compatible(&version("0.1.7")));
        assert!(!version("0.1.0").is_compatible(&version("0.2.0")));
        assert!(version("1.2.0").is_compatible(&version("1.9.3")));
        assert!(!version("1.2.0").is_compatible(&version("2.0.0")));
        for bad in ["1", "1.2", "1.2.3.4", "01.2.3", "1.x.3", ""] {
            assert!(bad.parse::<Version>().is_err(), "{bad}");
        }
    }
}
