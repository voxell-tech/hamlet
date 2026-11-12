//! The id of what a client uploads: the SHA-256 of its bytes.

/// Makes `$id`, a string of 64 lowercase hexadecimal digits, and
/// `$invalid`, the error of text that is not one.
macro_rules! hash_id {
    (
        $(#[$meta:meta])*
        $id:ident, $invalid:ident, $words:literal
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $id(String);

        #[doc = concat!("Text that is not an [`", stringify!($id), "`].")]
        #[derive(Debug, PartialEq, thiserror::Error)]
        #[error($words)]
        pub struct $invalid;

        impl $id {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $id {
            type Error = $invalid;

            fn try_from(text: String) -> Result<Self, $invalid> {
                let digit =
                    |byte: u8| matches!(byte, b'0'..=b'9' | b'a'..=b'f');
                if text.len() == 64 && text.bytes().all(digit) {
                    Ok(Self(text))
                } else {
                    Err($invalid)
                }
            }
        }

        impl std::str::FromStr for $id {
            type Err = $invalid;

            fn from_str(text: &str) -> Result<Self, $invalid> {
                Self::try_from(text.to_owned())
            }
        }

        impl std::ops::Deref for $id {
            type Target = str;

            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $id {
            fn fmt(
                &self,
                f: &mut std::fmt::Formatter<'_>,
            ) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl serde::Serialize for $id {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $id {
            fn deserialize<D: serde::Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Self, D::Error> {
                Self::try_from(
                    <String as serde::Deserialize>::deserialize(
                        deserializer,
                    )?,
                )
                .map_err(serde::de::Error::custom)
            }
        }

        impl schemars::JsonSchema for $id {
            fn inline_schema() -> bool {
                true
            }

            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($id).into()
            }

            fn json_schema(
                _: &mut schemars::SchemaGenerator,
            ) -> schemars::Schema {
                schemars::json_schema!({
                    "type": "string",
                    "pattern": "^[0-9a-f]{64}$"
                })
            }
        }
    };
}

pub(super) use hash_id;
