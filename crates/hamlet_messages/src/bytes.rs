//! Bytes as the messages carry them: as text.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A SHA-256, written as 64 lowercase hex characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Sha256(pub [u8; 32]);

impl std::fmt::Display for Sha256 {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl Serialize for Sha256 {
    fn serialize<S: Serializer>(
        &self,
        to: S,
    ) -> Result<S::Ok, S::Error> {
        to.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Sha256 {
    fn deserialize<D: Deserializer<'de>>(
        from: D,
    ) -> Result<Self, D::Error> {
        let text = String::deserialize(from)?;
        let refused = || {
            D::Error::custom(
                "a SHA-256 is 64 lowercase hex characters",
            )
        };
        let nibble = |digit: u8| match digit {
            b'0'..=b'9' => Some(digit - b'0'),
            b'a'..=b'f' => Some(digit - b'a' + 10),
            _ => None,
        };
        if text.len() != 64 {
            return Err(refused());
        }
        let mut hash = [0; 32];
        for (byte, pair) in
            hash.iter_mut().zip(text.as_bytes().chunks(2))
        {
            let (Some(high), Some(low)) =
                (nibble(pair[0]), nibble(pair[1]))
            else {
                return Err(refused());
            };
            *byte = high << 4 | low;
        }
        Ok(Self(hash))
    }
}

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
