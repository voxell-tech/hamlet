//! Bytes as the messages carry them: as text.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A hash of `$bytes` bytes, written as twice as many lowercase hex
/// characters.
macro_rules! hash {
    ($(#[$doc:meta])* $name:ident, $bytes:literal, $refused:literal) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name(pub [u8; $bytes]);

        impl $name {
            /// Reads its lowercase hex characters.
            pub fn from_hex(text: &str) -> Option<Self> {
                let nibble = |digit: u8| match digit {
                    b'0'..=b'9' => Some(digit - b'0'),
                    b'a'..=b'f' => Some(digit - b'a' + 10),
                    _ => None,
                };
                if text.len() != 2 * $bytes {
                    return None;
                }
                let mut hash = [0; $bytes];
                for (byte, pair) in
                    hash.iter_mut().zip(text.as_bytes().chunks(2))
                {
                    *byte = nibble(pair[0])? << 4 | nibble(pair[1])?;
                }
                Some(Self(hash))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(
                &self,
                f: &mut std::fmt::Formatter<'_>,
            ) -> std::fmt::Result {
                self.0
                    .iter()
                    .try_for_each(|byte| write!(f, "{byte:02x}"))
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(
                &self,
                to: S,
            ) -> Result<S::Ok, S::Error> {
                to.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(
                from: D,
            ) -> Result<Self, D::Error> {
                let text = String::deserialize(from)?;
                Self::from_hex(&text)
                    .ok_or_else(|| D::Error::custom($refused))
            }
        }
    };
}

hash!(
    /// A SHA-256, written as 64 lowercase hex characters.
    Sha256,
    32,
    "a SHA-256 is 64 lowercase hex characters"
);

hash!(
    /// A SHA-1, written as 40 lowercase hex characters.
    Sha1,
    20,
    "a SHA-1 is 40 lowercase hex characters"
);

/// Bytes as standard base64 text with padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Base64(pub Vec<u8>);

impl Serialize for Base64 {
    fn serialize<S: Serializer>(
        &self,
        to: S,
    ) -> Result<S::Ok, S::Error> {
        to.serialize_str(&STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Base64 {
    fn deserialize<D: Deserializer<'de>>(
        from: D,
    ) -> Result<Self, D::Error> {
        let text = String::deserialize(from)?;
        STANDARD.decode(text).map(Self).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{Base64, Sha256};

    #[test]
    fn a_hash_is_lowercase_hex_and_reads_back() {
        let hash = Sha256(std::array::from_fn(|at| at as u8 * 8));
        let text = serde_json::to_value(hash).unwrap();
        assert_eq!(
            text,
            json!(
                "0008101820283038404850586068707880889098a0a8b0b8c0c8d0d8e0e8f0f8"
            )
        );
        assert_eq!(
            serde_json::from_value::<Sha256>(text).unwrap(),
            hash
        );
    }

    #[test]
    fn a_hash_is_refused_unless_it_is_64_lowercase_hex_characters() {
        for text in [
            "AB".repeat(32),
            "ab".repeat(31),
            "ab".repeat(33),
            format!("{}zz", "ab".repeat(31)),
        ] {
            assert!(
                serde_json::from_value::<Sha256>(Value::String(
                    text.clone()
                ))
                .is_err(),
                "{text}"
            );
        }
    }

    #[test]
    fn bytes_are_padded_base64_and_read_back() {
        let bytes = Base64(b"GIF89a!".to_vec());
        let text = serde_json::to_value(&bytes).unwrap();
        assert_eq!(text, json!("R0lGODlhIQ=="));
        assert_eq!(
            serde_json::from_value::<Base64>(text).unwrap(),
            bytes
        );
        assert!(
            serde_json::from_value::<Base64>(json!("R0lGODlhIQ"))
                .is_err()
        );
    }
}
