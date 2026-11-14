//! An actor's address, the rule its names follow, and who made a
//! call. Plain data: nothing here depends on the host.

use std::fmt;

use serde::{Deserialize, Serialize};

/// An actor's address: its kind, which the host's directory maps to
/// where the kind's actors run, and its key within the kind. Both are
/// plain names, so an `Address` that exists is valid, and two never
/// name one file, also on a disk that ignores case. Its text is
/// `kind/key`.
///
/// Serde writes the map `{kind, key}`, and reads it through
/// [`Address::new`], so a decode refuses a name that is not plain.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(try_from = "Unchecked")]
pub struct Address {
    kind: String,
    key: String,
}

impl Address {
    /// Refuses a kind or a key that is not plain.
    pub fn new(
        kind: impl Into<String>,
        key: impl Into<String>,
    ) -> Result<Self, InvalidName> {
        let (kind, key) = (kind.into(), key.into());
        plain(&kind, Named::KindOrKey)?;
        plain(&key, Named::KindOrKey)?;
        Ok(Self { kind, key })
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn key(&self) -> &str {
        &self.key
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.kind, self.key)
    }
}

/// An address as it decodes, before its names are checked. Named
/// `Address` for serde, so a traced shape keeps its name.
#[derive(Deserialize)]
#[serde(rename = "Address")]
struct Unchecked {
    kind: String,
    key: String,
}

impl TryFrom<Unchecked> for Address {
    type Error = InvalidName;

    fn try_from(fields: Unchecked) -> Result<Self, InvalidName> {
        Self::new(fields.kind, fields.key)
    }
}

/// A name that is not plain: a kind or a key given to
/// [`Address::new`], or a kind or the host's name given to
/// `Host::open`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "`{name}` is not a valid {what}: use lower-case letters, \
     digits, `-`, `_` and `.`, and do not start with `.`"
)]
pub struct InvalidName {
    name: String,
    what: Named,
}

impl InvalidName {
    /// The name that was refused.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// What a refused name was to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Named {
    KindOrKey,
    Kind,
    Host,
}

impl fmt::Display for Named {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::KindOrKey => "actor kind or key",
            Self::Kind => "actor kind",
            Self::Host => "host name",
        })
    }
}

/// Refuses `name`, a `what`, unless it is plain: not empty, not
/// starting with `.`, at most 128 bytes, and only `a-z`, `0-9`, `-`,
/// `_` and `.`.
pub(crate) fn plain(
    name: &str,
    what: Named,
) -> Result<(), InvalidName> {
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && name.len() <= 128
        && name.bytes().all(|byte| {
            // With no upper-case letter, two names never name one
            // file, also on a disk that ignores case.
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_' | b'.')
        });
    if plain {
        Ok(())
    } else {
        Err(InvalidName {
            name: name.to_owned(),
            what,
        })
    }
}

/// Who made a call, as the host stamps it. The callee reads it from
/// its call's connection parameters, which are the CBOR of an
/// `Option<Caller>`: a call that no actor and no client made carries
/// none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Caller {
    /// The actor at this address.
    Actor(Address),
    /// A client of the program that opened the host.
    Client,
}

/// `kind/key` for an actor, `client` for a client.
impl fmt::Display for Caller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Actor(from) => from.fmt(f),
            Self::Client => f.write_str("client"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cbor<T: Serialize>(value: &T) -> Vec<u8> {
        let mut bytes = Vec::new();
        ciborium::into_writer(value, &mut bytes).unwrap();
        bytes
    }

    #[derive(Serialize)]
    struct Fields<'a> {
        kind: &'a str,
        key: &'a str,
    }

    #[test]
    fn a_name_that_is_not_plain_is_refused_by_name() {
        for (kind, key, name) in [
            ("counter", "A", "A"),
            ("counter", "a/b", "a/b"),
            ("counter", "", ""),
            ("counter", ".a", ".a"),
            ("Counter", "a", "Counter"),
        ] {
            let error = Address::new(kind, key).unwrap_err();
            assert_eq!(error.name(), name);
            assert_eq!(
                error.to_string(),
                format!(
                    "`{name}` is not a valid actor kind or key: use \
                     lower-case letters, digits, `-`, `_` and `.`, \
                     and do not start with `.`"
                )
            );
        }
        assert!(Address::new("counter", "a".repeat(128)).is_ok());
        assert!(Address::new("counter", "a".repeat(129)).is_err());
        assert!(Address::new("a-b_c.d", "0.9").is_ok());
    }

    #[test]
    fn a_decode_refuses_a_name_that_is_not_plain() {
        let bytes = cbor(&Fields {
            kind: "counter",
            key: "A",
        });
        let error =
            ciborium::from_reader::<Address, _>(bytes.as_slice())
                .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("`A` is not a valid actor kind or key"),
            "{error}"
        );
        let bytes = cbor(&Fields {
            kind: "counter",
            key: "a",
        });
        assert_eq!(
            ciborium::from_reader::<Address, _>(bytes.as_slice())
                .unwrap(),
            Address::new("counter", "a").unwrap()
        );
    }

    #[test]
    fn the_text_is_kind_slash_key() {
        let at = Address::new("counter", "a").unwrap();
        assert_eq!(at.to_string(), "counter/a");
        assert_eq!(Caller::Actor(at).to_string(), "counter/a");
        assert_eq!(Caller::Client.to_string(), "client");
    }

    /// The bytes captured from the base before `Address` was checked
    /// when built.
    #[test]
    fn an_address_has_the_bytes_it_had() {
        let at = Address::new("raw", "x").unwrap();
        assert_eq!(
            cbor(&at),
            b"\xa2\x64kind\x63raw\x63key\x61x".to_vec()
        );
    }
}
